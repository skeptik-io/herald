use base64::Engine;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct HeraldConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub store: StoreConfig,
    pub auth: AuthConfig,
    #[serde(default)]
    pub presence: PresenceConfig,
    #[serde(default)]
    pub webhook: Option<WebhookConfig>,
    #[serde(default)]
    pub shroudb: Option<ShroudbConfig>,
    #[serde(default)]
    pub tls: Option<TlsConfig>,
    #[serde(default)]
    pub tenant_limits: TenantLimitsConfig,
    #[serde(default)]
    pub cors: Option<CorsConfig>,
    #[serde(default)]
    pub cluster: ClusterConfig,
}

#[derive(Debug, Deserialize)]
pub struct TenantLimitsConfig {
    #[serde(default = "default_max_connections_per_tenant")]
    pub max_connections_per_tenant: u32,
    #[serde(default = "default_max_streams_per_tenant")]
    pub max_streams_per_tenant: u32,
}

impl Default for TenantLimitsConfig {
    fn default() -> Self {
        Self {
            max_connections_per_tenant: default_max_connections_per_tenant(),
            max_streams_per_tenant: default_max_streams_per_tenant(),
        }
    }
}

fn default_max_connections_per_tenant() -> u32 {
    10000
}
fn default_max_streams_per_tenant() -> u32 {
    10000
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct ShroudbConfig {
    pub sentry_addr: Option<String>,
    pub sentry_token: Option<String>,
    pub courier_addr: Option<String>,
    pub courier_token: Option<String>,
    pub chronicle_addr: Option<String>,
    pub chronicle_token: Option<String>,
    pub auth_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_max_messages_per_sec")]
    pub max_messages_per_sec: u32,
    #[serde(default = "default_api_rate_limit")]
    pub api_rate_limit: u32,
    #[serde(default = "default_shutdown_timeout")]
    pub shutdown_timeout_secs: u64,
    /// Maximum WebSocket message size in bytes. Default: 1MB (1048576).
    #[serde(default = "default_ws_max_message_size")]
    pub ws_max_message_size: usize,
    /// WebSocket heartbeat interval in seconds. Clients should ping at this
    /// rate; connections idle for 2× this value are closed. Default: 30.
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval_secs: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            log_level: default_log_level(),
            max_messages_per_sec: default_max_messages_per_sec(),
            api_rate_limit: default_api_rate_limit(),
            shutdown_timeout_secs: default_shutdown_timeout(),
            ws_max_message_size: default_ws_max_message_size(),
            heartbeat_interval_secs: default_heartbeat_interval(),
        }
    }
}

fn default_ws_max_message_size() -> usize {
    1_048_576 // 1MB
}

fn default_heartbeat_interval() -> u64 {
    30
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct StoreConfig {
    /// "embedded" (default) or "remote"
    #[serde(default = "default_store_mode")]
    pub mode: String,
    /// Path for embedded mode. Ignored when mode = "remote".
    #[serde(default = "default_store_path")]
    pub path: PathBuf,
    /// ShroudB server URI for remote mode (e.g. "tcp://shroudb.internal:5200").
    /// Required when mode = "remote".
    #[serde(default)]
    pub addr: Option<String>,
    #[serde(default = "default_ttl_days")]
    pub event_ttl_days: u32,
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            mode: default_store_mode(),
            path: default_store_path(),
            addr: None,
            event_ttl_days: default_ttl_days(),
        }
    }
}

fn default_store_mode() -> String {
    "embedded".to_string()
}

#[derive(Debug, Deserialize)]
pub struct AuthConfig {
    /// Server admin password. Required. Used for all `/admin/*` endpoints.
    pub password: Option<String>,
    /// Maximum age (seconds) of a signed connection token. Default 300 (5 min).
    #[serde(default = "default_token_window")]
    pub token_window_secs: u64,
}

