use std::sync::atomic::Ordering;
use std::sync::Arc;

use tokio::sync::mpsc;

use herald_core::error::ErrorCode;
use herald_core::message::{Message, MessageId};
use herald_core::protocol::*;

use crate::state::AppState;
use crate::store;
use crate::ws::connection::{now_millis, ConnContext};
use crate::ws::fanout::fanout_to_room;

const MAX_FETCH_LIMIT: u32 = 100;
const DEFAULT_FETCH_LIMIT: u32 = 50;
const MESSAGE_TTL_MS: i64 = 7 * 24 * 60 * 60 * 1000;

pub async fn handle_message(
    state: &Arc<AppState>,
    ctx: &mut ConnContext,
    tx: &mpsc::Sender<ServerMessage>,
    msg: ClientMessage,
) {
    match msg {
        ClientMessage::Subscribe { ref_, rooms } => {
            handle_subscribe(state, ctx, tx, ref_, rooms).await;
        }
        ClientMessage::Unsubscribe { ref_, rooms } => {
            handle_unsubscribe(state, ctx, ref_, rooms);
        }
        ClientMessage::MessageSend {
            ref_,
            room,
            body,
            meta,
        } => {
            handle_send(state, ctx, tx, ref_, room, body, meta).await;
        }
        ClientMessage::CursorUpdate { room, seq } => {
            handle_cursor_update(state, ctx, room, seq).await;
        }
        ClientMessage::PresenceSet { ref_, status } => {
            handle_presence_set(state, ctx, tx, ref_, status).await;
        }
        ClientMessage::TypingStart { room } => {
            handle_typing(state, ctx, &room, true);
        }
        ClientMessage::TypingStop { room } => {
            handle_typing(state, ctx, &room, false);
        }
        ClientMessage::MessagesFetch {
            ref_,
            room,
            before,
            limit,
        } => {
            handle_fetch(state, ctx, tx, ref_, room, before, limit).await;
        }
        ClientMessage::MessagesSearch {
            ref_,
            room,
            query,
            limit,
        } => {
            handle_search(state, ctx, tx, ref_, room, query, limit).await;
        }
        ClientMessage::Ping { ref_ } => {
            let _ = tx.send(ServerMessage::Pong { ref_ }).await;
        }
        ClientMessage::Auth { ref_, .. } | ClientMessage::AuthRefresh { ref_, .. } => {
            let _ = tx
                .send(ServerMessage::error(
                    ref_,
                    ErrorCode::BadRequest,
                    "already authenticated",
                ))
                .await;
        }
    }
}

