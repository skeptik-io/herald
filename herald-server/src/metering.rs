use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

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
struct QuotaCheckResponse {
    allowed: bool,
    #[allow(dead_code)]
    current_usage: Option<u64>,
    #[allow(dead_code)]
    limit: Option<u64>,
    remaining: Option<u64>,
}

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

/// Lightweight Meterd HTTP client with local buffering.
///
/// Usage events are queued via `track()` (non-blocking) and periodically
/// flushed to the Meterd API by a background task.
pub struct MeteringClient {
    config: MeteringConfig,
    http: reqwest::Client,
    buffer: Mutex<Vec<IngestEvent>>,
}

impl MeteringClient {
    pub fn new(config: MeteringConfig) -> Self {
        Self {
            config,
            http: reqwest::Client::new(),
            buffer: Mutex::new(Vec::new()),
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
    pub async fn flush(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
        let payload = BatchPayload { events };

        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .json(&payload)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            warn!(
                status = %status,
                body = %body,
                "metering: Meterd batch ingest failed"
            );
            return Err(format!("Meterd returned {status}: {body}").into());
        }

        debug!(count, "metering: flushed events successfully");
        Ok(())
    }

    /// Check quota with Meterd. Returns (allowed, remaining).
    pub async fn check_quota(
        &self,
        meter_slug: &str,
        customer_id: &str,
        requested: u64,
    ) -> Result<(bool, Option<u64>), Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{}/v1/quota/check", self.config.base_url);

        let body = serde_json::json!({
            "meter_slug": meter_slug,
            "customer_external_id": customer_id,
            "requested": requested,
        });

        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .json(&body)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let resp_body = resp.text().await.unwrap_or_default();
            warn!(
                status = %status,
                body = %resp_body,
                "metering: Meterd quota check failed"
            );
            return Err(format!("Meterd returned {status}: {resp_body}").into());
        }

        let result: QuotaCheckResponse = resp.json().await?;
        Ok((result.allowed, result.remaining))
    }

    /// Returns the number of events currently buffered.
    pub fn pending_count(&self) -> usize {
        self.buffer.lock().map(|b| b.len()).unwrap_or(0)
    }
}

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
        // Should succeed without making any HTTP call
        let result = client.flush().await;
        assert!(result.is_ok());
    }

    #[test]
    fn config_from_env_defaults() {
        // With no env vars set, should get defaults
        let config = MeteringConfig::from_env();
        assert!(!config.enabled);
        assert_eq!(config.base_url, "https://api.meterd.io");
        assert_eq!(config.flush_interval_secs, 10);
        assert_eq!(config.batch_size, 100);
    }
}
