use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::admin_events::EventBus;
use crate::config::{HeraldConfig, WebhookConfig};
use crate::engine::EngineSet;
use crate::integrations::{AuditEvent, ChronicleOps, CourierOps, SentryOps};
use crate::latency::Histogram;
use crate::registry::connection::ConnectionRegistry;
use crate::registry::presence::PresenceTracker;
use crate::registry::stream::StreamRegistry;
use crate::registry::typing::TypingTracker;
use crate::store;
use crate::webhook::WebhookEvent;

pub struct Metrics {
    pub events_published: AtomicU64,
    pub events_dropped: AtomicU64,
    pub events_acked: AtomicU64,
    pub events_redelivered: AtomicU64,
    pub ws_auth_failures: AtomicU64,
    // Latency histograms
    pub event_total: Histogram,
    pub event_store: Histogram,
    pub event_fanout: Histogram,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            events_published: AtomicU64::new(0),
            events_dropped: AtomicU64::new(0),
            events_acked: AtomicU64::new(0),
            events_redelivered: AtomicU64::new(0),
            ws_auth_failures: AtomicU64::new(0),
            event_total: Histogram::new(
                "herald_event_total_seconds",
                "Total event publish latency",
            ),
            event_store: Histogram::new("herald_event_store_seconds", "WAL store latency"),
            event_fanout: Histogram::new("herald_event_fanout_seconds", "Fan-out latency"),
        }
    }
}

impl Metrics {
    pub fn format_prometheus(&self, state: &AppState) -> String {
        let mut out = String::with_capacity(512);
        out.push_str("# HELP herald_connections_total Current WebSocket connections\n");
        out.push_str("# TYPE herald_connections_total gauge\n");
        out.push_str(&format!(
            "herald_connections_total {}\n",
            state.connections.total_connections()
        ));
        out.push_str("# HELP herald_streams_total Current streams\n");
        out.push_str("# TYPE herald_streams_total gauge\n");
        out.push_str(&format!(
            "herald_streams_total {}\n",
            state.streams.stream_count()
        ));
        out.push_str("# HELP herald_events_published_total Events published since startup\n");
        out.push_str("# TYPE herald_events_published_total counter\n");
        out.push_str(&format!(
            "herald_events_published_total {}\n",
            self.events_published.load(Ordering::Relaxed)
        ));
        out.push_str("# HELP herald_events_dropped_total Events dropped due to backpressure\n");
        out.push_str("# TYPE herald_events_dropped_total counter\n");
        out.push_str(&format!(
            "herald_events_dropped_total {}\n",
            self.events_dropped.load(Ordering::Relaxed)
        ));
        out.push_str("# HELP herald_events_acked_total Delivery acks received from clients\n");
        out.push_str("# TYPE herald_events_acked_total counter\n");
        out.push_str(&format!(
            "herald_events_acked_total {}\n",
            self.events_acked.load(Ordering::Relaxed)
        ));
        out.push_str(
            "# HELP herald_events_redelivered_total Events redelivered during ack-mode catchup\n",
        );
        out.push_str("# TYPE herald_events_redelivered_total counter\n");
        out.push_str(&format!(
            "herald_events_redelivered_total {}\n",
            self.events_redelivered.load(Ordering::Relaxed)
        ));
        out.push_str("# HELP herald_ws_auth_failures_total Failed WebSocket auth attempts\n");
        out.push_str("# TYPE herald_ws_auth_failures_total counter\n");
        out.push_str(&format!(
            "herald_ws_auth_failures_total {}\n",
            self.ws_auth_failures.load(Ordering::Relaxed)
        ));
        out.push_str("# HELP herald_uptime_seconds Server uptime\n");
        out.push_str("# TYPE herald_uptime_seconds gauge\n");
        out.push_str(&format!(
            "herald_uptime_seconds {}\n",
            state.start_time.elapsed().as_secs()
        ));

        // Latency histograms
        self.event_total.format_prometheus(&mut out);
        self.event_store.format_prometheus(&mut out);
        self.event_fanout.format_prometheus(&mut out);

        out
    }
}

/// Per-tenant HTTP API rate limit state.
pub struct RateLimitEntry {
    pub count: AtomicU32,
    pub window_start: std::sync::Mutex<std::time::Instant>,
}

/// How long a cached tenant config entry is considered fresh (5 minutes).
pub const TENANT_CACHE_TTL_SECS: u64 = 300;

type HmacSha256 = Hmac<Sha256>;

/// Cached tenant config for signed token validation.
#[derive(Clone)]
pub struct TenantConfig {
    pub id: String,
    pub key: String,
    pub secret: String,
    pub plan: String,
    pub event_ttl_days: Option<u32>,
    pub cached_at: std::time::Instant,
}

