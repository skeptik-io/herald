use std::sync::Arc;

use herald_core::protocol::ServerMessage;

use crate::registry::connection::ConnId;
use crate::state::AppState;

pub fn fanout_to_room(
    state: &Arc<AppState>,
    tenant_id: &str,
    room_id: &str,
    msg: &ServerMessage,
    exclude_conn: Option<ConnId>,
) {
    let subscribers = state.rooms.get_subscriber_conns(tenant_id, room_id);
    for (_user_id, conn_ids) in subscribers {
        for conn_id in conn_ids {
            if Some(conn_id) == exclude_conn {
                continue;
            }
            state.connections.send_to_conn(conn_id, msg);
        }
    }
}
