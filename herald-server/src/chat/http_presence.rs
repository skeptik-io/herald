use std::sync::Arc;

use axum::extract::{Extension, Path, State};
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
    let status = state.presence.resolve(
        tid,
        &user_id,
        &state.connections,
        state.config.presence.manual_override_ttl_secs,
    );
    let connections = state.connections.user_connection_count(tid, &user_id);

    Json(serde_json::json!({
        "user_id": user_id,
        "status": status,
        "connections": connections,
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
            let status = state.presence.resolve(
                tid,
                uid,
                &state.connections,
                state.config.presence.manual_override_ttl_secs,
            );
            serde_json::json!({
                "user_id": uid,
                "status": status,
            })
        })
        .collect();

    Json(serde_json::json!({"members": presence}))
}
