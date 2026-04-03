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
        ClientMessage::MessageDelete { ref_, room, id } => {
            handle_delete(state, ctx, tx, ref_, room, id).await;
        }
        ClientMessage::EventTrigger {
            ref_,
            room,
            event,
            data,
        } => {
            handle_event_trigger(state, ctx, tx, ref_, room, event, data).await;
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

        // For public rooms, auto-add as member on first subscribe
        if state.rooms.is_public(tid, &room_id) {
            if !state.rooms.is_member(tid, &room_id, &ctx.user_id) {
                state.rooms.add_member(tid, &room_id, &ctx.user_id);
                let member = herald_core::member::Member {
                    room_id: room_id.clone(),
                    user_id: ctx.user_id.clone(),
                    role: herald_core::member::Role::Member,
                    joined_at: now_millis(),
                };
                let _ = store::members::insert(&*state.db, tid, &member).await;
            }
        } else if !state.rooms.is_member(tid, &room_id, &ctx.user_id) {
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

        let room_id_ref = room_id.clone();
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

        // Broadcast subscriber count update
        let count = state.rooms.subscriber_count(tid, &room_id_ref);
        let count_msg = ServerMessage::RoomSubscriberCount {
            payload: RoomSubscriberCountPayload {
                room: room_id_ref.clone(),
                count,
            },
        };
        fanout_to_room(state, tid, &room_id_ref, &count_msg, None);

        // Cache channel: deliver last event to new subscriber
        if let Some(cached) = state.rooms.get_last_event(tid, &room_id_ref) {
            let _ = tx.send(cached).await;
        }
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

        // Broadcast subscriber count update
        let count = state.rooms.subscriber_count(&ctx.tenant_id, &room_id);
        let count_msg = ServerMessage::RoomSubscriberCount {
            payload: RoomSubscriberCountPayload {
                room: room_id.clone(),
                count,
            },
        };
        fanout_to_room(state, &ctx.tenant_id, &room_id, &count_msg, None);
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

    if body.len() > 65_536 {
        let _ = tx
            .send(ServerMessage::error(
                ref_,
                ErrorCode::BadRequest,
                "message body exceeds maximum length",
            ))
            .await;
        return;
    }

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

    if state.rooms.is_archived(tid, &room) {
        let _ = tx
            .send(ServerMessage::error(
                ref_,
                ErrorCode::BadRequest,
                "room is archived",
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

    let message = Message {
        id: MessageId(msg_id.clone()),
        room_id: room.clone(),
        seq,
        sender: ctx.user_id.clone(),
        body: body.clone(),
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
    state.increment_tenant_messages(tid);

    state.event_bus.push_event(
        crate::admin_events::EventKind::Message,
        Some(tid.to_string()),
        serde_json::json!({
            "room": &room,
            "sender": &ctx.user_id,
            "msg_id": &msg_id,
            "seq": seq,
        }),
    );

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

    // Cache channel: update last event for new subscribers
    state.rooms.set_last_event(tid, &room, new_msg);

    state.fire_webhook(
        tid,
        crate::webhook::WebhookEvent {
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
        },
    );

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
    state
        .typing
        .set_typing(&ctx.tenant_id, room, &ctx.user_id, active);
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
    let messages: Vec<MessageNewPayload> = messages
        .into_iter()
        .take(limit as usize)
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

async fn handle_delete(
    state: &Arc<AppState>,
    ctx: &ConnContext,
    tx: &mpsc::Sender<ServerMessage>,
    ref_: Option<String>,
    room: String,
    id: String,
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

    // Look up the message to verify ownership
    let msg = match store::messages::get_by_id(&*state.db, tid, &id).await {
        Ok(Some(m)) => m,
        Ok(None) => {
            let _ = tx
                .send(ServerMessage::error(
                    ref_,
                    ErrorCode::BadRequest,
                    "message not found",
                ))
                .await;
            return;
        }
        Err(e) => {
            tracing::error!("failed to look up message {id}: {e}");
            let _ = tx
                .send(ServerMessage::error(
                    ref_,
                    ErrorCode::Internal,
                    "internal error",
                ))
                .await;
            return;
        }
    };

    // Only the sender can delete their own message (admin/owner enforcement could be added via Sentry)
    if msg.sender != ctx.user_id {
        if let Err(e) = state
            .authorize(tid, &ctx.user_id, "message.delete", &room)
            .await
        {
            let _ = tx
                .send(ServerMessage::error(ref_, ErrorCode::Unauthorized, e))
                .await;
            return;
        }
    }

    match store::messages::delete_message(&*state.db, tid, &id).await {
        Ok(Some(original)) => {
            let deleted_msg = ServerMessage::MessageDeleted {
                payload: MessageDeletedPayload {
                    room: room.clone(),
                    id: id.clone(),
                    seq: original.seq,
                },
            };
            fanout_to_room(state, tid, &room, &deleted_msg, None);

            // Ack to sender
            let _ = tx
                .send(ServerMessage::MessageAck {
                    ref_,
                    payload: MessageAckPayload {
                        id,
                        seq: original.seq,
                        sent_at: now_millis(),
                    },
                })
                .await;
        }
        Ok(None) => {
            let _ = tx
                .send(ServerMessage::error(
                    ref_,
                    ErrorCode::BadRequest,
                    "message not found",
                ))
                .await;
        }
        Err(e) => {
            tracing::error!("failed to delete message {id}: {e}");
            let _ = tx
                .send(ServerMessage::error(
                    ref_,
                    ErrorCode::Internal,
                    "internal error",
                ))
                .await;
        }
    }
}

async fn handle_event_trigger(
    state: &Arc<AppState>,
    ctx: &ConnContext,
    tx: &mpsc::Sender<ServerMessage>,
    ref_: Option<String>,
    room: String,
    event: String,
    data: Option<serde_json::Value>,
) {
    let tid = &ctx.tenant_id;

    // Must be subscribed (member) to trigger events
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

    if state.rooms.is_archived(tid, &room) {
        let _ = tx
            .send(ServerMessage::error(
                ref_,
                ErrorCode::BadRequest,
                "room is archived",
            ))
            .await;
        return;
    }

    // Fan-out to all subscribers except sender
    let msg = ServerMessage::EventReceived {
        payload: EventReceivedPayload {
            room: room.clone(),
            event: event.clone(),
            sender: ctx.user_id.clone(),
            data,
        },
    };
    fanout_to_room(state, tid, &room, &msg, Some(ctx.conn_id));

    // Ack to sender (lightweight — no seq/id since ephemeral)
    if let Some(r) = ref_ {
        let _ = tx.send(ServerMessage::Pong { ref_: Some(r) }).await;
    }
}