/// Per-tenant counters.
#[derive(Default)]
pub struct TenantMetrics {
    pub events_published: AtomicU64,
    pub webhooks_sent: AtomicU64,
}

impl TenantMetrics {
    pub fn new() -> Self {
        Self {
            events_published: AtomicU64::new(0),
            webhooks_sent: AtomicU64::new(0),
        }
    }
}

pub struct AppState {
    pub config: HeraldConfig,
    pub db: Arc<crate::store_backend::StoreBackend>,
    pub connections: ConnectionRegistry,
    pub streams: StreamRegistry,
    pub presence: PresenceTracker,
    pub typing: TypingTracker,
    pub tenant_cache: DashMap<String, TenantConfig>,
    pub key_cache: DashMap<String, String>,
    pub tenant_metrics: DashMap<String, TenantMetrics>,
    pub api_rate_limits: DashMap<String, Arc<RateLimitEntry>>,
    pub start_time: std::time::Instant,
    pub sentry: Option<Arc<dyn SentryOps>>,
    pub courier: Option<Arc<dyn CourierOps>>,
    pub chronicle: Option<Arc<dyn ChronicleOps>>,
    pub metrics: Metrics,
    pub event_bus: Arc<EventBus>,
    webhook_config: Option<Arc<WebhookConfig>>,
    webhook_client: reqwest::Client,
    /// Unique identifier for this instance (used for cluster dedup).
    pub instance_id: String,
    /// Set of recently-written event keys (primary keys in NS_EVENTS).
    /// Used by the backplane consumer to skip locally-originated events.
    pub local_writes: dashmap::DashSet<Vec<u8>>,
    /// Registered engines (chat, presence, etc.).
    pub engines: EngineSet,
}

pub struct AppStateBuilder {
    pub config: HeraldConfig,
    pub db: Arc<crate::store_backend::StoreBackend>,
    pub sentry: Option<Arc<dyn SentryOps>>,
    pub courier: Option<Arc<dyn CourierOps>>,
    pub chronicle: Option<Arc<dyn ChronicleOps>>,
    pub instance_id: String,
    pub engines: EngineSet,
}

impl AppState {
    pub fn build(b: AppStateBuilder) -> Arc<Self> {
        let webhook_config = b.config.webhook.as_ref().map(|w| {
            Arc::new(WebhookConfig {
                url: w.url.clone(),
                secret: w.secret.clone(),
                retries: w.retries,
                events: w.events.clone(),
            })
        });

        Arc::new(Self {
            config: b.config,
            db: b.db,
            connections: ConnectionRegistry::new(),
            streams: StreamRegistry::new(),
            presence: PresenceTracker::new(),
            typing: TypingTracker::new(),
            tenant_cache: DashMap::new(),
            key_cache: DashMap::new(),
            tenant_metrics: DashMap::new(),
            api_rate_limits: DashMap::new(),
            start_time: std::time::Instant::now(),
            sentry: b.sentry,
            courier: b.courier,
            chronicle: b.chronicle,
            metrics: Metrics::default(),
            event_bus: EventBus::new(),
            webhook_config,
            webhook_client: reqwest::Client::new(),
            instance_id: b.instance_id,
            local_writes: dashmap::DashSet::new(),
            engines: b.engines,
        })
    }

