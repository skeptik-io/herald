use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tracing::{debug, info};

use herald_core::error::ErrorCode;
use herald_core::protocol::{
    AuthOkPayload, ClientMessage, EventNewPayload, EventsBatchPayload, MemberPresence,
    PresenceChangedPayload, RawFrame, ServerMessage, StreamSubscriberCountPayload,
    SubscribedPayload, WatchlistPayload,
};

use crate::registry::connection::ConnId;
use crate::state::AppState;
use crate::store;
use crate::ws::handler::handle_message;

const CHANNEL_CAPACITY: usize = 256;
const CATCHUP_LIMIT: u32 = 200;

/// Pre-validated auth data passed from the upgrade handler.
pub struct AuthenticatedConn {
    pub tenant_id: String,
    pub user_id: String,
    pub streams: Vec<String>,
    pub watchlist: Vec<String>,
    pub last_seen_at: Option<i64>,
    pub ack_mode: bool,
}

pub struct ConnContext {
    pub conn_id: ConnId,
    pub tenant_id: String,
    pub user_id: String,
    pub streams_claim: Vec<String>,
    pub watchlist: Vec<String>,
    pub ack_mode: bool,
}

pub async fn handle_connection(socket: WebSocket, state: Arc<AppState>, auth: AuthenticatedConn) {
    let conn_id = ConnId::next();
    let (mut ws_tx, mut ws_rx) = socket.split();
    let (msg_tx, mut msg_rx) = mpsc::channel::<ServerMessage>(CHANNEL_CAPACITY);

    let writer = tokio::spawn(async move {
        while let Some(msg) = msg_rx.recv().await {
            let json = msg.to_json();
            if ws_tx.send(Message::Text(json.into())).await.is_err() {
                break;
            }
        }
    });

    let tenant_id = auth.tenant_id;
    let user_id = auth.user_id;
    let streams_claim = auth.streams;
    let ack_mode = auth.ack_mode;
    let last_seen_at = auth.last_seen_at;

    state.connections.register(
        conn_id,
        tenant_id.clone(),
        user_id.clone(),
        msg_tx.clone(),
        ack_mode,
    );
    let _gen = state.connections.increment_generation(&tenant_id, &user_id);

    let _ = msg_tx
        .send(ServerMessage::AuthOk {
            ref_: None,
            payload: AuthOkPayload {
                user_id: user_id.clone(),
                connection_id: conn_id.0,
                server_time: now_millis(),
                heartbeat_interval: (state.config.server.heartbeat_interval_secs * 1000) as u32,
            },
        })
        .await;

    state.event_bus.push_event(
        crate::admin_events::EventKind::Connection,
        Some(tenant_id.clone()),
        serde_json::json!({
            "conn_id": conn_id.to_string(),
            "user_id": &user_id,
        }),
    );

    // When the presence engine is active, it handles broadcast + watchlist
    // via the on_connect hook below. Otherwise, fall back to inline logic.
    #[cfg(not(feature = "presence"))]
    {
        broadcast_presence_change(&state, &tenant_id, &user_id).await;

        // Set up watchlist and send initial online status
        if !auth.watchlist.is_empty() {
            state
                .connections
                .set_watchlist(&tenant_id, &user_id, auth.watchlist.clone());

            let online_ids: Vec<String> = auth
                .watchlist
                .iter()
                .filter(|uid| state.connections.is_user_online(&tenant_id, uid))
                .cloned()
                .collect();
            if !online_ids.is_empty() {
                let _ = msg_tx
                    .send(ServerMessage::WatchlistOnline {
                        payload: WatchlistPayload {
                            user_ids: online_ids,
                            last_seen_at: None,
                        },
                    })
                    .await;
            }
        }

        // Notify users who have this user in their watchlist (only on first connection)
        if state
            .connections
            .user_connection_count(&tenant_id, &user_id)
            == 1
        {
            let watchers = state.connections.get_watchers(&tenant_id, &user_id);
            if !watchers.is_empty() {
                let msg = ServerMessage::WatchlistOnline {
                    payload: WatchlistPayload {
                        user_ids: vec![user_id.clone()],
                        last_seen_at: None,
                    },
                };
                for watcher_id in &watchers {
                    state.connections.send_to_user(&tenant_id, watcher_id, &msg);
                }
            }
        }
    }

    // When presence engine is active, set up watchlist here (the engine
    // handles broadcast and watcher notification, but watchlist init + initial
    // status for THIS connection needs the msg_tx which engines don't have).
    #[cfg(feature = "presence")]
    if !auth.watchlist.is_empty() {
        state
            .connections
            .set_watchlist(&tenant_id, &user_id, auth.watchlist.clone());

        // Send initial presence status for watched users
        let online_ids: Vec<String> = auth
            .watchlist
            .iter()
            .filter(|uid| {
                state.presence.resolve_status(
                    &tenant_id,
                    uid,
                    &state.connections,
                    state.config.presence.manual_override_ttl_secs,
                ) == herald_core::presence::PresenceStatus::Online
            })
            .cloned()
            .collect();
        if !online_ids.is_empty() {
            let _ = msg_tx
                .send(ServerMessage::WatchlistOnline {
                    payload: WatchlistPayload {
                        user_ids: online_ids,
                        last_seen_at: None,
                    },
                })
                .await;
        }
    }

    // Run engine on_connect hooks
    {
        let engine_ctx = crate::engine::EngineConnContext {
            conn_id,
            tenant_id: tenant_id.clone(),
            user_id: user_id.clone(),
        };
        state.engines.on_connect(&state, &engine_ctx).await;
    }

    debug!(conn = %conn_id, tenant = %tenant_id, user = %user_id, "authenticated");

    // Reconnect catchup: subscribes first, then queries historical events.
    // Subscribe-before-query means live fanout and catchup can overlap — the
    // client-side dedup set handles this. A narrow gap exists: if an event's
    // store write is in-flight during the catchup query AND the connection's
    // channel is full (256 msgs) when fanout fires, that event could be missed.
    // This requires simultaneous store-write latency and channel backpressure
    // and is accepted as at-most-once delivery semantics.
    if ack_mode {
        reconnect_catchup_ack(
            &state,
            conn_id,
            &tenant_id,
            &user_id,
            &streams_claim,
            &msg_tx,
        )
        .await;
    } else if let Some(since) = last_seen_at {
        reconnect_catchup(
            &state,
            conn_id,
            &tenant_id,
            &user_id,
            &streams_claim,
            &msg_tx,
            since,
        )
        .await;
    }

    let mut ctx = ConnContext {
        conn_id,
        tenant_id: tenant_id.clone(),
        user_id: user_id.clone(),
        streams_claim,
        watchlist: auth.watchlist,
        ack_mode,
    };

    let max_msg_per_sec = state.config.server.max_messages_per_sec;
    let mut tokens: f64 = max_msg_per_sec as f64;
    let mut last_refill = std::time::Instant::now();

    // Close connection if no frames received within 2× heartbeat interval
    let idle_timeout = Duration::from_secs(state.config.server.heartbeat_interval_secs * 2);

    loop {
        let frame = match tokio::time::timeout(idle_timeout, ws_rx.next()).await {
            Ok(Some(Ok(frame))) => frame,
            Ok(Some(Err(_))) => break, // WebSocket error
            Ok(None) => break,         // Stream ended
            Err(_) => {
                // Idle timeout — client missed heartbeat
                debug!(conn = %conn_id, tenant = %tenant_id, user = %user_id, "idle timeout");
                break;
            }
        };
        match frame {
            Message::Text(text) => {
                // Token bucket rate limiting (no window boundary exploit)
                let elapsed = last_refill.elapsed().as_secs_f64();
                last_refill = std::time::Instant::now();
                tokens = (tokens + elapsed * max_msg_per_sec as f64).min(max_msg_per_sec as f64);
                tokens -= 1.0;
                if tokens < 0.0 {
                    let _ = msg_tx
                        .send(ServerMessage::error(
                            None,
                            ErrorCode::RateLimited,
                            "event rate limit exceeded",
                        ))
                        .await;
                    continue;
                }

                let raw: Result<RawFrame, _> = serde_json::from_str(&text);
                match raw {
                    Ok(raw) => match ClientMessage::from_raw(raw) {
                        Ok(client_msg) => {
                            handle_message(&state, &mut ctx, &msg_tx, client_msg).await;
                        }
                        Err(e) => {
                            state.event_bus.push_error(
                                crate::admin_events::ErrorCategory::Client,
                                Some(ctx.tenant_id.clone()),
                                format!("Bad request: {e}"),
                                serde_json::json!({"conn_id": conn_id.to_string(), "user_id": &ctx.user_id}),
                            );
                            let _ = msg_tx
                                .send(ServerMessage::error(None, ErrorCode::BadRequest, e))
                                .await;
                        }
                    },
                    Err(e) => {
                        state.event_bus.push_error(
                            crate::admin_events::ErrorCategory::Client,
                            Some(ctx.tenant_id.clone()),
                            format!("Invalid JSON: {e}"),
                            serde_json::json!({"conn_id": conn_id.to_string(), "user_id": &ctx.user_id}),
                        );
                        let _ = msg_tx
                            .send(ServerMessage::error(
                                None,
                                ErrorCode::BadRequest,
                                format!("invalid JSON: {e}"),
                            ))
                            .await;
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    debug!(conn = %conn_id, tenant = %tenant_id, user = %user_id, "disconnected");

    // Run engine on_disconnect hooks (typing cleanup, etc.)
    let engine_ctx = crate::engine::EngineConnContext {
        conn_id,
        tenant_id: tenant_id.clone(),
        user_id: user_id.clone(),
    };
    state.engines.on_disconnect(&state, &engine_ctx).await;

    state.event_bus.push_event(
        crate::admin_events::EventKind::Disconnection,
        Some(tenant_id.clone()),
        serde_json::json!({
            "conn_id": conn_id.to_string(),
            "user_id": &user_id,
        }),
    );

    // Flush acked seqs to cursor store before unregistering (at-least-once delivery).
    // This persists the high-water-mark so reconnect catchup (even to a different
    // instance in clustered mode) replays from the correct position.
    if let Some(acked_seqs) = state.connections.drain_acked_seqs(conn_id) {
        let now = now_millis();
        for (stream_id, seq) in &acked_seqs {
            debug!(
                conn = %conn_id,
                stream = %stream_id,
                seq,
                "flushing ack cursor on disconnect"
            );
        }
        for (stream_id, seq) in acked_seqs {
            let result =
                store::cursors::upsert(&*state.db, &tenant_id, &stream_id, &user_id, seq, now)
                    .await;
            if let Err(e) = result {
                tracing::warn!(
                    conn = %conn_id,
                    stream = %stream_id,
                    seq,
                    "failed to flush ack cursor: {e}"
                );
            }
        }
    }

    let affected_streams = state
        .streams
        .unsubscribe_conn_from_all(conn_id, &tenant_id, &user_id);
    state.connections.unregister(conn_id);

    // Broadcast subscriber count updates for affected streams
    for stream_id in &affected_streams {
        let count = state.streams.subscriber_count(&tenant_id, stream_id);
        let count_msg = ServerMessage::StreamSubscriberCount {
            payload: StreamSubscriberCountPayload {
                stream: stream_id.clone(),
                count,
            },
        };
        crate::ws::fanout::fanout_to_stream(&state, &tenant_id, stream_id, &count_msg, None, None)
            .await;
    }

    if state
        .connections
        .user_connection_count(&tenant_id, &user_id)
        == 0
    {
        let linger_secs = state.config.presence.linger_secs;
        let linger_state = state.clone();
        let linger_tenant = tenant_id.clone();
        let linger_user = user_id.clone();
        // Capture generation at disconnect time — if a reconnect happens during linger,
        // the generation will be different and we skip the offline broadcast.
        let gen_at_disconnect = state.connections.current_generation(&tenant_id, &user_id);
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(linger_secs)).await;
            let current_gen = linger_state
                .connections
                .current_generation(&linger_tenant, &linger_user);
            if current_gen == gen_at_disconnect
                && linger_state
                    .connections
                    .user_connection_count(&linger_tenant, &linger_user)
                    == 0
            {
                // When presence engine is not active, handle inline
                #[cfg(not(feature = "presence"))]
                {
                    // Stamp last-seen and persist to WAL
                    let now_ms = now_millis();
                    linger_state
                        .presence
                        .stamp_last_seen(&linger_tenant, &linger_user, now_ms);
                    let entry = crate::registry::presence::PersistedLastSeen {
                        tenant_id: linger_tenant.clone(),
                        user_id: linger_user.clone(),
                        last_seen_at_ms: now_ms,
                    };
                    let _ = crate::store::last_seen::save(&*linger_state.db, &entry).await;

                    broadcast_presence_change(&linger_state, &linger_tenant, &linger_user).await;

                    let watchers = linger_state
                        .connections
                        .get_watchers(&linger_tenant, &linger_user);
                    if !watchers.is_empty() {
                        let msg = ServerMessage::WatchlistOffline {
                            payload: WatchlistPayload {
                                user_ids: vec![linger_user.clone()],
                                last_seen_at: Some(std::collections::HashMap::from([(
                                    linger_user.clone(),
                                    now_ms,
                                )])),
                            },
                        };
                        for watcher_id in &watchers {
                            linger_state
                                .connections
                                .send_to_user(&linger_tenant, watcher_id, &msg);
                        }
                    }

                    linger_state
                        .connections
                        .cleanup_watchlist(&linger_tenant, &linger_user);
                }

                // Run engine on_last_disconnect hooks (presence engine handles
                // broadcast + watchlist + cleanup when active)
                let engine_ctx = crate::engine::EngineConnContext {
                    conn_id,
                    tenant_id: linger_tenant.clone(),
                    user_id: linger_user.clone(),
                };
                linger_state
                    .engines
                    .on_last_disconnect(&linger_state, &engine_ctx)
                    .await;
            }
        });
    }

    drop(msg_tx);
    let _ = writer.await;
}

async fn reconnect_catchup(
    state: &Arc<AppState>,
    conn_id: ConnId,
    tenant_id: &str,
    user_id: &str,
    streams_claim: &[String],
    tx: &mpsc::Sender<ServerMessage>,
    since_ms: i64,
) {
    for stream_id in streams_claim {
        if state.streams.is_public(tenant_id, stream_id) {
            if !state.streams.is_member(tenant_id, stream_id, user_id) {
                state.streams.add_member(tenant_id, stream_id, user_id);
                let member = herald_core::member::Member {
                    stream_id: stream_id.clone(),
                    user_id: user_id.to_string(),
                    role: herald_core::member::Role::Member,
                    joined_at: now_millis(),
                };
                let _ = store::members::insert(&*state.db, tenant_id, &member).await;
            }
        } else if !state.streams.is_member(tenant_id, stream_id, user_id) {
            continue;
        }

        state
            .streams
            .subscribe(tenant_id, stream_id, user_id, conn_id);
        state
            .connections
            .add_stream_subscription(conn_id, stream_id);

        let members = state.streams.get_members(tenant_id, stream_id);
        let mut member_presence = Vec::new();
        for uid in &members {
            if let Ok(Some(member)) =
                store::members::get(&*state.db, tenant_id, stream_id, uid).await
            {
                let presence = state.presence.resolve_status(
                    tenant_id,
                    uid,
                    &state.connections,
                    state.config.presence.manual_override_ttl_secs,
                );
                member_presence.push(MemberPresence {
                    user_id: uid.clone(),
                    role: member.role,
                    presence,
                });
            }
        }

        let cursor = store::cursors::get(&*state.db, tenant_id, stream_id, user_id)
            .await
            .unwrap_or(0);
        let latest_seq = state.streams.current_seq(tenant_id, stream_id);

        let _ = tx
            .send(ServerMessage::Subscribed {
                ref_: None,
                payload: SubscribedPayload {
                    stream: stream_id.clone(),
                    members: member_presence,
                    cursor,
                    latest_seq,
                },
            })
            .await;

        let events = store::events::list_since_time(
            &*state.db,
            tenant_id,
            stream_id,
            since_ms,
            CATCHUP_LIMIT + 1,
        )
        .await
        .unwrap_or_default();

        if !events.is_empty() {
            let has_more = events.len() > CATCHUP_LIMIT as usize;
            let batch: Vec<EventNewPayload> = events
                .into_iter()
                .take(CATCHUP_LIMIT as usize)
                .map(|m| EventNewPayload {
                    stream: m.stream_id,
                    id: m.id.0,
                    seq: m.seq,
                    sender: m.sender,
                    body: m.body,
                    meta: m.meta,
                    parent_id: m.parent_id,
                    sent_at: m.sent_at,
                })
                .collect();

            info!(
                conn = %conn_id,
                tenant = %tenant_id,
                user = %user_id,
                stream = %stream_id,
                events = batch.len(),
                has_more,
                "catch-up replay"
            );

            let _ = tx
                .send(ServerMessage::EventsBatch {
                    ref_: None,
                    payload: EventsBatchPayload {
                        stream: stream_id.clone(),
                        events: batch,
                        has_more,
                    },
                })
                .await;
        }
    }
}

/// Seq-based reconnect catchup for ack-mode connections.
/// Reads the last-acked cursor per stream from the store and replays events
/// with seq > acked_seq. This is the at-least-once delivery path.
async fn reconnect_catchup_ack(
    state: &Arc<AppState>,
    conn_id: ConnId,
    tenant_id: &str,
    user_id: &str,
    streams_claim: &[String],
    tx: &mpsc::Sender<ServerMessage>,
) {
    for stream_id in streams_claim {
        if state.streams.is_public(tenant_id, stream_id) {
            if !state.streams.is_member(tenant_id, stream_id, user_id) {
                state.streams.add_member(tenant_id, stream_id, user_id);
                let member = herald_core::member::Member {
                    stream_id: stream_id.clone(),
                    user_id: user_id.to_string(),
                    role: herald_core::member::Role::Member,
                    joined_at: now_millis(),
                };
                let _ = store::members::insert(&*state.db, tenant_id, &member).await;
            }
        } else if !state.streams.is_member(tenant_id, stream_id, user_id) {
            continue;
        }

        state
            .streams
            .subscribe(tenant_id, stream_id, user_id, conn_id);
        state
            .connections
            .add_stream_subscription(conn_id, stream_id);

        let members = state.streams.get_members(tenant_id, stream_id);
        let mut member_presence = Vec::new();
        for uid in &members {
            if let Ok(Some(member)) =
                store::members::get(&*state.db, tenant_id, stream_id, uid).await
            {
                let presence = state.presence.resolve_status(
                    tenant_id,
                    uid,
                    &state.connections,
                    state.config.presence.manual_override_ttl_secs,
                );
                member_presence.push(MemberPresence {
                    user_id: uid.clone(),
                    role: member.role,
                    presence,
                });
            }
        }

        let cursor = store::cursors::get(&*state.db, tenant_id, stream_id, user_id)
            .await
            .unwrap_or(0);
        let latest_seq = state.streams.current_seq(tenant_id, stream_id);

        let _ = tx
            .send(ServerMessage::Subscribed {
                ref_: None,
                payload: SubscribedPayload {
                    stream: stream_id.clone(),
                    members: member_presence,
                    cursor,
                    latest_seq,
                },
            })
            .await;

        // Read the last-acked seq from the cursor store.
        // The cursor store is shared across instances in clustered mode.
        let acked_seq = store::cursors::get(&*state.db, tenant_id, stream_id, user_id)
            .await
            .unwrap_or(0);

        if acked_seq == 0 {
            // No prior ack — treat as fresh subscriber, no catchup
            continue;
        }

        let events = store::events::list_after(
            &*state.db,
            tenant_id,
            stream_id,
            acked_seq,
            CATCHUP_LIMIT + 1,
        )
        .await
        .unwrap_or_default();

        if !events.is_empty() {
            let has_more = events.len() > CATCHUP_LIMIT as usize;
            let event_count = events.len().min(CATCHUP_LIMIT as usize);
            let batch: Vec<EventNewPayload> = events
                .into_iter()
                .take(CATCHUP_LIMIT as usize)
                .map(|m| EventNewPayload {
                    stream: m.stream_id,
                    id: m.id.0,
                    seq: m.seq,
                    sender: m.sender,
                    body: m.body,
                    meta: m.meta,
                    parent_id: m.parent_id,
                    sent_at: m.sent_at,
                })
                .collect();

            state
                .metrics
                .events_redelivered
                .fetch_add(event_count as u64, std::sync::atomic::Ordering::Relaxed);

            info!(
                conn = %conn_id,
                tenant = %tenant_id,
                user = %user_id,
                stream = %stream_id,
                events = batch.len(),
                has_more,
                acked_seq,
                "ack-mode catch-up replay"
            );

            let _ = tx
                .send(ServerMessage::EventsBatch {
                    ref_: None,
                    payload: EventsBatchPayload {
                        stream: stream_id.clone(),
                        events: batch,
                        has_more,
                    },
                })
                .await;
        }
    }
}

/// Well-known stream name for tenant-wide presence broadcast.
/// Clients subscribe to this stream to receive all presence changes
/// without needing per-user watchlists or shared stream membership.
pub const PRESENCE_STREAM: &str = "__presence";

pub async fn broadcast_presence_change(state: &Arc<AppState>, tenant_id: &str, user_id: &str) {
    let resolved = state.presence.resolve(
        tenant_id,
        user_id,
        &state.connections,
        state.config.presence.manual_override_ttl_secs,
    );
    let msg = ServerMessage::PresenceChanged {
        payload: PresenceChangedPayload {
            user_id: user_id.to_string(),
            presence: resolved.status,
            until: resolved.until.clone(),
            last_seen_at: resolved.last_seen_at,
        },
    };

    // Broadcast to all streams the user is a member of
    for stream_id in state.streams.get_member_streams(tenant_id, user_id) {
        crate::ws::fanout::fanout_to_stream(
            state,
            tenant_id,
            &stream_id,
            &msg,
            None,
            Some(user_id),
        )
        .await;
    }

    // Broadcast to the tenant-wide __presence stream (if anyone is subscribed).
    // This enables global real-time presence for apps where presence isn't
    // scoped to shared streams (e.g. creator/fan platforms, team dashboards).
    if state.streams.subscriber_count(tenant_id, PRESENCE_STREAM) > 0 {
        crate::ws::fanout::fanout_to_stream(
            state,
            tenant_id,
            PRESENCE_STREAM,
            &msg,
            None,
            None, // No sender filtering on presence stream
        )
        .await;
    }
}

pub fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
