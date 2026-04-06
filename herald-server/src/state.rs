use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use jsonwebtoken::{DecodingKey, Validation};

use crate::admin_events::EventBus;
use crate::config::{HeraldConfig, WebhookConfig};
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

/// Cached tenant config for JWT validation.
#[derive(Clone)]
pub struct TenantConfig {
    pub id: String,
    pub jwt_secret: String,
    pub jwt_issuer: Option<String>,
    pub plan: String,
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
}

pub struct AppStateBuilder {
    pub config: HeraldConfig,
    pub db: Arc<crate::store_backend::StoreBackend>,
    pub sentry: Option<Arc<dyn SentryOps>>,
    pub courier: Option<Arc<dyn CourierOps>>,
    pub chronicle: Option<Arc<dyn ChronicleOps>>,
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
        })
    }

    /// Load all tenants from DB into cache.
    pub async fn hydrate_tenant_cache(&self) -> Result<(), anyhow::Error> {
        let tenants = store::tenants::list(&*self.db).await?;
        for t in tenants {
            self.tenant_cache.insert(
                t.id.clone(),
                TenantConfig {
                    id: t.id,
                    jwt_secret: t.jwt_secret,
                    jwt_issuer: t.jwt_issuer,
                    plan: t.plan,
                    cached_at: std::time::Instant::now(),
                },
            );
        }
        Ok(())
    }

    /// Bootstrap a single-tenant setup: create a "default" tenant from config-level auth settings.
    /// In single-tenant mode, all JWTs use the config jwt_secret and the tenant claim is "default".
    pub async fn bootstrap_single_tenant(&self) -> Result<String, anyhow::Error> {
        let tenant_id = "default".to_string();
        let jwt_secret =
            self.config.auth.jwt_secret.clone().ok_or_else(|| {
                anyhow::anyhow!("auth.jwt_secret required for single-tenant mode")
            })?;
        let jwt_issuer = self.config.auth.jwt_issuer.clone();

        // Create tenant if it doesn't exist
        let existing = store::tenants::get(&*self.db, &tenant_id).await?;
        if existing.is_none() {
            let tenant = store::tenants::Tenant {
                id: tenant_id.clone(),
                name: "Default Tenant".to_string(),
                jwt_secret: jwt_secret.clone(),
                jwt_issuer: jwt_issuer.clone(),
                plan: "self-hosted".to_string(),
                config: serde_json::json!({}),
                created_at: crate::ws::connection::now_millis(),
            };
            store::tenants::insert(&*self.db, &tenant).await?;
        }

        // Create API token from config if any
        for token in &self.config.auth.api.tokens {
            let existing = store::tenants::validate_token(&*self.db, token).await?;
            if existing.is_none() {
                store::tenants::create_token(&*self.db, token, &tenant_id, None).await?;
            }
        }

        // Add to cache
        self.tenant_cache.insert(
            tenant_id.clone(),
            TenantConfig {
                id: tenant_id.clone(),
                jwt_secret,
                jwt_issuer,
                plan: "self-hosted".to_string(),
                cached_at: std::time::Instant::now(),
            },
        );

        Ok(tenant_id)
    }

    /// Validate a JWT against the tenant's secret.
    /// Returns (tenant_id, claims) on success.
    pub fn validate_jwt(
        &self,
        token: &str,
    ) -> Result<(String, herald_core::auth::JwtClaims), String> {
        // First, decode without verification to extract the tenant claim
        let mut insecure = Validation::default();
        insecure.insecure_disable_signature_validation();
        insecure.set_required_spec_claims::<&str>(&[]);
        insecure.validate_exp = false;

        let peek = jsonwebtoken::decode::<herald_core::auth::JwtClaims>(
            token,
            &DecodingKey::from_secret(b"unused"),
            &insecure,
        )
        .map_err(|e| format!("invalid JWT structure: {e}"))?;

        let tenant_id = &peek.claims.tenant;
        if tenant_id.is_empty() {
            return Err("missing tenant claim in JWT".to_string());
        }

        // Look up tenant
        let tenant = self
            .tenant_cache
            .get(tenant_id)
            .ok_or_else(|| format!("unknown tenant: {tenant_id}"))?;

        // Validate with tenant's secret
        let key = DecodingKey::from_secret(tenant.jwt_secret.as_bytes());
        let mut validation = Validation::new(jsonwebtoken::Algorithm::HS256);
        if let Some(ref issuer) = tenant.jwt_issuer {
            validation.set_issuer(&[issuer]);
        }
        validation.set_required_spec_claims(&["sub", "exp", "iat"]);

        let data = jsonwebtoken::decode::<herald_core::auth::JwtClaims>(token, &key, &validation)
            .map_err(|e| format!("JWT validation failed: {e}"))?;

        Ok((tenant_id.clone(), data.claims))
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
        resource: &str,
        actor: &str,
        result: &str,
    ) {
        let Some(ref chronicle) = self.chronicle else {
            return;
        };
        let mut metadata = std::collections::HashMap::new();
        metadata.insert("tenant".to_string(), tenant_id.to_string());
        let event = AuditEvent {
            operation: operation.to_string(),
            resource: resource.to_string(),
            actor: actor.to_string(),
            result: result.to_string(),
            metadata,
        };
        let chronicle = chronicle.clone();
        tokio::spawn(async move {
            if let Err(e) = chronicle.ingest(event).await {
                tracing::warn!("chronicle ingest failed: {e}");
            }
        });
    }
}