fn default_token_window() -> u64 {
    300
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct PresenceConfig {
    #[serde(default = "default_linger_secs")]
    pub linger_secs: u64,
    #[serde(default = "default_manual_override_ttl")]
    pub manual_override_ttl_secs: u64,
}

impl Default for PresenceConfig {
    fn default() -> Self {
        Self {
            linger_secs: default_linger_secs(),
            manual_override_ttl_secs: default_manual_override_ttl(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct WebhookConfig {
    pub url: String,
    pub secret: String,
    #[serde(default = "default_webhook_retries")]
    pub retries: u32,
    /// Event types to deliver. Empty/None = all events.
    #[serde(default)]
    pub events: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct TlsConfig {
    pub cert_path: String,
    pub key_path: String,
}

#[derive(Debug, Default, Deserialize)]
pub struct ClusterConfig {
    /// Enable cross-instance fanout via ShroudB store subscriptions.
    /// Requires `store.mode = "remote"`.
    #[serde(default)]
    pub enabled: bool,
    /// Unique identifier for this instance. Auto-generated UUID if omitted.
    #[serde(default)]
    pub instance_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct CorsConfig {
    /// Allowed origins. Use ["*"] for any origin (development only).
    pub allowed_origins: Vec<String>,
}

fn default_bind() -> String {
    "0.0.0.0:6200".to_string()
}
fn default_log_level() -> String {
    "info".to_string()
}
fn default_store_path() -> PathBuf {
    PathBuf::from("./herald-data/herald.db")
}
fn default_ttl_days() -> u32 {
    7
}
fn default_linger_secs() -> u64 {
    10
}
fn default_manual_override_ttl() -> u64 {
    14400
}
fn default_webhook_retries() -> u32 {
    3
}
fn default_max_messages_per_sec() -> u32 {
    10
}
fn default_api_rate_limit() -> u32 {
    100
}
fn default_shutdown_timeout() -> u64 {
    30
}

impl HeraldConfig {
    /// Validate configuration values at startup. Returns descriptive errors.
    pub fn validate(&self) -> anyhow::Result<()> {
        // Server config
        if self.server.max_messages_per_sec == 0 {
            anyhow::bail!("server.max_messages_per_sec must be > 0");
        }
        if self.server.api_rate_limit == 0 {
            anyhow::bail!("server.api_rate_limit must be > 0");
        }
        if self.server.shutdown_timeout_secs == 0 {
            anyhow::bail!("server.shutdown_timeout_secs must be > 0");
        }

        // Store config
        match self.store.mode.as_str() {
            "embedded" => {}
            "remote" => {
                if self.store.addr.as_deref().unwrap_or("").is_empty() {
                    anyhow::bail!("store.addr is required when store.mode = \"remote\"");
                }
            }
            other => {
                anyhow::bail!("store.mode must be \"embedded\" or \"remote\", got \"{other}\"")
            }
        }
        if self.store.event_ttl_days == 0 {
            anyhow::bail!("store.event_ttl_days must be > 0");
        }

        // Auth config
        let password = self.auth.password.as_deref().unwrap_or("");
        if password.is_empty() {
            anyhow::bail!("auth.password is required");
        }
        if password.len() < 16 {
            anyhow::bail!("auth.password should be at least 16 characters");
        }
        if self.auth.token_window_secs == 0 {
            anyhow::bail!("auth.token_window_secs must be > 0");
        }

        // TLS config
        if let Some(ref tls) = self.tls {
            if tls.cert_path.is_empty() {
                anyhow::bail!("tls.cert_path must not be empty when TLS is configured");
            }
            if tls.key_path.is_empty() {
                anyhow::bail!("tls.key_path must not be empty when TLS is configured");
            }
            if !std::path::Path::new(&tls.cert_path).exists() {
                anyhow::bail!("tls.cert_path '{}' does not exist", tls.cert_path);
            }
            if !std::path::Path::new(&tls.key_path).exists() {
                anyhow::bail!("tls.key_path '{}' does not exist", tls.key_path);
            }
        }

        // Cluster config
        if self.cluster.enabled && self.store.mode != "remote" {
            anyhow::bail!(
                "cluster.enabled requires store.mode = \"remote\" (shared storage for cross-instance coordination)"
            );
        }

        // Webhook config
        if let Some(ref webhook) = self.webhook {
            if webhook.url.is_empty() {
                anyhow::bail!("webhook.url must not be empty when webhook is configured");
            }
            if webhook.secret.is_empty() {
                anyhow::bail!("webhook.secret must not be empty when webhook is configured — unsigned webhooks are insecure");
            }
        }

        Ok(())
    }

    pub fn load(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("failed to read config {path}: {e}"))?;
        let config: Self =
            toml::from_str(&content).map_err(|e| anyhow::anyhow!("invalid config: {e}"))?;
        Ok(config)
    }

    /// Build config entirely from environment variables.
    /// Prefix: HERALD_ for server settings, AUTH_ for auth, etc.
    /// This is the Railway-native deployment path — no config file needed.
    pub fn from_env() -> anyhow::Result<Self> {
        let env = |key: &str| std::env::var(key).ok();
        let env_or =
            |key: &str, default: &str| std::env::var(key).unwrap_or_else(|_| default.to_string());

        let port = env_or("PORT", "6200");
        let bind = format!("0.0.0.0:{port}");

        Ok(Self {
            server: ServerConfig {
                bind: env("HERALD_BIND").unwrap_or(bind),
                log_level: env_or("HERALD_LOG_LEVEL", "info"),
                max_messages_per_sec: env("HERALD_MAX_MSG_PER_SEC")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(10),
                api_rate_limit: env("HERALD_API_RATE_LIMIT")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(100),
                shutdown_timeout_secs: env("HERALD_SHUTDOWN_TIMEOUT")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(30),
                ws_max_message_size: env("HERALD_WS_MAX_MESSAGE_SIZE")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1_048_576),
                heartbeat_interval_secs: env("HERALD_HEARTBEAT_INTERVAL")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(30),
            },
            store: StoreConfig {
                mode: env_or("HERALD_STORE_MODE", "embedded"),
                path: env_or("HERALD_DATA_DIR", "./herald-data").into(),
                addr: env("HERALD_STORE_ADDR"),
                event_ttl_days: env("HERALD_EVENT_TTL_DAYS")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(7),
            },
            auth: AuthConfig {
                password: env("HERALD_PASSWORD"),
                token_window_secs: env("HERALD_TOKEN_WINDOW_SECS")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(300),
            },
            presence: PresenceConfig {
                linger_secs: env("HERALD_LINGER_SECS")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(10),
                manual_override_ttl_secs: env("HERALD_PRESENCE_TTL")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(14400),
            },
            webhook: env("HERALD_WEBHOOK_URL").map(|url| WebhookConfig {
                url,
                secret: env_or("HERALD_WEBHOOK_SECRET", ""),
                retries: env("HERALD_WEBHOOK_RETRIES")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(3),
                events: env("HERALD_WEBHOOK_EVENTS")
                    .map(|s| s.split(',').map(|e| e.trim().to_string()).collect()),
            }),
            shroudb: if env("HERALD_SENTRY_ADDR").is_some() {
                Some(ShroudbConfig {
                    sentry_addr: env("HERALD_SENTRY_ADDR"),
                    sentry_token: env("HERALD_SENTRY_TOKEN"),
                    courier_addr: env("HERALD_COURIER_ADDR"),
                    courier_token: env("HERALD_COURIER_TOKEN"),
                    chronicle_addr: env("HERALD_CHRONICLE_ADDR"),
                    chronicle_token: env("HERALD_CHRONICLE_TOKEN"),
                    auth_token: env("HERALD_MOAT_TOKEN"),
                })
            } else {
                None
            },
            tls: env("HERALD_TLS_CERT").map(|cert| TlsConfig {
                cert_path: cert,
                key_path: env_or("HERALD_TLS_KEY", ""),
            }),
            tenant_limits: TenantLimitsConfig {
                max_connections_per_tenant: env("HERALD_MAX_CONNS_PER_TENANT")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(10000),
                max_streams_per_tenant: env("HERALD_MAX_STREAMS_PER_TENANT")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(10000),
            },
            cors: env("HERALD_CORS_ORIGINS").map(|origins| CorsConfig {
                allowed_origins: origins.split(',').map(|s| s.trim().to_string()).collect(),
            }),
            cluster: ClusterConfig {
                enabled: env("HERALD_CLUSTER_ENABLED")
                    .map(|v| v == "true" || v == "1")
                    .unwrap_or(false),
                instance_id: env("HERALD_CLUSTER_INSTANCE_ID"),
            },
        })
    }

    /// Load from file if it exists, otherwise from env vars.
    pub fn load_or_env(path: &str) -> anyhow::Result<Self> {
        if std::path::Path::new(path).exists() {
            Self::load(path)
        } else {
            tracing::info!("no config file at {path}, loading from environment variables");
            Self::from_env()
        }
    }

    /// Pull secrets from ShroudB Keep and merge into config.
    /// Called after initial config load if HERALD_KEEP_ADDR is set.
    pub async fn load_secrets_from_keep(&mut self) -> anyhow::Result<()> {
        let addr = match std::env::var("HERALD_KEEP_ADDR") {
            Ok(a) if !a.is_empty() => a,
            _ => return Ok(()), // No Keep configured — skip
        };
        let token = std::env::var("HERALD_KEEP_TOKEN").ok();

        tracing::info!(addr = %addr, "loading secrets from Keep");

        let mut client = shroudb_keep_client::KeepClient::connect(&addr)
            .await
            .map_err(|e| anyhow::anyhow!("keep connect failed: {e}"))?;

        if let Some(ref t) = token {
            client
                .auth(t)
                .await
                .map_err(|e| anyhow::anyhow!("keep auth failed: {e}"))?;
        }

        // Pull secrets — Keep stores values as base64, decode them.
        // Master key is hex (not base64) — handled separately below.
        let decode_keep = |val: &str| -> Result<String, anyhow::Error> {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(val)
                .map_err(|e| anyhow::anyhow!("base64 decode: {e}"))?;
            Ok(String::from_utf8(bytes)?)
        };

        match client.get("herald/password", None).await {
            Ok(result) => match decode_keep(&result.value) {
                Ok(password) => {
                    self.auth.password = Some(password);
                    tracing::info!("loaded herald/password from Keep");
                }
                Err(e) => tracing::warn!("herald/password decode failed: {e}"),
            },
            Err(e) => tracing::warn!("herald/password not in Keep: {e}"),
        }
        match client.get("herald/webhook-secret", None).await {
            Ok(result) => match decode_keep(&result.value) {
                Ok(secret) => {
                    if let Some(ref mut wh) = self.webhook {
                        wh.secret = secret;
                        tracing::info!("loaded herald/webhook-secret from Keep");
                    }
                }
                Err(e) => tracing::warn!("herald/webhook-secret decode failed: {e}"),
            },
            Err(e) => tracing::debug!("herald/webhook-secret not in Keep: {e}"),
        }

        // Master key for Herald's own storage engine.
        // Set as env var so ChainedMasterKeySource picks it up when storage opens.
        if std::env::var("SHROUDB_MASTER_KEY").is_err() {
            match client.get("herald/master-key", None).await {
                Ok(result) => {
                    std::env::set_var("SHROUDB_MASTER_KEY", &result.value);
                    tracing::info!("loaded master-key from Keep");
                }
                Err(e) => {
                    tracing::error!(
                        "failed to load herald/master-key from Keep: {e}. \
                         Either store the key in Keep at 'herald/master-key' or \
                         set SHROUDB_MASTER_KEY env var directly."
                    );
                    return Err(anyhow::anyhow!(
                        "master key not available: not in env and Keep returned: {e}"
                    ));
                }
            }
        }

        Ok(())
    }
}
