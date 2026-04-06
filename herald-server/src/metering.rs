use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

// ── Wire types ──────────────────────────────────────────────────────

/// A single usage event to be ingested by Meterd.
#[derive(Debug, Clone, Serialize)]
pub struct IngestEvent {
    pub event_type: String,
    pub customer_id: String,
    pub value: f64,
    pub timestamp: i64,
    pub idempotency_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<serde_json::Value>,
}

/// Batch payload sent to `POST /v1/events/batch`.
#[derive(Debug, Serialize)]
struct BatchPayload {
    events: Vec<IngestEvent>,
}

/// Response from `POST /v1/quota/check`.
#[derive(Debug, Deserialize)]
pub struct QuotaCheckResponse {
    pub allowed: bool,
    pub current_usage: Option<u64>,
    pub limit: Option<u64>,
    pub remaining: Option<u64>,
    #[serde(default)]
    pub enforcement: Option<String>,
}

/// A single entry from Meterd's `GET /v1/quota/snapshot` response.
#[derive(Debug, Deserialize)]
struct QuotaSnapshotEntry {
    meter_slug: String,
    limit: Option<u64>,
}

/// Result of a quota check that includes overage context.
#[derive(Debug, Clone)]
pub struct QuotaResult {
    pub allowed: bool,
    pub remaining: Option<u64>,
    /// `true` when enforcement is `soft_cap` and usage exceeds the limit.
    /// The request should be allowed but the caller should emit a warning.
    pub soft_overage: bool,
    pub current_usage: Option<u64>,
    pub limit: Option<u64>,
}

// ── Circuit breaker ─────────────────────────────────────────────────

/// Circuit breaker states following the standard pattern.
/// - Closed: requests flow normally, failures are counted.
/// - Open: requests are rejected immediately (fail-open for metering).
/// - HalfOpen: a single probe request is allowed through to test recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

struct CircuitBreaker {
    state: Mutex<CircuitState>,
    failure_count: AtomicU64,
    last_failure: Mutex<Option<Instant>>,
    /// Number of consecutive failures before the circuit opens.
    threshold: u64,
    /// How long the circuit stays open before transitioning to half-open.
    cooldown: Duration,
}

impl CircuitBreaker {
    fn new(threshold: u64, cooldown: Duration) -> Self {
        Self {
            state: Mutex::new(CircuitState::Closed),
            failure_count: AtomicU64::new(0),
            last_failure: Mutex::new(None),
            threshold,
            cooldown,
        }
    }

    /// Returns `true` if the request should be allowed through.
    fn allow_request(&self) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        match *state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                let last = self.last_failure.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(t) = *last {
                    if t.elapsed() >= self.cooldown {
                        *state = CircuitState::HalfOpen;
                        debug!("metering circuit breaker: open → half-open");
                        return true;
                    }
                }
                false
            }
            CircuitState::HalfOpen => {
                // Only one probe request allowed — block the rest until we
                // get a success or failure on the probe.
                false
            }
        }
    }

    fn record_success(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if *state == CircuitState::HalfOpen {
            info!("metering circuit breaker: half-open → closed (probe succeeded)");
        }
        *state = CircuitState::Closed;
        self.failure_count.store(0, Ordering::Relaxed);
    }

    fn record_failure(&self) {
        let count = self.failure_count.fetch_add(1, Ordering::Relaxed) + 1;
        *self.last_failure.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());

        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if *state == CircuitState::HalfOpen || count >= self.threshold {
            if *state != CircuitState::Open {
                warn!(
                    failures = count,
                    "metering circuit breaker: → open ({count} failures)"
                );
            }
            *state = CircuitState::Open;
        }
    }

    #[cfg(test)]
    fn is_open(&self) -> bool {
        *self.state.lock().unwrap() == CircuitState::Open
    }
}

// ── Configuration ───────────────────────────────────────────────────

/// Configuration for the Meterd metering integration.
#[derive(Debug, Clone)]
pub struct MeteringConfig {
    pub enabled: bool,
    pub api_key: String,
    pub base_url: String,
    pub flush_interval_secs: u64,
    pub batch_size: usize,
}

