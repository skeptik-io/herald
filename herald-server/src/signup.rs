//! Self-serve tenant provisioning via ShroudB Sigil (Moat HTTP API).
//!
//! Herald connects to a Moat instance's HTTP API (`/v1/sigil`) for credential
//! management. Public endpoints allow signup, login, and token refresh.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::state::AppState;
use crate::store;
use crate::ws::connection::now_millis;

/// Schema name used for Herald tenant owner credentials.
const SIGIL_SCHEMA: &str = "herald";

// ── Sigil HTTP client ───────────────────────────────────────────────

/// Minimal Sigil client that talks to Moat's HTTP API.
pub struct SigilHttpClient {
    http: reqwest::Client,
    base_url: String,
    auth_token: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SigilRequest {
    args: Vec<String>,
}

impl SigilHttpClient {
    pub fn new(moat_url: &str, auth_token: Option<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: moat_url.trim_end_matches('/').to_string(),
            auth_token,
        }
    }

    async fn command(&self, args: &[&str]) -> Result<serde_json::Value, String> {
        let url = format!("{}/v1/sigil", self.base_url);
        let body = SigilRequest {
            args: args.iter().map(|s| s.to_string()).collect(),
        };

        let mut req = self.http.post(&url).json(&body);
        if let Some(ref token) = self.auth_token {
            req = req.bearer_auth(token);
        }

        let resp = req
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| format!("sigil request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("sigil returned {status}: {body}"));
        }

        resp.json::<serde_json::Value>()
            .await
            .map_err(|e| format!("sigil response parse failed: {e}"))
    }

    pub async fn health(&self) -> Result<(), String> {
        self.command(&["HEALTH"]).await?;
        Ok(())
    }

    pub async fn schema_register(
        &self,
        name: &str,
        definition: &serde_json::Value,
    ) -> Result<(), String> {
        let json =
            serde_json::to_string(definition).map_err(|e| format!("schema serialize: {e}"))?;
        self.command(&["SCHEMA", "REGISTER", name, &json]).await?;
        Ok(())
    }

    pub async fn user_create(
        &self,
        schema: &str,
        user_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let json = serde_json::to_string(fields).map_err(|e| format!("fields serialize: {e}"))?;
        self.command(&["USER", "CREATE", schema, user_id, &json])
            .await
    }

    pub async fn user_get(&self, schema: &str, user_id: &str) -> Result<serde_json::Value, String> {
        self.command(&["USER", "GET", schema, user_id]).await
    }

    pub async fn user_lookup(
        &self,
        schema: &str,
        field: &str,
        value: &str,
    ) -> Result<String, String> {
        let resp = self
            .command(&["USER", "LOOKUP", schema, field, value])
            .await?;
        resp["entity_id"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| "missing entity_id in lookup response".to_string())
    }

