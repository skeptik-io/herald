use std::sync::Arc;

use axum::extract::ws::{WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use serde::Deserialize;

use crate::state::AppState;
use crate::ws::connection::{handle_connection, AuthenticatedConn};

#[derive(Debug, Deserialize)]
pub struct WsAuthQuery {
    pub key: String,
    pub token: String,
    pub user_id: String,
    /// Comma-separated stream IDs.
    pub streams: String,
    /// Comma-separated user IDs for presence watchlist (optional).
    #[serde(default)]
    pub watchlist: String,
    /// Reconnect catchup: last seen timestamp in ms.
    #[serde(default)]
    pub last_seen_at: Option<i64>,
    /// Enable at-least-once delivery.
    #[serde(default)]
    pub ack_mode: Option<bool>,
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Query(auth): Query<WsAuthQuery>,
) -> impl IntoResponse {
    let streams: Vec<String> = auth
        .streams
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let watchlist: Vec<String> = if auth.watchlist.is_empty() {
        vec![]
    } else {
        auth.watchlist
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    };

    // Validate BEFORE upgrading — no resources allocated on auth failure
    let tenant_id = match state
        .validate_signed_token(&auth.key, &auth.token, &auth.user_id, &streams, &watchlist)
        .await
    {
        Ok(tid) => tid,
        Err(e) => {
            tracing::debug!(key = %auth.key, "ws auth failed: {e}");
            state
                .metrics
                .ws_auth_failures
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            state.event_bus.push_event(
                crate::admin_events::EventKind::AuthFailure,
                None,
                serde_json::json!({"key": &auth.key, "error": "authentication failed"}),
            );
            return (
                axum::http::StatusCode::UNAUTHORIZED,
                "authentication failed",
            )
                .into_response();
        }
    };

    // Check per-tenant connection limit BEFORE upgrade
    let tenant_conns = state.connections.tenant_connection_count(&tenant_id);
    let max_conns = state.config.tenant_limits.max_connections_per_tenant as usize;
    if tenant_conns >= max_conns {
        return (
            axum::http::StatusCode::TOO_MANY_REQUESTS,
            "tenant connection limit reached",
        )
            .into_response();
    }

    let max_size = state.config.server.ws_max_message_size;
    let authenticated = AuthenticatedConn {
        tenant_id,
        user_id: auth.user_id,
        streams,
        watchlist,
        last_seen_at: auth.last_seen_at,
        ack_mode: auth.ack_mode.unwrap_or(false),
    };

    ws.max_message_size(max_size)
        .max_frame_size(max_size)
        .on_upgrade(move |socket| handle_socket(socket, state, authenticated))
        .into_response()
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>, auth: AuthenticatedConn) {
    handle_connection(socket, state, auth).await;
}