impl MeteringConfig {
    /// Build metering config from environment variables.
    pub fn from_env() -> Self {
        let env = |key: &str| std::env::var(key).ok();
        let env_or =
            |key: &str, default: &str| std::env::var(key).unwrap_or_else(|_| default.to_string());

        Self {
            enabled: env("HERALD_METERING_ENABLED")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false),
            api_key: env_or("HERALD_METERING_API_KEY", ""),
            base_url: env_or("HERALD_METERING_URL", "https://api.meterd.io"),
            flush_interval_secs: env("HERALD_METERING_FLUSH_INTERVAL")
                .and_then(|v| v.parse().ok())
                .unwrap_or(10),
            batch_size: env("HERALD_METERING_BATCH_SIZE")
                .and_then(|v| v.parse().ok())
                .unwrap_or(100),
        }
    }
}

// ── Client ──────────────────────────────────────────────────────────

/// Lightweight Meterd HTTP client with local buffering and circuit breaker.
///
/// Usage events are queued via `track()` (non-blocking) and periodically
/// flushed to the Meterd API by a background task. Quota checks are
/// synchronous HTTP calls gated by the circuit breaker — when the breaker
/// is open, quota checks fail-open (allow the request).
pub struct MeteringClient {
    config: MeteringConfig,
    http: reqwest::Client,
    buffer: Mutex<Vec<IngestEvent>>,
    breaker: CircuitBreaker,
}

impl MeteringClient {
    pub fn new(config: MeteringConfig) -> Self {
        Self {
            config,
            http: reqwest::Client::new(),
            buffer: Mutex::new(Vec::new()),
            // 5 consecutive failures → open, 30s cooldown (matches CLAUDE.md rules)
            breaker: CircuitBreaker::new(5, Duration::from_secs(30)),
        }
    }

    /// Queue a usage event for batch submission. Non-blocking.
    pub fn track(
        &self,
        event_type: &str,
        customer_id: &str,
        value: f64,
        properties: Option<serde_json::Value>,
    ) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let timestamp = now.as_secs() as i64;
        let random = uuid::Uuid::new_v4();
        let idempotency_key = format!("herald_{}_{}", now.as_millis(), random);

        let event = IngestEvent {
            event_type: event_type.to_string(),
            customer_id: customer_id.to_string(),
            value,
            timestamp,
            idempotency_key,
            properties,
        };