    pub async fn session_create(
        &self,
        schema: &str,
        user_id: &str,
        password: &str,
        metadata: Option<&serde_json::Value>,
    ) -> Result<TokenPair, String> {
        let mut args = vec!["SESSION", "CREATE", schema, user_id, password];
        let meta_str;
        if let Some(meta) = metadata {
            meta_str = serde_json::to_string(meta).map_err(|e| format!("meta serialize: {e}"))?;
            args.push("META");
            args.push(&meta_str);
        }
        let resp = self.command(&args).await?;
        Ok(TokenPair {
            access_token: resp["access_token"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            refresh_token: resp["refresh_token"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            expires_in: resp["expires_in"].as_u64().unwrap_or(900),
        })
    }

    pub async fn session_create_by_field(
        &self,
        schema: &str,
        field: &str,
        value: &str,
        password: &str,
    ) -> Result<TokenPair, String> {
        let resp = self
            .command(&["SESSION", "LOGIN", schema, field, value, password])
            .await?;
        Ok(TokenPair {
            access_token: resp["access_token"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            refresh_token: resp["refresh_token"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            expires_in: resp["expires_in"].as_u64().unwrap_or(900),
        })
    }

    pub async fn session_refresh(
        &self,
        schema: &str,
        refresh_token: &str,
    ) -> Result<TokenPair, String> {
        let resp = self
            .command(&["SESSION", "REFRESH", schema, refresh_token])
            .await?;
        Ok(TokenPair {
            access_token: resp["access_token"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            refresh_token: resp["refresh_token"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            expires_in: resp["expires_in"].as_u64().unwrap_or(900),
        })
    }
}

pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
}

// ── Request/Response types ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct SignupRequest {
    pub email: String,
    pub password: String,
    pub org_name: String,
}

#[derive(Serialize)]
pub struct SignupResponse {
    pub tenant_id: String,
    pub user_id: String,
    pub api_token: String,
    pub access_token: String,
    pub refresh_token: String,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub tenant_id: String,
}

#[derive(Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

#[derive(Serialize)]
pub struct RefreshResponse {
    pub access_token: String,
    pub refresh_token: String,
}

// ── Handlers ────────────────────────────────────────────────────────

/// POST /signup — create a new tenant with credentials.
pub async fn signup(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SignupRequest>,
) -> impl IntoResponse {
    if req.email.is_empty() || !req.email.contains('@') {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid email"})),
        )
            .into_response();
    }
    if req.password.len() < 8 {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "password must be at least 8 characters"})),
        )
            .into_response();
    }
    if req.org_name.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "org_name is required"})),
        )
            .into_response();
    }

    let sigil = match state.sigil.as_ref() {
        Some(s) => s,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "signup not available (no Moat configured)"})),
            )
                .into_response();
        }
    };

    let user_id = uuid::Uuid::new_v4().to_string();
    let tenant_id = uuid::Uuid::new_v4().to_string();

    // 1. Check if email is already registered (Veil blind index lookup)
    match sigil.user_lookup(SIGIL_SCHEMA, "email", &req.email).await {
        Ok(_existing_id) => {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({"error": "email already registered"})),
            )
                .into_response();
        }
        Err(e) if e.contains("not found") => {} // good — email is available
        Err(e) => {
            tracing::error!(error = %e, "signup: email lookup failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "signup failed"})),
            )
                .into_response();
        }
    }

    // 2. Create Sigil user (Argon2id-hashed password, encrypted email via Cipher, blind index via Veil)
    let fields = serde_json::json!({
        "email": req.email,
        "password": req.password,
        "tenant_id": tenant_id,
    });

    if let Err(e) = sigil.user_create(SIGIL_SCHEMA, &user_id, &fields).await {
        if e.contains("already exists") {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({"error": "email already registered"})),
            )
                .into_response();
        }
        tracing::error!(error = %e, "signup: sigil user_create failed");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "signup failed"})),
        )
            .into_response();
    }

    // 2. Create Herald tenant
    let jwt_secret = hex::encode(uuid::Uuid::new_v4().as_bytes());
    let tenant = store::tenants::Tenant {
        id: tenant_id.clone(),
        name: req.org_name,
        jwt_secret: jwt_secret.clone(),
        jwt_issuer: None,
        plan: "free".to_string(),
        config: serde_json::json!({"owner_user_id": user_id}),
        created_at: now_millis(),
    };

    if let Err(e) = store::tenants::insert(&*state.db, &tenant).await {
        tracing::error!(error = %e, "signup: failed to create tenant");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "signup failed"})),
        )
            .into_response();
    }

    state.tenant_cache.insert(
        tenant_id.clone(),
        crate::state::TenantConfig {
            id: tenant_id.clone(),
            jwt_secret: jwt_secret.clone(),
            jwt_issuer: None,
            plan: "free".to_string(),
            cached_at: std::time::Instant::now(),
        },
    );

    // 3. Generate API token
    let api_token = format!(
        "hld_{}",
        hex::encode(&uuid::Uuid::new_v4().as_bytes()[..12])
    );
    if let Err(e) = store::tenants::create_token(&*state.db, &api_token, &tenant_id, None).await {
        tracing::error!(error = %e, "signup: failed to create API token");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "signup failed"})),
        )
            .into_response();
    }

    // 4. Issue session tokens via Sigil
    let extra_claims = serde_json::json!({
        "tenant": tenant_id,
        "role": "owner",
    });
    match sigil
        .session_create(SIGIL_SCHEMA, &user_id, &req.password, Some(&extra_claims))
        .await
    {
        Ok(tokens) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "tenant_id": tenant_id,
                "user_id": user_id,
                "api_token": api_token,
                "access_token": tokens.access_token,
                "refresh_token": tokens.refresh_token,
            })),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(error = %e, "signup: session creation failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "signup failed"})),
            )
                .into_response()
        }
    }
}

/// POST /login — authenticate and issue session tokens.
pub async fn login(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LoginRequest>,
) -> impl IntoResponse {
    let sigil = match state.sigil.as_ref() {
        Some(s) => s,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "login not available"})),
            )
                .into_response();
        }
    };

    // Login by email field — Sigil resolves via Veil blind index, verifies with Argon2id
    match sigil
        .session_create_by_field(SIGIL_SCHEMA, "email", &req.email, &req.password)
        .await
    {
        Ok(tokens) => {
            // Extract tenant_id from the access token claims, or look up user
            // For now, look up the user to get tenant_id
            let user_id = match sigil.user_lookup(SIGIL_SCHEMA, "email", &req.email).await {
                Ok(id) => id,
                Err(_) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({"error": "login failed"})),
                    )
                        .into_response();
                }
            };

            let tenant_id = match sigil.user_get(SIGIL_SCHEMA, &user_id).await {
                Ok(record) => record["fields"]["tenant_id"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
                Err(_) => String::new(),
            };

            Json(serde_json::json!({
                "access_token": tokens.access_token,
                "refresh_token": tokens.refresh_token,
                "tenant_id": tenant_id,
            }))
            .into_response()
        }
        Err(e) => {
            if e.contains("verification failed")
                || e.contains("invalid")
                || e.contains("locked")
                || e.contains("not found")
            {
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({"error": "invalid credentials"})),
                )
                    .into_response();
            }
            tracing::error!(error = %e, "login failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "login failed"})),
            )
                .into_response()
        }
    }
}

/// POST /auth/refresh — rotate refresh token and issue new access token.
pub async fn refresh(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RefreshRequest>,
) -> impl IntoResponse {
    let sigil = match state.sigil.as_ref() {
        Some(s) => s,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "auth not available"})),
            )
                .into_response();
        }
    };

    match sigil
        .session_refresh(SIGIL_SCHEMA, &req.refresh_token)
        .await
    {
        Ok(tokens) => Json(serde_json::json!({
            "access_token": tokens.access_token,
            "refresh_token": tokens.refresh_token,
        }))
        .into_response(),
        Err(e) => {
            if e.contains("invalid") || e.contains("expired") || e.contains("revoked") {
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({"error": "invalid refresh token"})),
                )
                    .into_response();
            }
            tracing::error!(error = %e, "refresh failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "refresh failed"})),
            )
                .into_response()
        }
    }
}