async fn handle_subscribe(
    state: &Arc<AppState>,
    ctx: &mut ConnContext,
    tx: &mpsc::Sender<ServerMessage>,
    ref_: Option<String>,
    rooms: Vec<String>,
) {
    let tid = &ctx.tenant_id;
    for room_id in rooms {
        if !ctx.rooms_claim.contains(&room_id) {
            let _ = tx
                .send(ServerMessage::error(
                    ref_.clone(),
                    ErrorCode::Unauthorized,
                    format!("not authorized for room {room_id}"),
                ))
                .await;
            continue;
        }

        if !state.rooms.is_member(tid, &room_id, &ctx.user_id) {
            let _ = tx
                .send(ServerMessage::error(
                    ref_.clone(),
                    ErrorCode::RoomNotFound,
                    format!("not a member of room {room_id}"),
                ))
                .await;
            continue;
        }

        state
            .rooms
            .subscribe(tid, &room_id, &ctx.user_id, ctx.conn_id);
        state
            .connections
            .add_room_subscription(ctx.conn_id, &room_id);

        let members = state.rooms.get_members(tid, &room_id);
        let mut member_presence = Vec::new();
        for uid in &members {
            if let Ok(Some(member)) = store::members::get(&*state.db, tid, &room_id, uid).await {
                let presence = state.presence.resolve(
                    tid,
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

        let cursor = store::cursors::get(&*state.db, tid, &room_id, &ctx.user_id)
            .await
            .unwrap_or(0);
        let latest_seq = state.rooms.current_seq(tid, &room_id);

        let _ = tx
            .send(ServerMessage::Subscribed {
                ref_: ref_.clone(),
                payload: SubscribedPayload {
                    room: room_id,
                    members: member_presence,
                    cursor,
                    latest_seq,
                },
            })
            .await;
    }
}

fn handle_unsubscribe(
    state: &Arc<AppState>,
    ctx: &mut ConnContext,
    _ref_: Option<String>,
    rooms: Vec<String>,
) {
    for room_id in rooms {
        state
            .rooms
            .unsubscribe(&ctx.tenant_id, &room_id, &ctx.user_id, ctx.conn_id);
        state
            .connections
            .remove_room_subscription(ctx.conn_id, &room_id);
    }
}

async fn handle_send(
    state: &Arc<AppState>,
    ctx: &ConnContext,
    tx: &mpsc::Sender<ServerMessage>,
    ref_: Option<String>,
    room: String,
    body: String,
    meta: Option<serde_json::Value>,
) {
    let tid = &ctx.tenant_id;

    if !state.rooms.is_member(tid, &room, &ctx.user_id) {
        let _ = tx
            .send(ServerMessage::error(
                ref_,
                ErrorCode::NotSubscribed,
                "not a member of this room",
            ))
            .await;
        return;
    }

    if let Err(e) = state
        .authorize(tid, &ctx.user_id, "message.send", &room)
        .await
    {
        let _ = tx
            .send(ServerMessage::error(ref_, ErrorCode::Unauthorized, e))
            .await;
        return;
    }

    let total_start = std::time::Instant::now();

    let seq = state.rooms.next_seq(tid, &room);
    let now = now_millis();
    let msg_id = uuid::Uuid::new_v4().to_string();

    // Encrypt
    let encrypt_start = std::time::Instant::now();
    let stored_body = match state.encrypt_body(tid, &room, &body).await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("encryption failed: {e}");
            let _ = tx
                .send(ServerMessage::error(ref_, ErrorCode::Internal, e))
                .await;
            return;
        }
    };
    state.metrics.message_encrypt.observe_since(encrypt_start);

    let message = Message {
        id: MessageId(msg_id.clone()),
        room_id: room.clone(),
        seq,
        sender: ctx.user_id.clone(),
        body: stored_body,
        meta: meta.clone(),
        sent_at: now,
    };

    // Store
    let store_start = std::time::Instant::now();
    if let Err(e) = store::messages::insert(&*state.db, tid, &message, now + MESSAGE_TTL_MS).await {
        tracing::error!("failed to store message: {e}");
        let _ = tx
            .send(ServerMessage::error(
                ref_,
                ErrorCode::Internal,
                "storage error",
            ))
            .await;
        return;
    }
    state.metrics.message_store.observe_since(store_start);

    state.metrics.messages_sent.fetch_add(1, Ordering::Relaxed);

    // Index
    let index_start = std::time::Instant::now();
    state.index_message(tid, &room, &msg_id, &body).await;
    state.metrics.message_index.observe_since(index_start);

    let _ = tx
        .send(ServerMessage::MessageAck {
            ref_,
            payload: MessageAckPayload {
                id: msg_id.clone(),
                seq,
                sent_at: now,
            },
        })
        .await;

    // Fan-out
    let fanout_start = std::time::Instant::now();
    let new_msg = ServerMessage::MessageNew {
        payload: MessageNewPayload {
            room: room.clone(),
            id: msg_id.clone(),
            seq,
            sender: ctx.user_id.clone(),
            body: body.clone(),
            meta: meta.clone(),
            sent_at: now,
        },
    };
    fanout_to_room(state, tid, &room, &new_msg, None);
    state.metrics.message_fanout.observe_since(fanout_start);

    state.fire_webhook(crate::webhook::WebhookEvent {
        event: "message.new".to_string(),
        room: room.clone(),
        id: Some(msg_id),
        seq: Some(seq),
        sender: Some(ctx.user_id.clone()),
        body: Some(body.clone()),
        meta,
        sent_at: Some(now),
        user_id: None,
        role: None,
    });

    state.audit(tid, "message.send", &room, &ctx.user_id, "success");
    state.notify_offline_members(tid, &room, &ctx.user_id, &body);

    state.metrics.message_total.observe_since(total_start);
}

async fn handle_cursor_update(state: &Arc<AppState>, ctx: &ConnContext, room: String, seq: u64) {
    let now = now_millis();
    let _ = store::cursors::upsert(&*state.db, &ctx.tenant_id, &room, &ctx.user_id, seq, now).await;

    let msg = ServerMessage::CursorMoved {
        payload: CursorMovedPayload {
            room: room.clone(),
            user_id: ctx.user_id.clone(),
            seq,
        },
    };
    fanout_to_room(state, &ctx.tenant_id, &room, &msg, Some(ctx.conn_id));
}

async fn handle_presence_set(
    state: &Arc<AppState>,
    ctx: &ConnContext,
    _tx: &mpsc::Sender<ServerMessage>,
    _ref_: Option<String>,
    status: herald_core::presence::PresenceStatus,
) {
    state
        .presence
        .set_manual(&ctx.tenant_id, &ctx.user_id, status);

    let msg = ServerMessage::PresenceChanged {
        payload: PresenceChangedPayload {
            user_id: ctx.user_id.clone(),
            presence: status,
        },
    };
    for room_id in state.rooms.get_member_rooms(&ctx.tenant_id, &ctx.user_id) {
        fanout_to_room(state, &ctx.tenant_id, &room_id, &msg, None);
    }
}

fn handle_typing(state: &Arc<AppState>, ctx: &ConnContext, room: &str, active: bool) {
    let msg = ServerMessage::Typing {
        payload: TypingPayload {
            room: room.to_string(),
            user_id: ctx.user_id.clone(),
            active,
        },
    };
    fanout_to_room(state, &ctx.tenant_id, room, &msg, Some(ctx.conn_id));
}

async fn handle_fetch(
    state: &Arc<AppState>,
    ctx: &ConnContext,
    tx: &mpsc::Sender<ServerMessage>,
    ref_: Option<String>,
    room: String,
    before: Option<u64>,
    limit: Option<u32>,
) {
    let tid = &ctx.tenant_id;

    if !state.rooms.is_member(tid, &room, &ctx.user_id) {
        let _ = tx
            .send(ServerMessage::error(
                ref_,
                ErrorCode::NotSubscribed,
                "not a member of this room",
            ))
            .await;
        return;
    }

    let limit = limit.unwrap_or(DEFAULT_FETCH_LIMIT).min(MAX_FETCH_LIMIT);
    let before_seq = before.unwrap_or(i64::MAX as u64);

    let messages = store::messages::list_before(&*state.db, tid, &room, before_seq, limit + 1)
        .await
        .unwrap_or_default();

    let has_more = messages.len() > limit as usize;
    let mut result = Vec::with_capacity(limit as usize);
    for m in messages.into_iter().take(limit as usize) {
        let body = match state.decrypt_body(tid, &room, &m.body).await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(id = %m.id.0, "skipping message: decrypt failed: {e}");
                continue;
            }
        };
        result.push(MessageNewPayload {
            room: m.room_id,
            id: m.id.0,
            seq: m.seq,
            sender: m.sender,
            body,
            meta: m.meta,
            sent_at: m.sent_at,
        });
    }
    let messages = result;

    let _ = tx
        .send(ServerMessage::MessagesBatch {
            ref_,
            payload: MessagesBatchPayload {
                room,
                messages,
                has_more,
            },
        })
        .await;
}

