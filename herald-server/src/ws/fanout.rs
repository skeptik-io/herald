use std::sync::atomic::Ordering;
use std::sync::Arc;

use herald_core::protocol::ServerMessage;

use crate::registry::connection::ConnId;
use crate::state::AppState;

/// Fan out a message to all subscribers of a stream.
///
/// When `sender` is `Some`, block filtering is applied: subscribers who have
/// blocked the sender will not receive the message. When `sender` is `None`
/// (system messages like member join/leave), no block filtering is applied.
///
/// Block filtering is only active when the `chat` feature is enabled.
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
        // Block filtering: skip this subscriber if they've blocked the sender
        #[cfg(feature = "chat")]
        if let Some(sender_id) = sender {
            if sender_id != user_id {
                match crate::chat::store_blocks::is_blocked(
                    &*state.db, tenant_id, &user_id, sender_id,
                )
                .await
                {
                    Ok(true) => continue,
                    Err(e) => {
                        tracing::warn!(
                            tenant = tenant_id,
                            stream = stream_id,
                            blocker = %user_id,
                            blocked = sender_id,
                            "block check failed during fanout: {e}"
                        );
                        // Fail-open: deliver the message on error
                    }
                    _ => {}
                }
            }
        }
        #[cfg(not(feature = "chat"))]
        let _ = sender;

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