    /// Load all tenants from DB into cache.
    pub async fn hydrate_tenant_cache(&self) -> Result<(), anyhow::Error> {
        let tenants = store::tenants::list(&*self.db).await?;
        for t in tenants {
            self.key_cache.insert(t.key.clone(), t.id.clone());
            self.tenant_cache.insert(
                t.id.clone(),
                TenantConfig {
                    id: t.id,
                    key: t.key,
                    secret: t.secret,
                    plan: t.plan,
                    event_ttl_days: t
                        .config
                        .get("event_ttl_days")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as u32),
                    cached_at: std::time::Instant::now(),
                },
            );
        }
        Ok(())
    }

    /// Ensure a "default" tenant exists. On first startup, generates credentials
    /// and logs them. On subsequent startups, loads from DB (credentials already
    /// populated by hydrate_tenant_cache).
    pub async fn bootstrap_default_tenant(&self) -> Result<(), anyhow::Error> {
        let tenant_id = "default";

        if store::tenants::get(&*self.db, tenant_id).await?.is_some() {
            // Already exists (hydrated from cache)
            return Ok(());
        }

        // First startup — generate credentials
        let (key, secret) = store::tenants::generate_tenant_credentials();
        let tenant = store::tenants::Tenant {
            id: tenant_id.to_string(),
            name: "Default Tenant".to_string(),
            key: key.clone(),
            secret: secret.clone(),
            plan: "default".to_string(),
            config: serde_json::json!({}),
            created_at: crate::ws::connection::now_millis(),
        };
        store::tenants::insert(&*self.db, &tenant).await?;

        // Populate caches
        self.key_cache.insert(key.clone(), tenant_id.to_string());
        self.tenant_cache.insert(
            tenant_id.to_string(),
            TenantConfig {
                id: tenant_id.to_string(),
                key: key.clone(),
                secret: secret.clone(),
                plan: "default".to_string(),
                event_ttl_days: None,
                cached_at: std::time::Instant::now(),
            },
        );

        tracing::info!(
            tenant = %tenant_id,
            key = %key,
            secret = %secret,
            "default tenant created — save these credentials"
        );

        Ok(())
    }

    /// Validate a signed connection token.
    ///
    /// The token is an HMAC-SHA256 signature over the canonical payload:
    ///   "{user_id}:{sorted_streams}:{sorted_watchlist}"
    ///
    /// Returns (tenant_id, validated streams, validated watchlist) on success.
    pub async fn validate_signed_token(
        &self,
        key: &str,
        signature: &str,
        user_id: &str,
        streams: &[String],
        watchlist: &[String],
    ) -> Result<String, String> {
        // Look up key → tenant_id
        let tenant_id = self
            .key_cache
            .get(key)
            .map(|v| v.clone())
            .ok_or_else(|| "unknown key".to_string())?;

        // Look up tenant config (with TTL enforcement)
        let secret = {
            let tenant = self
                .tenant_cache
                .get(&tenant_id)
                .ok_or_else(|| "unknown tenant".to_string())?;

            // Enforce cache TTL — if stale, the caller should refresh
            if tenant.cached_at.elapsed().as_secs() > TENANT_CACHE_TTL_SECS {
                drop(tenant);
                // Refresh from DB
                match store::tenants::get(&*self.db, &tenant_id).await {
                    Ok(Some(t)) => {
                        self.key_cache.insert(t.key.clone(), t.id.clone());
                        let secret = t.secret.clone();
                        self.tenant_cache.insert(
                            t.id.clone(),
                            TenantConfig {
                                id: t.id,
                                key: t.key,
                                secret: secret.clone(),
                                plan: t.plan,
                                event_ttl_days: t
                                    .config
                                    .get("event_ttl_days")
                                    .and_then(|v| v.as_u64())
                                    .map(|v| v as u32),
                                cached_at: std::time::Instant::now(),
                            },
                        );
                        secret
                    }
                    Ok(None) => {
                        self.tenant_cache.remove(&tenant_id);
                        self.key_cache.remove(key);
                        return Err("unknown tenant".to_string());
                    }
                    Err(e) => {
                        tracing::warn!("cache refresh failed for tenant {tenant_id}: {e}");
                        return Err("internal error".to_string());
                    }
                }
            } else {
                tenant.secret.clone()
            }
        };

        // Build canonical payload
        let mut sorted_streams: Vec<&str> = streams.iter().map(|s| s.as_str()).collect();
        sorted_streams.sort();
        let mut sorted_watchlist: Vec<&str> = watchlist.iter().map(|s| s.as_str()).collect();
        sorted_watchlist.sort();
        let payload = format!(
            "{}:{}:{}",
            user_id,
            sorted_streams.join(","),
            sorted_watchlist.join(","),
        );

        // Verify HMAC-SHA256
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
            .map_err(|_| "internal error".to_string())?;
        mac.update(payload.as_bytes());

        let expected_sig =
            hex::decode(signature).map_err(|_| "invalid token format".to_string())?;
        mac.verify_slice(&expected_sig)
            .map_err(|_| "invalid token".to_string())?;

        Ok(tenant_id)
    }

    /// Returns the event TTL in milliseconds for a tenant.
    /// Uses per-tenant `event_ttl_days` from tenant config if set,
    /// otherwise falls back to the global `store.event_ttl_days`.
    pub fn event_ttl_ms(&self, tenant_id: &str) -> i64 {
        let days = self
            .tenant_cache
            .get(tenant_id)
            .and_then(|tc| tc.event_ttl_days)
            .unwrap_or(self.config.store.event_ttl_days);
        days as i64 * 24 * 60 * 60 * 1000
    }

    /// Allocate the next sequence number for a stream.
    /// In clustered mode, uses the shared store for coordination.
    /// In non-clustered mode, uses the per-process AtomicU64.
    pub async fn allocate_seq(
        &self,
        tenant_id: &str,
        stream_id: &str,
    ) -> Result<u64, anyhow::Error> {
        if self.config.cluster.enabled {
            store::sequences::allocate(&*self.db, tenant_id, stream_id).await
        } else {
            Ok(self.streams.next_seq(tenant_id, stream_id))
        }
    }

    /// Record a locally-written event key for backplane dedup.
    pub fn record_local_write(&self, key: Vec<u8>) {
        // Bounded: if the set grows too large, clear it (ring buffer semantics).
        // In practice this set is consumed quickly by the backplane consumer.
        if self.local_writes.len() > 100_000 {
            self.local_writes.clear();
        }
        self.local_writes.insert(key);
    }

    /// Check and remove a key from the local writes set.
    /// Returns true if the key was locally originated.
    pub fn consume_local_write(&self, key: &[u8]) -> bool {
        self.local_writes.remove(key).is_some()
    }

    pub fn increment_tenant_events(&self, tenant_id: &str) {
        self.tenant_metrics
            .entry(tenant_id.to_string())
            .or_default()
            .events_published
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_tenant_webhooks(&self, tenant_id: &str) {
        self.tenant_metrics
            .entry(tenant_id.to_string())
            .or_default()
            .webhooks_sent
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn tenant_events_published(&self, tenant_id: &str) -> u64 {
        self.tenant_metrics
            .get(tenant_id)
            .map(|m| m.events_published.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    pub fn tenant_webhooks_sent(&self, tenant_id: &str) -> u64 {
        self.tenant_metrics
            .get(tenant_id)
            .map(|m| m.webhooks_sent.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    pub fn fire_webhook(&self, tenant_id: &str, event: WebhookEvent) {
        if let Some(ref config) = self.webhook_config {
            // Check event filter
            if let Some(ref allowed) = config.events {
                if !allowed.iter().any(|e| e == &event.event) {
                    return;
                }
            }
            self.event_bus.increment_webhooks();
            self.increment_tenant_webhooks(tenant_id);
            crate::webhook::deliver(
                config.clone(),
                self.webhook_client.clone(),
                event,
                self.event_bus.clone(),
            );
        }
    }

    pub async fn authorize(
        &self,
        tenant_id: &str,
        subject: &str,
        action: &str,
        resource: &str,
    ) -> Result<(), String> {
        if let Some(ref sentry) = self.sentry {
            let scoped_resource = format!("herald:{tenant_id}:{resource}");
            sentry
                .evaluate(subject, action, &scoped_resource)
                .await
                .map_err(|e| e.to_string())
        } else {
            Ok(())
        }
    }

    pub fn notify_offline_members(
        &self,
        tenant_id: &str,
        stream_id: &str,
        sender: &str,
        body: &str,
    ) {
        let Some(ref courier) = self.courier else {
            return;
        };
        let members = self.streams.get_members(tenant_id, stream_id);
        let courier = courier.clone();
        let _tenant_id = tenant_id.to_string();
        let stream_id = stream_id.to_string();
        let sender = sender.to_string();
        let body = body.to_string();

        tokio::spawn(async move {
            for member in members {
                if member == sender {
                    continue;
                }
                let subject = format!("New event in {stream_id}");
                if let Err(e) = courier.notify("default", &member, &subject, &body).await {
                    tracing::warn!("courier notify failed for {member}: {e}");
                }
            }
        });
    }

    pub fn audit(
        &self,
        tenant_id: &str,
        operation: &str,
        resource_type: &str,
        resource_id: &str,
        actor: &str,
        result: &str,
    ) {
        let Some(ref chronicle) = self.chronicle else {
            return;
        };
        let event = AuditEvent {
            operation: operation.to_string(),
            resource_type: resource_type.to_string(),
            resource_id: resource_id.to_string(),
            actor: actor.to_string(),
            result: result.to_string(),
            tenant_id: Some(tenant_id.to_string()),
            diff: None,
            metadata: std::collections::HashMap::new(),
        };
        let chronicle = chronicle.clone();
        tokio::spawn(async move {
            if let Err(e) = chronicle.ingest(event).await {
                tracing::warn!("chronicle ingest failed: {e}");
            }
        });
    }

    pub fn audit_event(&self, event: AuditEvent) {
        let Some(ref chronicle) = self.chronicle else {
            return;
        };
        let chronicle = chronicle.clone();
        tokio::spawn(async move {
            if let Err(e) = chronicle.ingest(event).await {
                tracing::warn!("chronicle ingest failed: {e}");
            }
        });
    }
}
