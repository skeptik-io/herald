use std::sync::Arc;

use axum::extract::{Extension, Path, Query, State};
use axum::response::IntoResponse;
use axum::Json;

use crate::http::TenantId;
use crate::state::AppState;

#[utoipa::path(
    get, path = "/presence/{user_id}",
    tag = "presence",
    params(("user_id" = String, Path, description = "User ID")),
    security(("basic_auth" = []), ("bearer_auth" = [])),
    responses(
        (status = 200, description = "User presence status", body = crate::http::openapi::UserPresenceResponse),
    ),
)]
pub async fn user_presence(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path(user_id): Path<String>,
) -> impl IntoResponse {
    let tid = &tenant.0;
    let resolved = state.presence.resolve(
        tid,
        &user_id,
        &state.connections,
        state.config.presence.manual_override_ttl_secs,
    );
    let connections = state.connections.user_connection_count(tid, &user_id);

    Json(serde_json::json!({
        "user_id": user_id,
        "status": resolved.status,
        "connections": connections,
        "last_seen_at": resolved.last_seen_at,
    }))
}

#[utoipa::path(
    get, path = "/streams/{id}/presence",
    tag = "presence",
    params(("id" = String, Path, description = "Stream ID")),
    security(("basic_auth" = []), ("bearer_auth" = [])),
    responses(
        (status = 200, description = "Presence for stream members", body = crate::http::openapi::StreamPresenceResponse),
    ),
)]
pub async fn stream_presence(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path(stream_id): Path<String>,
) -> impl IntoResponse {
    let tid = &tenant.0;
    let members = state.streams.get_members(tid, &stream_id);
    let presence: Vec<serde_json::Value> = members
        .iter()
        .map(|uid| {
            let resolved = state.presence.resolve(
                tid,
                uid,
                &state.connections,
                state.config.presence.manual_override_ttl_secs,
            );
            serde_json::json!({
                "user_id": uid,
                "status": resolved.status,
                "last_seen_at": resolved.last_seen_at,
            })
        })
        .collect();

    Json(serde_json::json!({"members": presence}))
}

/// Query parameters for batch presence lookup.
#[derive(serde::Deserialize)]
pub struct BatchPresenceQuery {
    /// Comma-separated user IDs.
    pub user_ids: String,
}

#[utoipa::path(
    get, path = "/presence",
    tag = "presence",
    params(("user_ids" = String, Query, description = "Comma-separated user IDs")),
    security(("basic_auth" = []), ("bearer_auth" = [])),
    responses(
        (status = 200, description = "Batch presence query", body = crate::http::openapi::BatchPresenceResponse),
    ),
)]
pub async fn batch_presence(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Query(query): Query<BatchPresenceQuery>,
) -> impl IntoResponse {
    let tid = &tenant.0;
    let users: Vec<serde_json::Value> = query
        .user_ids
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|uid| {
            let uid = uid.trim();
            let resolved = state.presence.resolve(
                tid,
                uid,
                &state.connections,
                state.config.presence.manual_override_ttl_secs,
            );
            let connections = state.connections.user_connection_count(tid, uid);
            serde_json::json!({
                "user_id": uid,
                "status": resolved.status,
                "connections": connections,
                "last_seen_at": resolved.last_seen_at,
            })
        })
        .collect();

    Json(serde_json::json!({"users": users}))
}

/// Request body for admin presence override.
#[derive(serde::Deserialize, utoipa::ToSchema)]
pub struct AdminSetPresenceRequest {
    pub status: herald_core::presence::PresenceStatus,
}

#[utoipa::path(
    post, path = "/presence/{user_id}",
    tag = "presence",
    params(("user_id" = String, Path, description = "User ID")),
    request_body = AdminSetPresenceRequest,
    security(("basic_auth" = []), ("bearer_auth" = [])),
    responses(
        (status = 200, description = "Presence override applied", body = crate::http::openapi::UserPresenceResponse),
    ),
)]
pub async fn admin_set_presence(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path(user_id): Path<String>,
    Json(body): Json<AdminSetPresenceRequest>,
) -> impl IntoResponse {
    let tid = &tenant.0;
    state
        .presence
        .set_manual(tid, &user_id, body.status, None, None);

    // Broadcast the change to all streams the user is a member of
    crate::ws::connection::broadcast_presence_change(&state, tid, &user_id).await;

    // Notify watchlist watchers
    let watchers = state.connections.get_watchers(tid, &user_id);
    if !watchers.is_empty() {
        use herald_core::protocol::{PresenceChangedPayload, ServerMessage};
        let msg = ServerMessage::PresenceChanged {
            payload: PresenceChangedPayload {
                user_id: user_id.clone(),
                presence: body.status,
                until: None,
                last_seen_at: None,
            },
        };
        for watcher_id in &watchers {
            state.connections.send_to_user(tid, watcher_id, &msg);
        }
    }

    let resolved = state.presence.resolve(
        tid,
        &user_id,
        &state.connections,
        state.config.presence.manual_override_ttl_secs,
    );
    let connections = state.connections.user_connection_count(tid, &user_id);

    Json(serde_json::json!({
        "user_id": user_id,
        "status": resolved.status,
        "connections": connections,
        "last_seen_at": resolved.last_seen_at,
    }))
}
