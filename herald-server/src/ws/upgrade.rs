use std::sync::Arc;

use axum::extract::ws::{WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;

use crate::state::AppState;
use crate::ws::connection::handle_connection;

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let max_size = state.config.server.ws_max_message_size;
    ws.max_message_size(max_size)
        .max_frame_size(max_size)
        .on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    handle_connection(socket, state).await;
}
