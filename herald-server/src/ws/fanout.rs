use std::sync::atomic::Ordering;
use std::sync::Arc;

use herald_core::protocol::ServerMessage;

use crate::registry::connection::ConnId;
use crate::state::AppState;

pub fn fanout_to_stream(
    state: &Arc<AppState>,
    tenant_id: &str,
    stream_id: &str,
    msg: &ServerMessage,
    exclude_conn: Option<ConnId>,
) {
    let subscribers = state.streams.get_subscriber_conns(tenant_id, stream_id);
    for (_user_id, conn_ids) in subscribers {
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
