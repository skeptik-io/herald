pub mod admin;
#[cfg(not(feature = "chat"))]
pub mod blocks;
pub mod events;
pub mod health;
pub mod members;
#[cfg(not(feature = "chat"))]
pub mod presence;
pub mod streams;
pub mod validation;

use std::sync::Arc;

use crate::state::{AppState, RateLimitEntry};
use subtle::ConstantTimeEq;

use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::Router;
use tower_http::cors::{Any, CorsLayer};

/// Tenant ID extracted from API token lookup.
#[derive(Clone)]
pub struct TenantId(pub String);

/// Token scope extracted from API token lookup.
#[derive(Clone)]
pub struct TokenScope(pub Option<String>);

/// Request ID extracted from middleware.
#[derive(Clone)]
pub struct RequestId(pub String);

pub fn router(state: Arc<AppState>) -> Router {
    let mut tenant_api = Router::new()
        .route("/streams", post(streams::create_stream))
        .route("/streams", get(streams::list_streams))
        .route("/streams/{id}", get(streams::get_stream))
        .route("/streams/{id}", patch(streams::update_stream))
        .route("/streams/{id}", delete(streams::delete_stream))
        .route("/streams/{id}/members", post(members::add_member))
        .route("/streams/{id}/members", get(members::list_members))
        .route(
            "/streams/{id}/members/{user_id}",
            delete(members::remove_member),
        )
        .route(
            "/streams/{id}/members/{user_id}",
            patch(members::update_member),
        )
        .route("/streams/{id}/events", post(events::inject_event))
        .route("/streams/{id}/events", get(events::list_events))
        .route("/streams/{id}/trigger", post(events::trigger_ephemeral))
        .route("/stats", get(health::tenant_stats));

    // Chat-specific routes: event edit/delete, reactions, cursors, presence, blocks
    #[cfg(feature = "chat")]
    {
        tenant_api = tenant_api
            .route(
                "/streams/{id}/events/{event_id}",
                delete(crate::chat::http_events_ext::delete_event)
                    .patch(crate::chat::http_events_ext::edit_event),
            )
            .route(
                "/streams/{id}/events/{event_id}/reactions",
                get(crate::chat::http_events_ext::get_reactions),
            )
            .route(
                "/streams/{id}/cursors",
                get(crate::chat::http_events_ext::list_cursors),
            )
            .route(
                "/streams/{id}/presence",
                get(crate::chat::http_presence::stream_presence),
            )
            .route(
                "/presence/{user_id}",
                get(crate::chat::http_presence::user_presence),
            )
            .route("/blocks", post(crate::chat::http_blocks::block_user))
            .route("/blocks", delete(crate::chat::http_blocks::unblock_user))
            .route(
                "/blocks/{user_id}",
                get(crate::chat::http_blocks::list_blocked),
            );
    }

    #[cfg(not(feature = "chat"))]
    {
        tenant_api = tenant_api
            .route(
                "/streams/{id}/events/{event_id}",
                delete(events::delete_event).patch(events::edit_event),
            )
            .route(
                "/streams/{id}/events/{event_id}/reactions",
                get(events::get_reactions),
            )
            .route("/streams/{id}/cursors", get(events::list_cursors))
            .route("/streams/{id}/presence", get(presence::stream_presence))
            .route("/presence/{user_id}", get(presence::user_presence))
            .route("/blocks", post(blocks::block_user))
            .route("/blocks", delete(blocks::unblock_user))
            .route("/blocks/{user_id}", get(blocks::list_blocked));
    }

    let tenant_api = tenant_api
        .layer(DefaultBodyLimit::max(1024 * 1024)) // 1MB
        .layer(middleware::from_fn(scope_check_middleware))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            tenant_rate_limit_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            tenant_auth_middleware,
        ));

    let admin_api = Router::new()
        .route("/admin/tenants", post(admin::create_tenant))
        .route("/admin/tenants", get(admin::list_tenants))
        .route("/admin/tenants/{id}", get(admin::get_tenant))
        .route("/admin/tenants/{id}", patch(admin::update_tenant))
        .route("/admin/tenants/{id}", delete(admin::delete_tenant))
        .route("/admin/tenants/{id}/tokens", post(admin::create_api_token))
        .route("/admin/tenants/{id}/tokens", get(admin::list_api_tokens))
        .route(
            "/admin/tenants/{id}/tokens/{token}",
            delete(admin::delete_api_token),
        )
        .route(
            "/admin/tenants/{id}/streams",
            get(admin::list_tenant_streams),
        )
        .route("/admin/connections", get(admin::list_connections))
        .route("/admin/events", get(admin::list_events))
        .route("/admin/events/stream", get(admin::events_stream))
        .route("/admin/errors", get(admin::list_errors))
        .route("/admin/stats", get(admin::get_stats))
        .layer(DefaultBodyLimit::max(1024 * 1024)) // 1MB
        .layer(middleware::from_fn_with_state(
            state.clone(),
            admin_auth_middleware,
        ));

    let cors = if let Some(ref cors_config) = state.config.cors {
        if cors_config.allowed_origins.iter().any(|o| o == "*") {
            CorsLayer::permissive()
        } else {
            let origins: Vec<_> = cors_config
                .allowed_origins
                .iter()
                .filter_map(|o| o.parse().ok())
                .collect();
            CorsLayer::new()
                .allow_origin(origins)
                .allow_methods(Any)
                .allow_headers(Any)
        }
    } else {
        // Default: permissive (allow any origin) for ease of development
        CorsLayer::permissive()
    };

    Router::new()
        .merge(tenant_api)
        .merge(admin_api)
        .route("/health", get(health::health))
        .route("/health/live", get(health::liveness))
        .route("/health/ready", get(health::readiness))
        .route("/metrics", get(health::metrics))
        // WebSocket upgrade on the same port — browsers connect to wss://domain/ws
        .route("/ws", get(crate::ws::upgrade::ws_handler))
        .layer(cors)
        .layer(middleware::from_fn(request_id_middleware))
        .with_state(state)
}

