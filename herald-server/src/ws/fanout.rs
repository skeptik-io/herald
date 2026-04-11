use std::sync::atomic::Ordering;
use std::sync::Arc;

use herald_core::protocol::ServerMessage;

use crate::registry::connection::ConnId;
use crate::state::AppState;

/// Fan out a message to all subscribers of a stream.
///
/// When `sender` is `Some`, engine fanout filters are applied (e.g. block
/// filtering from the chat engine). When `sender` is `None` (system messages
/// like member join/leave), no filtering is applied.
pub async fn fanout_to_stream(
    state: &Arc<AppState>,
    tenant_id: &str,
    stream_id: &str,
    msg: &ServerMessage,
    exclude_conn: Option<ConnId>,
    sender: Option<&str>,
) {
    let subscribers = state.streams.get_subscriber_conns(tenant_id, stream_id);
    for (user_id, conn_ids) in subscribers {
        // Engine fanout filters (block filtering, etc.)
        if let Some(sender_id) = sender {
            if sender_id != user_id
                && !state
                    .engines
                    .fanout_filter(state, tenant_id, &user_id, sender_id)
                    .await
            {
                continue;
            }
        }

        for conn_id in conn_ids {
            if Some(conn_id) == exclude_conn {
                continue;
            }
            if !state.connections.send_to_conn(conn_id, msg) {
                state.metrics.events_dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}
