pub mod admin;
pub mod health;
pub mod members;
pub mod messages;
pub mod presence;
pub mod rooms;

use std::sync::Arc;

use crate::state::AppState;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::Router;

/// Tenant ID extracted from API token lookup.
#[derive(Clone)]
pub struct TenantId(pub String);

pub fn router(state: Arc<AppState>) -> Router {
    let tenant_api = Router::new()
        .route("/rooms", post(rooms::create_room))
        .route("/rooms", get(rooms::list_rooms))
        .route("/rooms/{id}", get(rooms::get_room))
        .route("/rooms/{id}", patch(rooms::update_room))
        .route("/rooms/{id}", delete(rooms::delete_room))
        .route("/rooms/{id}/members", post(members::add_member))
        .route("/rooms/{id}/members", get(members::list_members))
        .route(
            "/rooms/{id}/members/{user_id}",
            delete(members::remove_member),
        )
        .route(
            "/rooms/{id}/members/{user_id}",
            patch(members::update_member),
        )
        .route("/rooms/{id}/messages", post(messages::inject_message))
        .route("/rooms/{id}/messages", get(messages::list_messages))
        .route(
            "/rooms/{id}/messages/search",
            get(messages::search_messages),
        )
        .route("/rooms/{id}/cursors", get(messages::list_cursors))
        .route("/rooms/{id}/presence", get(presence::room_presence))
        .route("/presence/{user_id}", get(presence::user_presence))
        .route("/stats", get(health::tenant_stats))
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
        .route("/admin/tenants/{id}/rooms", get(admin::list_tenant_rooms))
        .route("/admin/connections", get(admin::list_connections))
        .route("/admin/events", get(admin::list_events))
        .route("/admin/events/stream", get(admin::events_stream))
        .route("/admin/errors", get(admin::list_errors))
        .route("/admin/stats", get(admin::get_stats))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            admin_auth_middleware,
        ));

    Router::new()
        .merge(tenant_api)
        .merge(admin_api)
        .route("/health", get(health::health))
        .route("/metrics", get(health::metrics))
        // WebSocket upgrade on the same port — browsers connect to wss://domain/ws
        .route("/ws", get(crate::ws::upgrade::ws_handler))
        .with_state(state)
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
    let tenant_id = match crate::store::tenants::validate_token(&*state.db, token).await {
        Ok(Some(tid)) => tid,
        Ok(None) => {
            return (StatusCode::UNAUTHORIZED, "invalid token").into_response();
        }
        Err(e) => {
            tracing::error!("token validation error: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response();
        }
    };

    req.extensions_mut().insert(TenantId(tenant_id));
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
    if token != super_token || super_token.is_empty() {
        return (StatusCode::UNAUTHORIZED, "invalid admin token").into_response();
    }

    next.run(req).await
}