/// Middleware that generates a UUID per request, injects it as an extension,
/// adds it to the response as `X-Request-Id`, and sets security headers.
async fn request_id_middleware(mut req: Request, next: Next) -> Response {
    let request_id = uuid::Uuid::new_v4().to_string();
    req.extensions_mut().insert(RequestId(request_id.clone()));
    let mut response = next.run(req).await;
    let headers = response.headers_mut();
    headers.insert(
        "x-request-id",
        axum::http::HeaderValue::from_str(&request_id)
            .unwrap_or_else(|_| axum::http::HeaderValue::from_static("unknown")),
    );
    headers.insert(
        "x-content-type-options",
        axum::http::HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        "x-frame-options",
        axum::http::HeaderValue::from_static("DENY"),
    );
    headers.insert(
        "referrer-policy",
        axum::http::HeaderValue::from_static("no-referrer"),
    );
    response
}

/// Tenant API auth: validates bearer token against api_tokens table, extracts tenant_id.
async fn tenant_auth_middleware(
    State(state): State<Arc<AppState>>,
    mut req: Request,
    next: Next,
) -> Response {
    let auth_header = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok());

    let token = match auth_header {
        Some(h) if h.starts_with("Bearer ") => &h[7..],
        _ => {
            return (StatusCode::UNAUTHORIZED, "missing authorization header").into_response();
        }
    };

    // Look up token in DB to find tenant
    let (tenant_id, scope) = match crate::store::tenants::validate_token(&*state.db, token).await {
        Ok(Some(api_token)) => (api_token.tenant_id, api_token.scope),
        Ok(None) => {
            return (StatusCode::UNAUTHORIZED, "invalid token").into_response();
        }
        Err(e) => {
            tracing::error!("token validation error: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response();
        }
    };

    req.extensions_mut().insert(TenantId(tenant_id));
    req.extensions_mut().insert(TokenScope(scope));
    next.run(req).await
}

/// Per-tenant HTTP API rate limiter (fixed-window, 60s).
async fn tenant_rate_limit_middleware(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    let tenant_id = match req.extensions().get::<TenantId>() {
        Some(t) => t.0.clone(),
        None => return next.run(req).await,
    };

    let limit = state
        .get_plan_limits_cached(&tenant_id)
        .map(|pl| pl.api_rate_limit)
        .unwrap_or(state.config.server.api_rate_limit);
    let entry = state
        .api_rate_limits
        .entry(tenant_id)
        .or_insert_with(|| {
            Arc::new(RateLimitEntry {
                count: std::sync::atomic::AtomicU32::new(0),
                window_start: std::sync::Mutex::new(std::time::Instant::now()),
            })
        })
        .clone();

    // Reset window if 60 seconds have passed
    {
        let mut start = entry.window_start.lock().unwrap_or_else(|e| e.into_inner());
        if start.elapsed().as_secs() >= 60 {
            *start = std::time::Instant::now();
            entry.count.store(0, std::sync::atomic::Ordering::Relaxed);
        }
    }

    let count = entry
        .count
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        + 1;
    if count > limit {
        return (StatusCode::TOO_MANY_REQUESTS, "rate limit exceeded").into_response();
    }

    next.run(req).await
}

fn check_scope(scope: &Option<String>, method: &axum::http::Method, path: &str) -> bool {
    let Some(ref s) = scope else {
        return true;
    }; // No scope = full access

    if s == "read-only" {
        // Only allow GET requests
        return *method == axum::http::Method::GET;
    }

    if let Some(stream_id) = s.strip_prefix("stream:") {
        // Only allow access to the specific stream
        return path.contains(&format!("/streams/{stream_id}"))
            || path.contains(&format!("/streams/{stream_id}/"));
    }

    true // Unknown scope = full access (don't break on forward-compatible scopes)
}

/// Scope enforcement middleware — checks token scope against request method and path.
async fn scope_check_middleware(req: Request, next: Next) -> Response {
    if let Some(scope) = req.extensions().get::<TokenScope>() {
        if !check_scope(&scope.0, req.method(), req.uri().path()) {
            return (
                StatusCode::FORBIDDEN,
                "token scope does not permit this action",
            )
                .into_response();
        }
    }
    next.run(req).await
}

/// Admin API auth: validates against super_admin_token in config.
async fn admin_auth_middleware(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    let auth_header = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok());

    let token = match auth_header {
        Some(h) if h.starts_with("Bearer ") => &h[7..],
        _ => {
            return (StatusCode::UNAUTHORIZED, "missing authorization header").into_response();
        }
    };

    let super_token = state.config.auth.super_admin_token.as_deref().unwrap_or("");
    if super_token.is_empty() || token.as_bytes().ct_eq(super_token.as_bytes()).unwrap_u8() != 1 {
        return (StatusCode::UNAUTHORIZED, "invalid admin token").into_response();
    }

    next.run(req).await
}
