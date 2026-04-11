pub mod admin;
pub mod events;
pub mod health;
pub mod members;
pub mod openapi;
pub mod presence;
pub mod self_service;
pub mod streams;
pub mod validation;

use std::sync::Arc;

use crate::state::{AppState, RateLimitEntry};
use base64::Engine;
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
        .route("/streams/{id}/events", delete(events::purge_user_events))
        .route("/streams/{id}/trigger", post(events::trigger_ephemeral))
        .route("/stats", get(health::tenant_stats));

    // Presence routes: served by the presence engine when enabled,
    // otherwise fall back to the basic http::presence handlers.
    #[cfg(not(feature = "presence"))]
    {
        tenant_api = tenant_api
            .route("/streams/{id}/presence", get(presence::stream_presence))
            .route("/presence/{user_id}", get(presence::user_presence));
    }

    // Merge routes from registered engines (chat, presence, etc.)
    for engine_routes in state.engines.tenant_routes(state.clone()) {
        tenant_api = tenant_api.merge(engine_routes);
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
        .route("/admin/tenants/{id}/data", delete(admin::purge_tenant_data))
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
        .route("/admin/tenants/{id}/audit", get(admin::query_audit))
        .route("/admin/tenants/{id}/audit/count", get(admin::count_audit))
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

    let self_api = Router::new()
        .route("/self/connections", get(self_service::list_connections))
        .route("/self/events", get(self_service::list_events))
        .route("/self/events/stream", get(self_service::events_stream))
        .route("/self/errors", get(self_service::list_errors))
        .route("/self/secret/rotate", post(self_service::rotate_secret))
        .route("/self/tokens", post(self_service::create_token))
        .route("/self/tokens", get(self_service::list_tokens))
        .route("/self/tokens/{token}", delete(self_service::delete_token))
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            tenant_auth_middleware,
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

    let app = Router::new()
        .merge(tenant_api)
        .merge(admin_api)
        .merge(self_api)
        .route("/health", get(health::health))
        .route("/health/live", get(health::liveness))
        .route("/health/ready", get(health::readiness))
        .route("/metrics", get(health::metrics));

    #[cfg(feature = "openapi")]
    let app = app.route("/openapi.json", get(serve_openapi));

    app
        // WebSocket upgrade on the same port — browsers connect to wss://domain/ws
        .route("/ws", get(crate::ws::upgrade::ws_handler))
        .layer(cors)
        .layer(middleware::from_fn(request_id_middleware))
        .with_state(state)
}

/// Serve the OpenAPI spec as JSON.
#[cfg(feature = "openapi")]
async fn serve_openapi() -> impl IntoResponse {
    use utoipa::OpenApi;
    let spec = openapi::ApiDoc::openapi().to_json().unwrap_or_default();
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        spec,
    )
}

/// Middleware that generates a UUID per request, injects it as an extension,
/// Assigns a request ID, adds it to the response as `X-Request-Id`,
/// propagates W3C `traceparent` header, and sets security headers.
async fn request_id_middleware(mut req: Request, next: Next) -> Response {
    let request_id = uuid::Uuid::new_v4().to_string();
    req.extensions_mut().insert(RequestId(request_id.clone()));

    // Propagate incoming traceparent if present
    let traceparent = req
        .headers()
        .get("traceparent")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let mut response = next.run(req).await;
    let headers = response.headers_mut();
    headers.insert(
        "x-request-id",
        axum::http::HeaderValue::from_str(&request_id)
            .unwrap_or_else(|_| axum::http::HeaderValue::from_static("unknown")),
    );
    // Return traceparent: either propagate the incoming one or generate a new one
    if let Some(tp) = traceparent {
        if let Ok(val) = tp.parse() {
            headers.insert("traceparent", val);
        }
    } else {
        // Generate a W3C traceparent: version-trace_id-span_id-flags
        let trace_id = request_id.replace('-', "");
        let span_id = &trace_id[..16];
        let tp = format!("00-{trace_id}-{span_id}-01");
        if let Ok(val) = tp.parse() {
            headers.insert("traceparent", val);
        }
    }
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

/// Tenant API auth: accepts Basic (key:secret) or Bearer (API token).
async fn tenant_auth_middleware(
    State(state): State<Arc<AppState>>,
    mut req: Request,
    next: Next,
) -> Response {
    let auth_header = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok());

    let auth_header = match auth_header {
        Some(h) => h,
        None => {
            return (StatusCode::UNAUTHORIZED, "missing authorization header").into_response();
        }
    };

    // Try Basic auth (key:secret)
    if let Some(encoded) = auth_header.strip_prefix("Basic ") {
        let decoded = match base64::engine::general_purpose::STANDARD.decode(encoded.trim()) {
            Ok(d) => d,
            Err(_) => {
                return (StatusCode::UNAUTHORIZED, "invalid credentials").into_response();
            }
        };
        let decoded_str = match String::from_utf8(decoded) {
            Ok(s) => s,
            Err(_) => {
                return (StatusCode::UNAUTHORIZED, "invalid credentials").into_response();
            }
        };
        let (key, provided_secret) = match decoded_str.split_once(':') {
            Some((k, s)) => (k.to_string(), s.to_string()),
            None => {
                return (StatusCode::UNAUTHORIZED, "invalid credentials").into_response();
            }
        };

        // Look up key → tenant_id
        let tenant_id = match state.key_cache.get(&key) {
            Some(tid) => tid.clone(),
            None => {
                return (StatusCode::UNAUTHORIZED, "invalid credentials").into_response();
            }
        };

        // Verify secret (constant-time)
        let expected_secret = match state.tenant_cache.get(&tenant_id) {
            Some(tc) => tc.secret.clone(),
            None => {
                return (StatusCode::UNAUTHORIZED, "invalid credentials").into_response();
            }
        };
        if provided_secret
            .as_bytes()
            .ct_eq(expected_secret.as_bytes())
            .unwrap_u8()
            != 1
        {
            return (StatusCode::UNAUTHORIZED, "invalid credentials").into_response();
        }

        req.extensions_mut().insert(TenantId(tenant_id));
        req.extensions_mut().insert(TokenScope(None));
        return next.run(req).await;
    }

    // Try Bearer auth (API token)
    if let Some(token) = auth_header.strip_prefix("Bearer ") {
        let (tenant_id, scope) =
            match crate::store::tenants::validate_token(&*state.db, token).await {
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
        return next.run(req).await;
    }

    (StatusCode::UNAUTHORIZED, "invalid authorization header").into_response()
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

    let limit = state.config.server.api_rate_limit;
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

/// Admin API auth: validates against server password in config.
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

    let password = state.config.auth.password.as_deref().unwrap_or("");
    if password.is_empty() || token.as_bytes().ct_eq(password.as_bytes()).unwrap_u8() != 1 {
        return (StatusCode::UNAUTHORIZED, "invalid credentials").into_response();
    }

    next.run(req).await
}