        if let Ok(mut buf) = self.buffer.lock() {
            buf.push(event);
        } else {
            warn!("metering: failed to acquire buffer lock, dropping event");
        }
    }

    /// Flush pending events to Meterd. Called periodically by background task.
    /// Gated by circuit breaker — when open, events stay buffered.
    pub async fn flush(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if !self.breaker.allow_request() {
            debug!("metering: circuit breaker open, skipping flush");
            return Ok(());
        }

        let events = {
            let mut buf = self
                .buffer
                .lock()
                .map_err(|e| format!("metering: buffer lock poisoned: {e}"))?;
            if buf.is_empty() {
                return Ok(());
            }
            // Drain up to batch_size events
            let drain_count = buf.len().min(self.config.batch_size);
            buf.drain(..drain_count).collect::<Vec<_>>()
        };

        let count = events.len();
        debug!(count, "metering: flushing events to Meterd");

        let url = format!("{}/v1/events/batch", self.config.base_url);
        let payload = BatchPayload {
            events: events.clone(),
        };

        let result = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .json(&payload)
            .timeout(Duration::from_secs(10))
            .send()
            .await;

        match result {
            Ok(resp) if resp.status().is_success() => {
                self.breaker.record_success();
                debug!(count, "metering: flushed events successfully");
                Ok(())
            }
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                self.breaker.record_failure();
                // Re-buffer events so they aren't lost
                if let Ok(mut buf) = self.buffer.lock() {
                    // Prepend failed events so they're retried first
                    let mut retry = events;
                    retry.extend(buf.drain(..));
                    *buf = retry;
                }
                warn!(
                    status = %status,
                    body = %body,
                    "metering: Meterd batch ingest failed, {count} events re-buffered"
                );
                Err(format!("Meterd returned {status}: {body}").into())
            }
            Err(e) => {
                self.breaker.record_failure();
                // Re-buffer events so they aren't lost
                if let Ok(mut buf) = self.buffer.lock() {
                    let mut retry = events;
                    retry.extend(buf.drain(..));
                    *buf = retry;
                }
                warn!(error = %e, "metering: Meterd request failed, {count} events re-buffered");
                Err(e.into())
            }
        }
    }

    /// Check quota with Meterd. Returns a `QuotaResult` with enforcement context.
    ///
    /// When the circuit breaker is open, this **fails open** — the request is
    /// allowed through with no usage data. This matches the project rule:
    /// "Sentry fail-open (permit when circuit trips)."
    pub async fn check_quota(
        &self,
        meter_slug: &str,
        customer_id: &str,
        requested: u64,
    ) -> QuotaResult {
        if !self.breaker.allow_request() {
            debug!("metering: circuit breaker open, failing open for quota check");
            return QuotaResult {
                allowed: true,
                remaining: None,
                soft_overage: false,
                current_usage: None,
                limit: None,
            };
        }

        let url = format!("{}/v1/quota/check", self.config.base_url);

        let body = serde_json::json!({
            "meter_slug": meter_slug,
            "customer_external_id": customer_id,
            "requested": requested,
        });

        let result = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .json(&body)
            .timeout(Duration::from_secs(5))
            .send()
            .await;

        match result {
            Ok(resp) if resp.status().is_success() => {
                self.breaker.record_success();
                match resp.json::<QuotaCheckResponse>().await {
                    Ok(qr) => {
                        let soft_overage = qr
                            .enforcement
                            .as_deref()
                            .map(|e| e == "soft_cap")
                            .unwrap_or(false)
                            && !qr.allowed;
                        QuotaResult {
                            // soft_cap overages are allowed through with a warning
                            allowed: qr.allowed || soft_overage,
                            remaining: qr.remaining,
                            soft_overage,
                            current_usage: qr.current_usage,
                            limit: qr.limit,
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "metering: failed to parse quota response, failing open");
                        QuotaResult {
                            allowed: true,
                            remaining: None,
                            soft_overage: false,
                            current_usage: None,
                            limit: None,
                        }
                    }
                }
            }
            Ok(resp) => {
                let status = resp.status();
                self.breaker.record_failure();
                warn!(status = %status, "metering: Meterd quota check failed, failing open");
                QuotaResult {
                    allowed: true,
                    remaining: None,
                    soft_overage: false,
                    current_usage: None,
                    limit: None,
                }
            }
            Err(e) => {
                self.breaker.record_failure();
                warn!(error = %e, "metering: Meterd quota request failed, failing open");
                QuotaResult {
                    allowed: true,
                    remaining: None,
                    soft_overage: false,
                    current_usage: None,
                    limit: None,
                }
            }
        }
    }

    /// Fetch plan limits from Meterd's quota snapshot endpoint.
    ///
    /// Maps Meterd meter slugs to `PlanLimits` fields:
    ///   - `max_connections` → meter `max_connections`
    ///   - `max_streams` → meter `max_streams`
    ///   - `api_rate_limit` → meter `api_rate_limit`
    ///   - `events_per_month` → meter `events_per_month`
    ///   - `retention_days` → meter `retention_days`
    ///
    /// Returns `None` if the call fails (circuit breaker or network error),
    /// letting the caller fall back to built-in defaults.
    pub async fn fetch_plan_limits(&self, customer_id: &str) -> Option<crate::config::PlanLimits> {
        if !self.breaker.allow_request() {
            debug!("metering: circuit breaker open, skipping plan limits fetch");
            return None;
        }

        let url = format!(
            "{}/v1/quota/snapshot?customer={}",
            self.config.base_url, customer_id
        );

        let result = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .timeout(Duration::from_secs(5))
            .send()
            .await;

        match result {
            Ok(resp) if resp.status().is_success() => {
                self.breaker.record_success();
                match resp.json::<Vec<QuotaSnapshotEntry>>().await {
                    Ok(entries) => {
                        let mut max_connections: Option<u32> = None;
                        let mut max_streams: Option<u32> = None;
                        let mut api_rate_limit: Option<u32> = None;
                        let mut events_per_month: Option<u64> = None;
                        let mut retention_days: Option<u32> = None;

                        for entry in &entries {
                            match entry.meter_slug.as_str() {
                                "max_connections" => {
                                    max_connections = entry.limit.map(|l| l as u32);
                                }
                                "max_streams" => {
                                    max_streams = entry.limit.map(|l| l as u32);
                                }
                                "api_rate_limit" => {
                                    api_rate_limit = entry.limit.map(|l| l as u32);
                                }
                                "events_per_month" | "events_published" => {
                                    events_per_month = entry.limit;
                                }
                                "retention_days" => {
                                    retention_days = entry.limit.map(|l| l as u32);
                                }
                                slug => {
                                    debug!(slug, "metering: unknown meter slug in quota snapshot");
                                }
                            }
                        }

                        // All required meters must be present in the snapshot
                        let missing: Vec<&str> = [
                            max_connections.is_none().then_some("max_connections"),
                            max_streams.is_none().then_some("max_streams"),
                            api_rate_limit.is_none().then_some("api_rate_limit"),
                            events_per_month.is_none().then_some("events_per_month"),
                            retention_days.is_none().then_some("retention_days"),
                        ]
                        .into_iter()
                        .flatten()
                        .collect();

                        if !missing.is_empty() {
                            warn!(
                                customer = customer_id,
                                ?missing,
                                "metering: quota snapshot missing expected meters"
                            );
                            return None;
                        }

                        Some(crate::config::PlanLimits {
                            max_connections: max_connections?,
                            max_streams: max_streams?,
                            api_rate_limit: api_rate_limit?,
                            events_per_month: events_per_month?,
                            retention_days: retention_days?,
                        })
                    }
                    Err(e) => {
                        warn!(error = %e, "metering: failed to parse quota snapshot");
                        None
                    }
                }
            }
            Ok(resp) => {
                let status = resp.status();
                self.breaker.record_failure();
                warn!(status = %status, "metering: quota snapshot fetch failed");
                None
            }
            Err(e) => {
                self.breaker.record_failure();
                warn!(error = %e, "metering: quota snapshot request failed");
                None
            }
        }
    }

    /// Returns the number of events currently buffered.
    pub fn pending_count(&self) -> usize {
        self.buffer.lock().map(|b| b.len()).unwrap_or(0)
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> MeteringConfig {
        MeteringConfig {
            enabled: true,
            api_key: "test-key".to_string(),
            base_url: "http://localhost:9999".to_string(),
            flush_interval_secs: 10,
            batch_size: 100,
        }
    }

    #[test]
    fn track_buffers_events() {
        let client = MeteringClient::new(test_config());
        assert_eq!(client.pending_count(), 0);

        client.track("events_published", "tenant-1", 1.0, None);
        client.track("webhooks_sent", "tenant-1", 1.0, None);
        assert_eq!(client.pending_count(), 2);
    }

    #[test]
    fn track_with_properties() {
        let client = MeteringClient::new(test_config());
        let props = serde_json::json!({"stream": "chat-general"});
        client.track("events_published", "tenant-1", 1.0, Some(props));
        assert_eq!(client.pending_count(), 1);

        let buf = client.buffer.lock().unwrap();
        assert!(buf[0].properties.is_some());
        assert!(buf[0].idempotency_key.starts_with("herald_"));
    }

    #[tokio::test]
    async fn flush_empty_buffer_is_noop() {
        let client = MeteringClient::new(test_config());
        let result = client.flush().await;
        assert!(result.is_ok());
    }

    #[test]
    fn config_from_env_defaults() {
        let config = MeteringConfig::from_env();
        assert!(!config.enabled);
        assert_eq!(config.base_url, "https://api.meterd.io");
        assert_eq!(config.flush_interval_secs, 10);
        assert_eq!(config.batch_size, 100);
    }

    #[test]
    fn circuit_breaker_opens_after_threshold() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(30));
        assert!(cb.allow_request());

        cb.record_failure();
        cb.record_failure();
        assert!(cb.allow_request()); // 2 failures, threshold is 3

        cb.record_failure();
        assert!(cb.is_open());
        assert!(!cb.allow_request()); // circuit is open
    }

    #[test]
    fn circuit_breaker_resets_on_success() {
        let cb = CircuitBreaker::new(2, Duration::from_secs(0)); // 0s cooldown for test
        cb.record_failure();
        cb.record_failure();
        assert!(cb.is_open());

        // With 0s cooldown, the next allow_request should transition to half-open
        std::thread::sleep(Duration::from_millis(10));
        assert!(cb.allow_request()); // half-open probe

        cb.record_success();
        assert!(!cb.is_open()); // back to closed
        assert!(cb.allow_request());
    }

    #[tokio::test]
    async fn check_quota_fails_open_when_circuit_open() {
        let client = MeteringClient::new(test_config());
        // Trip the breaker
        for _ in 0..5 {
            client.breaker.record_failure();
        }
        assert!(client.breaker.is_open());

        let result = client.check_quota("events_published", "tenant-1", 1).await;
        assert!(result.allowed); // fail-open
        assert!(!result.soft_overage);
    }

    #[tokio::test]
    async fn flush_skips_when_circuit_open() {
        let client = MeteringClient::new(test_config());
        client.track("events_published", "tenant-1", 1.0, None);

        for _ in 0..5 {
            client.breaker.record_failure();
        }
        assert!(client.breaker.is_open());

        let result = client.flush().await;
        assert!(result.is_ok());
        // Events should still be buffered (not drained)
        assert_eq!(client.pending_count(), 1);
    }
}
