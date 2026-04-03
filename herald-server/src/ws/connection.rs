use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tracing::{debug, info};

use herald_core::error::ErrorCode;
use herald_core::protocol::{
    AuthOkPayload, ClientMessage, MemberPresence, MessageNewPayload, MessagesBatchPayload,
    PresenceChangedPayload, RawFrame, RoomSubscriberCountPayload, ServerMessage, SubscribedPayload,
    WatchlistPayload,
};

use crate::registry::connection::ConnId;
use crate::state::AppState;
use crate::store;
use crate::ws::handler::handle_message;

const CHANNEL_CAPACITY: usize = 256;
const AUTH_TIMEOUT_SECS: u64 = 5;
const CATCHUP_LIMIT: u32 = 200;

pub struct ConnContext {
    pub conn_id: ConnId,
    pub tenant_id: String,
    pub user_id: String,
    pub rooms_claim: Vec<String>,
    pub watchlist: Vec<String>,
}

pub async fn handle_connection(socket: WebSocket, state: Arc<AppState>) {
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

    let auth_data = tokio::time::timeout(
        std::time::Duration::from_secs(AUTH_TIMEOUT_SECS),
        wait_for_auth(&mut ws_rx),
    )
    .await;

    let (token, auth_ref, last_seen_at) = match auth_data {
        Ok(Some(data)) => data,
        Ok(None) => {
            debug!(conn = %conn_id, "closed before auth");
            writer.abort();
            return;
        }
        Err(_) => {
            let _ = msg_tx
                .send(ServerMessage::AuthError {
                    ref_: None,
                    payload: herald_core::protocol::ErrorPayload {
                        code: ErrorCode::TokenExpired,
                        message: "auth timeout".to_string(),
                    },
                })
                .await;
            drop(msg_tx);
            let _ = writer.await;
            return;
        }
    };

    let (tenant_id, claims) = match state.validate_jwt(&token) {
        Ok(v) => v,
        Err(e) => {
            state
                .metrics
                .ws_auth_failures
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            state.event_bus.push_event(
                crate::admin_events::EventKind::AuthFailure,
                None,
                serde_json::json!({
                    "conn_id": conn_id.to_string(),
                    "error": &e,
                }),
            );
            state.event_bus.push_error(
                crate::admin_events::ErrorCategory::Client,
                format!("Auth failure: {e}"),
                serde_json::json!({"conn_id": conn_id.to_string()}),
            );
            let _ = msg_tx
                .send(ServerMessage::AuthError {
                    ref_: auth_ref.clone(),
                    payload: herald_core::protocol::ErrorPayload {
                        code: ErrorCode::TokenInvalid,
                        message: e,
                    },
                })
                .await;
            tokio::task::yield_now().await;
            drop(msg_tx);
            let _ = writer.await;
            return;
        }
    };

    let user_id = claims.sub.clone();
    let rooms_claim = claims.rooms.clone();

    // Enforce per-tenant connection limit
    let tenant_conns = state.connections.tenant_connection_count(&tenant_id);
    let max_conns = state.config.tenant_limits.max_connections_per_tenant as usize;
    if tenant_conns >= max_conns {
        let _ = msg_tx
            .send(ServerMessage::error(
                auth_ref,
                ErrorCode::RateLimited,
                format!("tenant connection limit reached ({max_conns})"),
            ))
            .await;
        drop(msg_tx);
        let _ = writer.await;
        return;
    }

    state
        .connections
        .register(conn_id, tenant_id.clone(), user_id.clone(), msg_tx.clone());
    let _gen = state.connections.increment_generation(&tenant_id, &user_id);

    let _ = msg_tx
        .send(ServerMessage::AuthOk {
            ref_: auth_ref,
            payload: AuthOkPayload {
                user_id: user_id.clone(),
                server_time: now_millis(),
                heartbeat_interval: 30000,
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

    broadcast_presence_change(&state, &tenant_id, &user_id);

    // Set up watchlist from JWT claims and send initial online status
    if !claims.watchlist.is_empty() {
        state
            .connections
            .set_watchlist(&tenant_id, &user_id, claims.watchlist.clone());

        // Send initial online status for watched users who are already connected
        let online_ids: Vec<String> = claims
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
                },
            };
            for watcher_id in &watchers {
                state.connections.send_to_user(&tenant_id, watcher_id, &msg);
            }
        }
    }

    debug!(conn = %conn_id, tenant = %tenant_id, user = %user_id, "authenticated");

    if let Some(since) = last_seen_at {
        reconnect_catchup(
            &state,
            conn_id,
            &tenant_id,
            &user_id,
            &rooms_claim,
            &msg_tx,
            since,
        )
        .await;
    }

    let mut ctx = ConnContext {
        conn_id,
        tenant_id: tenant_id.clone(),
        user_id: user_id.clone(),
        rooms_claim,
        watchlist: claims.watchlist.clone(),
    };

    let max_msg_per_sec = state.config.server.max_messages_per_sec;
    let mut tokens: f64 = max_msg_per_sec as f64;
    let mut last_refill = std::time::Instant::now();

    while let Some(Ok(frame)) = ws_rx.next().await {
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
                            "message rate limit exceeded",
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

    // Clear typing state and broadcast stop for any rooms this user was typing in
    let typing_rooms = state.typing.remove_user(&tenant_id, &user_id);
    for room_id in typing_rooms {
        let msg = ServerMessage::Typing {
            payload: herald_core::protocol::TypingPayload {
                room: room_id.clone(),
                user_id: user_id.clone(),
                active: false,
            },
        };
        crate::ws::fanout::fanout_to_room(&state, &tenant_id, &room_id, &msg, None);
    }

    state.event_bus.push_event(
        crate::admin_events::EventKind::Disconnection,
        Some(tenant_id.clone()),
        serde_json::json!({
            "conn_id": conn_id.to_string(),
            "user_id": &user_id,
        }),
    );

    let affected_rooms = state
        .rooms
        .unsubscribe_conn_from_all(conn_id, &tenant_id, &user_id);
    state.connections.unregister(conn_id);

    // Broadcast subscriber count updates for affected rooms
    for room_id in &affected_rooms {
        let count = state.rooms.subscriber_count(&tenant_id, room_id);
        let count_msg = ServerMessage::RoomSubscriberCount {
            payload: RoomSubscriberCountPayload {
                room: room_id.clone(),
                count,
            },
        };
        crate::ws::fanout::fanout_to_room(&state, &tenant_id, room_id, &count_msg, None);
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
                broadcast_presence_change(&linger_state, &linger_tenant, &linger_user);

                // Notify watchlist watchers
                let watchers = linger_state
                    .connections
                    .get_watchers(&linger_tenant, &linger_user);
                if !watchers.is_empty() {
                    let msg = ServerMessage::WatchlistOffline {
                        payload: WatchlistPayload {
                            user_ids: vec![linger_user.clone()],
                        },
                    };
                    for watcher_id in &watchers {
                        linger_state
                            .connections
                            .send_to_user(&linger_tenant, watcher_id, &msg);
                    }
                }

                // Clean up watchlist entries for the disconnected user
                linger_state
                    .connections
                    .cleanup_watchlist(&linger_tenant, &linger_user);
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
    rooms_claim: &[String],
    tx: &mpsc::Sender<ServerMessage>,
    since_ms: i64,
) {
    for room_id in rooms_claim {
        if state.rooms.is_public(tenant_id, room_id) {
            if !state.rooms.is_member(tenant_id, room_id, user_id) {
                state.rooms.add_member(tenant_id, room_id, user_id);
                let member = herald_core::member::Member {
                    room_id: room_id.clone(),
                    user_id: user_id.to_string(),
                    role: herald_core::member::Role::Member,
                    joined_at: now_millis(),
                };
                let _ = store::members::insert(&*state.db, tenant_id, &member).await;
            }
        } else if !state.rooms.is_member(tenant_id, room_id, user_id) {
            continue;
        }

        state.rooms.subscribe(tenant_id, room_id, user_id, conn_id);
        state.connections.add_room_subscription(conn_id, room_id);

        let members = state.rooms.get_members(tenant_id, room_id);
        let mut member_presence = Vec::new();
        for uid in &members {
            if let Ok(Some(member)) = store::members::get(&*state.db, tenant_id, room_id, uid).await
            {
                let presence = state.presence.resolve(
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

        let cursor = store::cursors::get(&*state.db, tenant_id, room_id, user_id)
            .await
            .unwrap_or(0);
        let latest_seq = state.rooms.current_seq(tenant_id, room_id);

        let _ = tx
            .send(ServerMessage::Subscribed {
                ref_: None,
                payload: SubscribedPayload {
                    room: room_id.clone(),
                    members: member_presence,
                    cursor,
                    latest_seq,
                },
            })
            .await;

        let messages = store::messages::list_since_time(
            &*state.db,
            tenant_id,
            room_id,
            since_ms,
            CATCHUP_LIMIT + 1,
        )
        .await
        .unwrap_or_default();

        if !messages.is_empty() {
            let has_more = messages.len() > CATCHUP_LIMIT as usize;
            let batch: Vec<MessageNewPayload> = messages
                .into_iter()
                .take(CATCHUP_LIMIT as usize)
                .map(|m| MessageNewPayload {
                    room: m.room_id,
                    id: m.id.0,
                    seq: m.seq,
                    sender: m.sender,
                    body: m.body,
                    meta: m.meta,
                    sent_at: m.sent_at,
                })
                .collect();

            info!(
                conn = %conn_id,
                tenant = %tenant_id,
                user = %user_id,
                room = %room_id,
                messages = batch.len(),
                has_more,
                "catch-up replay"
            );

            let _ = tx
                .send(ServerMessage::MessagesBatch {
                    ref_: None,
                    payload: MessagesBatchPayload {
                        room: room_id.clone(),
                        messages: batch,
                        has_more,
                    },
                })
                .await;
        }
    }
}

async fn wait_for_auth(
    ws_rx: &mut futures_util::stream::SplitStream<WebSocket>,
) -> Option<(String, Option<String>, Option<i64>)> {
    while let Some(Ok(frame)) = ws_rx.next().await {
        if let Message::Text(text) = frame {
            if let Ok(raw) = serde_json::from_str::<RawFrame>(&text) {
                if raw.type_ == "auth" {
                    if let Ok(ClientMessage::Auth {
                        ref_,
                        token,
                        last_seen_at,
                    }) = ClientMessage::from_raw(raw)
                    {
                        return Some((token, ref_, last_seen_at));
                    }
                }
            }
        }
    }
    None
}

pub fn broadcast_presence_change(state: &Arc<AppState>, tenant_id: &str, user_id: &str) {
    let status = state.presence.resolve(
        tenant_id,
        user_id,
        &state.connections,
        state.config.presence.manual_override_ttl_secs,
    );
    let msg = ServerMessage::PresenceChanged {
        payload: PresenceChangedPayload {
            user_id: user_id.to_string(),
            presence: status,
        },
    };
    for room_id in state.rooms.get_member_rooms(tenant_id, user_id) {
        crate::ws::fanout::fanout_to_room(state, tenant_id, &room_id, &msg, None);
    }
}

pub fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
