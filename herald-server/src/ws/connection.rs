use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tracing::{debug, info};

use herald_core::error::ErrorCode;
use herald_core::protocol::{
    AuthOkPayload, ClientMessage, MemberPresence, MessageNewPayload, MessagesBatchPayload,
    PresenceChangedPayload, RawFrame, ServerMessage, SubscribedPayload,
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
    };

    let max_msg_per_sec = state.config.server.max_messages_per_sec;
    let mut rate_window_start = std::time::Instant::now();
    let mut rate_count: u32 = 0;

    while let Some(Ok(frame)) = ws_rx.next().await {
        match frame {
            Message::Text(text) => {
                if rate_window_start.elapsed().as_secs() >= 1 {
                    rate_window_start = std::time::Instant::now();
                    rate_count = 0;
                }
                rate_count += 1;
                if rate_count > max_msg_per_sec {
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

    state.event_bus.push_event(
        crate::admin_events::EventKind::Disconnection,
        Some(tenant_id.clone()),
        serde_json::json!({
            "conn_id": conn_id.to_string(),
            "user_id": &user_id,
        }),
    );

    state
        .rooms
        .unsubscribe_conn_from_all(conn_id, &tenant_id, &user_id);
    state.connections.unregister(conn_id);

    if state
        .connections
        .user_connection_count(&tenant_id, &user_id)
        == 0
    {
        let linger_secs = state.config.presence.linger_secs;
        let linger_state = state.clone();
        let linger_tenant = tenant_id.clone();
        let linger_user = user_id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(linger_secs)).await;
            if linger_state
                .connections
                .user_connection_count(&linger_tenant, &linger_user)
                == 0
            {
                broadcast_presence_change(&linger_state, &linger_tenant, &linger_user);
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
        if !state.rooms.is_member(tenant_id, room_id, user_id) {
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
            CATCHUP_LIMIT,
        )
        .await
        .unwrap_or_default();

        if !messages.is_empty() {
            let mut batch = Vec::with_capacity(messages.len());
            for m in messages {
                let body = match state.decrypt_body(tenant_id, room_id, &m.body).await {
                    Ok(b) => b,
                    Err(e) => {
                        tracing::warn!(id = %m.id.0, "skipping catch-up message: decrypt failed: {e}");
                        continue;
                    }
                };
                batch.push(MessageNewPayload {
                    room: m.room_id,
                    id: m.id.0,
                    seq: m.seq,
                    sender: m.sender,
                    body,
                    meta: m.meta,
                    sent_at: m.sent_at,
                });
            }

            info!(
                conn = %conn_id,
                tenant = %tenant_id,
                user = %user_id,
                room = %room_id,
                messages = batch.len(),
                "catch-up replay"
            );

            let _ = tx
                .send(ServerMessage::MessagesBatch {
                    ref_: None,
                    payload: MessagesBatchPayload {
                        room: room_id.clone(),
                        messages: batch,
                        has_more: false,
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
