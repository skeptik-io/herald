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
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct ShroudbConfig {
    pub cipher_addr: Option<String>,
    pub cipher_token: Option<String>,
    pub veil_addr: Option<String>,
    pub veil_token: Option<String>,
    pub sentry_addr: Option<String>,
    pub sentry_token: Option<String>,
    pub courier_addr: Option<String>,
    pub courier_token: Option<String>,
    pub chronicle_addr: Option<String>,
    pub chronicle_token: Option<String>,
    pub moat_addr: Option<String>,
    pub auth_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_ws_bind")]
    pub ws_bind: String,
    #[serde(default = "default_http_bind")]
    pub http_bind: String,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_max_messages_per_sec")]
    pub max_messages_per_sec: u32,
    #[serde(default = "default_api_rate_limit")]
    pub api_rate_limit: u32,
    #[serde(default = "default_shutdown_timeout")]
    pub shutdown_timeout_secs: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            ws_bind: default_ws_bind(),
            http_bind: default_http_bind(),
            log_level: default_log_level(),
            max_messages_per_sec: default_max_messages_per_sec(),
            api_rate_limit: default_api_rate_limit(),
            shutdown_timeout_secs: default_shutdown_timeout(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct StoreConfig {
    #[serde(default = "default_store_path")]
    pub path: PathBuf,
    #[serde(default = "default_ttl_days")]
    pub message_ttl_days: u32,
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            path: default_store_path(),
            message_ttl_days: default_ttl_days(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct AuthConfig {
    #[serde(default)]
    pub jwt_secret: Option<String>,
    #[serde(default)]
    pub jwt_issuer: Option<String>,
    #[serde(default)]
    pub super_admin_token: Option<String>,
    #[serde(default)]
    pub api: ApiAuthConfig,
}

#[derive(Debug, Default, Deserialize)]
pub struct ApiAuthConfig {
    #[serde(default)]
    pub tokens: Vec<String>,
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
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct TlsConfig {
    pub cert_path: String,
    pub key_path: String,
}

fn default_ws_bind() -> String {
    "0.0.0.0:6200".to_string()
}
fn default_http_bind() -> String {
    "0.0.0.0:6201".to_string()
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
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("failed to read config {path}: {e}"))?;
        let config: Self =
            toml::from_str(&content).map_err(|e| anyhow::anyhow!("invalid config: {e}"))?;
        Ok(config)
    }
}
