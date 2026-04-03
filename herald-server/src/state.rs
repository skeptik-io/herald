use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use jsonwebtoken::{DecodingKey, Validation};

use crate::admin_events::EventBus;
use crate::config::{HeraldConfig, WebhookConfig};
use crate::integrations::{
    AuditEvent, ChronicleOps, CipherOps, CourierOps, SearchHit, SentryOps, VeilOps,
};
use crate::latency::Histogram;
use crate::registry::connection::ConnectionRegistry;
use crate::registry::presence::PresenceTracker;
use crate::registry::room::RoomRegistry;
use crate::store;
use crate::webhook::WebhookEvent;

pub struct Metrics {
    pub messages_sent: AtomicU64,
    pub messages_dropped: AtomicU64,
    pub ws_auth_failures: AtomicU64,
    // Latency histograms
    pub message_total: Histogram,
    pub message_encrypt: Histogram,
    pub message_store: Histogram,
    pub message_index: Histogram,
    pub message_fanout: Histogram,
    pub message_decrypt: Histogram,
    pub search: Histogram,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            messages_sent: AtomicU64::new(0),
            messages_dropped: AtomicU64::new(0),
            ws_auth_failures: AtomicU64::new(0),
            message_total: Histogram::new(
                "herald_message_total_seconds",
                "Total message send latency",
            ),
            message_encrypt: Histogram::new(
                "herald_message_encrypt_seconds",
                "Cipher encrypt latency",
            ),
            message_store: Histogram::new("herald_message_store_seconds", "Postgres store latency"),
            message_index: Histogram::new("herald_message_index_seconds", "Veil index latency"),
            message_fanout: Histogram::new("herald_message_fanout_seconds", "Fan-out latency"),
            message_decrypt: Histogram::new(
                "herald_message_decrypt_seconds",
                "Cipher decrypt latency (fetch/search)",
            ),
            search: Histogram::new("herald_search_seconds", "Veil search latency"),
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
        out.push_str("# HELP herald_rooms_total Current rooms\n");
        out.push_str("# TYPE herald_rooms_total gauge\n");
        out.push_str(&format!(
            "herald_rooms_total {}\n",
            state.rooms.room_count()
        ));
        out.push_str("# HELP herald_messages_sent_total Messages sent since startup\n");
        out.push_str("# TYPE herald_messages_sent_total counter\n");
        out.push_str(&format!(
            "herald_messages_sent_total {}\n",
            self.messages_sent.load(Ordering::Relaxed)
        ));
        out.push_str("# HELP herald_messages_dropped_total Messages dropped due to backpressure\n");
        out.push_str("# TYPE herald_messages_dropped_total counter\n");
        out.push_str(&format!(
            "herald_messages_dropped_total {}\n",
            self.messages_dropped.load(Ordering::Relaxed)
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
        self.message_total.format_prometheus(&mut out);
        self.message_encrypt.format_prometheus(&mut out);
        self.message_store.format_prometheus(&mut out);
        self.message_index.format_prometheus(&mut out);
        self.message_fanout.format_prometheus(&mut out);
        self.message_decrypt.format_prometheus(&mut out);
        self.search.format_prometheus(&mut out);

        out
    }
}

/// Cached tenant config for JWT validation.
#[derive(Clone)]
pub struct TenantConfig {
    pub id: String,
    pub jwt_secret: String,
    pub jwt_issuer: Option<String>,
    pub plan: String,
}

/// Per-tenant counters.
#[derive(Default)]
pub struct TenantMetrics {
    pub messages_sent: AtomicU64,
    pub webhooks_sent: AtomicU64,
}

impl TenantMetrics {
    pub fn new() -> Self {
        Self {
            messages_sent: AtomicU64::new(0),
            webhooks_sent: AtomicU64::new(0),
        }
    }
}

pub struct AppState {
    pub config: HeraldConfig,
    pub db: Arc<shroudb_storage::EmbeddedStore>,
    pub connections: ConnectionRegistry,
    pub rooms: RoomRegistry,
    pub presence: PresenceTracker,
    pub tenant_cache: DashMap<String, TenantConfig>,
    pub tenant_metrics: DashMap<String, TenantMetrics>,
    pub start_time: std::time::Instant,
    pub cipher: Option<Arc<dyn CipherOps>>,
    pub veil: Option<Arc<dyn VeilOps>>,
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
    pub db: Arc<shroudb_storage::EmbeddedStore>,
    pub cipher: Option<Arc<dyn CipherOps>>,
    pub veil: Option<Arc<dyn VeilOps>>,
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
            })
        });

        Arc::new(Self {
            config: b.config,
            db: b.db,
            connections: ConnectionRegistry::new(),
            rooms: RoomRegistry::new(),
            presence: PresenceTracker::new(),
            tenant_cache: DashMap::new(),
            tenant_metrics: DashMap::new(),
            start_time: std::time::Instant::now(),
            cipher: b.cipher,
            veil: b.veil,
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
                store::tenants::create_token(&*self.db, token, &tenant_id).await?;
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

    pub fn increment_tenant_messages(&self, tenant_id: &str) {
        self.tenant_metrics
            .entry(tenant_id.to_string())
            .or_default()
            .messages_sent
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_tenant_webhooks(&self, tenant_id: &str) {
        self.tenant_metrics
            .entry(tenant_id.to_string())
            .or_default()
            .webhooks_sent
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn tenant_messages_sent(&self, tenant_id: &str) -> u64 {
        self.tenant_metrics
            .get(tenant_id)
            .map(|m| m.messages_sent.load(Ordering::Relaxed))
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

    pub async fn encrypt_body(
        &self,
        tenant_id: &str,
        room_id: &str,
        body: &str,
    ) -> Result<String, String> {
        let mode = self.rooms.encryption_mode(tenant_id, room_id);
        if mode != herald_core::room::EncryptionMode::ServerEncrypted {
            return Ok(body.to_string());
        }
        let cipher = self
            .cipher
            .as_ref()
            .ok_or_else(|| "cipher not configured for encrypted room".to_string())?;
        let keyring = format!("herald-{tenant_id}-room-{room_id}");
        cipher
            .encrypt(&keyring, body, Some(room_id))
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn decrypt_body(
        &self,
        tenant_id: &str,
        room_id: &str,
        body: &str,
    ) -> Result<String, String> {
        let mode = self.rooms.encryption_mode(tenant_id, room_id);
        if mode != herald_core::room::EncryptionMode::ServerEncrypted {
            return Ok(body.to_string());
        }
        let cipher = self
            .cipher
            .as_ref()
            .ok_or_else(|| "cipher not configured for encrypted room".to_string())?;
        let keyring = format!("herald-{tenant_id}-room-{room_id}");
        cipher
            .decrypt(&keyring, body, Some(room_id))
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn index_message(
        &self,
        tenant_id: &str,
        room_id: &str,
        message_id: &str,
        body: &str,
    ) {
        if let Some(ref veil) = self.veil {
            let index = format!("herald-{tenant_id}-room-{room_id}");
            if let Err(e) = veil.put(&index, message_id, body).await {
                tracing::warn!("veil indexing failed for {message_id}: {e}");
            }
        }
    }

    pub async fn search_messages(
        &self,
        tenant_id: &str,
        room_id: &str,
        query: &str,
        limit: Option<usize>,
    ) -> Result<Vec<SearchHit>, String> {
        let veil = self
            .veil
            .as_ref()
            .ok_or_else(|| "search not available".to_string())?;
        let index = format!("herald-{tenant_id}-room-{room_id}");
        veil.search(&index, query, limit)
            .await
            .map_err(|e| e.to_string())
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

    pub fn notify_offline_members(&self, tenant_id: &str, room_id: &str, sender: &str, body: &str) {
        let Some(ref courier) = self.courier else {
            return;
        };
        let members = self.rooms.get_members(tenant_id, room_id);
        let courier = courier.clone();
        let _tenant_id = tenant_id.to_string();
        let room_id = room_id.to_string();
        let sender = sender.to_string();
        let body = body.to_string();

        tokio::spawn(async move {
            for member in members {
                if member == sender {
                    continue;
                }
                let subject = format!("New message in {room_id}");
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