async fn handle_search(
    state: &Arc<AppState>,
    ctx: &ConnContext,
    tx: &mpsc::Sender<ServerMessage>,
    ref_: Option<String>,
    room: String,
    query: String,
    limit: Option<u32>,
) {
    let tid = &ctx.tenant_id;

    if !state.rooms.is_member(tid, &room, &ctx.user_id) {
        let _ = tx
            .send(ServerMessage::error(
                ref_,
                ErrorCode::NotSubscribed,
                "not a member of this room",
            ))
            .await;
        return;
    }

    let hits = match state
        .search_messages(tid, &room, &query, limit.map(|l| l as usize))
        .await
    {
        Ok(h) => h,
        Err(e) => {
            let _ = tx
                .send(ServerMessage::error(ref_, ErrorCode::SearchUnavailable, e))
                .await;
            return;
        }
    };

    let mut messages = Vec::with_capacity(hits.len());
    for hit in &hits {
        if let Ok(Some(msg)) = store::messages::get_by_id(&*state.db, tid, &hit.id).await {
            messages.push(msg);
        }
    }

    let mut result = Vec::with_capacity(messages.len());
    for m in messages {
        let body = match state.decrypt_body(tid, &room, &m.body).await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(id = %m.id.0, "skipping search result: decrypt failed: {e}");
                continue;
            }
        };
        result.push(MessageNewPayload {
            room: m.room_id,
            id: m.id.0,
            seq: m.seq,
            sender: m.sender,
            body,
            meta: m.meta,
            sent_at: m.sent_at,
        });
    }

    let _ = tx
        .send(ServerMessage::MessagesBatch {
            ref_,
            payload: MessagesBatchPayload {
                room,
                messages: result,
                has_more: false,
            },
        })
        .await;
}
