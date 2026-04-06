use std::sync::atomic::Ordering;
use std::sync::Arc;

use tokio::sync::mpsc;

use herald_core::error::ErrorCode;
use herald_core::event::{Event, EventId};
use herald_core::protocol::*;

use crate::state::AppState;
use crate::store;
use crate::ws::connection::{now_millis, ConnContext};
use crate::ws::fanout::fanout_to_stream;

const MAX_FETCH_LIMIT: u32 = 100;
const DEFAULT_FETCH_LIMIT: u32 = 50;
const EVENT_TTL_MS: i64 = 7 * 24 * 60 * 60 * 1000;

pub async fn handle_message(
    state: &Arc<AppState>,
    ctx: &mut ConnContext,
    tx: &mpsc::Sender<ServerMessage>,
    msg: ClientMessage,
) {
    match msg {
        ClientMessage::Subscribe { ref_, streams } => {
            handle_subscribe(state, ctx, tx, ref_, streams).await;
        }
        ClientMessage::Unsubscribe { ref_, streams } => {
            handle_unsubscribe(state, ctx, ref_, streams);
        }
        ClientMessage::EventPublish {
            ref_,
            stream,
            body,
            meta,
            parent_id,
        } => {
            handle_publish(state, ctx, tx, ref_, stream, body, meta, parent_id).await;
        }
        ClientMessage::EventEdit {
            ref_,
            stream,
            id,
            body,
        } => {
            #[cfg(feature = "chat")]
            {
                crate::chat::ws_handler::handle_edit(state, ctx, tx, ref_, stream, id, body).await;
            }
            #[cfg(not(feature = "chat"))]
            {
                let _ = (stream, id, body);
                let _ = tx
                    .send(ServerMessage::error(
                        ref_,
                        ErrorCode::BadRequest,
                        "event editing requires the chat feature",
                    ))
                    .await;
            }
        }
        ClientMessage::CursorUpdate { stream, seq } => {
            #[cfg(feature = "chat")]
            {
                crate::chat::ws_handler::handle_cursor_update(state, ctx, stream, seq).await;
            }
            #[cfg(not(feature = "chat"))]
            {
                let _ = (stream, seq);
            }
        }
        ClientMessage::PresenceSet { ref_, status } => {
            #[cfg(feature = "chat")]
            {
                crate::chat::ws_handler::handle_presence_set(state, ctx, tx, ref_, status).await;
            }
            #[cfg(not(feature = "chat"))]
            {
                let _ = status;
                let _ = tx
                    .send(ServerMessage::error(
                        ref_,
                        ErrorCode::BadRequest,
                        "manual presence requires the chat feature",
                    ))
                    .await;
            }
        }
        ClientMessage::TypingStart { stream } => {
            #[cfg(feature = "chat")]
            {
                crate::chat::ws_handler::handle_typing(state, ctx, &stream, true);
            }
            #[cfg(not(feature = "chat"))]
            {
                let _ = stream;
            }
        }
        ClientMessage::TypingStop { stream } => {
            #[cfg(feature = "chat")]
            {
                crate::chat::ws_handler::handle_typing(state, ctx, &stream, false);
            }
            #[cfg(not(feature = "chat"))]
            {
                let _ = stream;
            }
        }
        ClientMessage::EventsFetch {
            ref_,
            stream,
            before,
            limit,
        } => {
            handle_fetch(state, ctx, tx, ref_, stream, before, limit).await;
        }
        ClientMessage::EventDelete { ref_, stream, id } => {
            #[cfg(feature = "chat")]
            {
                crate::chat::ws_handler::handle_delete(state, ctx, tx, ref_, stream, id).await;
            }
            #[cfg(not(feature = "chat"))]
            {
                let _ = (stream, id);
                let _ = tx
                    .send(ServerMessage::error(
                        ref_,
                        ErrorCode::BadRequest,
                        "event deletion requires the chat feature",
                    ))
                    .await;
            }
        }
        ClientMessage::EventTrigger {
            ref_,
            stream,
            event,
            data,
        } => {
            handle_ephemeral_trigger(state, ctx, tx, ref_, stream, event, data).await;
        }
        ClientMessage::ReactionAdd {
            ref_,
            stream,
            event_id,
            emoji,
        } => {
            #[cfg(feature = "chat")]
            {
                crate::chat::ws_handler::handle_reaction(
                    state, ctx, tx, ref_, stream, event_id, emoji, true,
                )
                .await;
            }
            #[cfg(not(feature = "chat"))]
            {
                let _ = (stream, event_id, emoji);
                let _ = tx
                    .send(ServerMessage::error(
                        ref_,
                        ErrorCode::BadRequest,
                        "reactions require the chat feature",
                    ))
                    .await;
            }
        }
        ClientMessage::ReactionRemove {
            ref_,
            stream,
            event_id,
            emoji,
        } => {
            #[cfg(feature = "chat")]
            {
                crate::chat::ws_handler::handle_reaction(
                    state, ctx, tx, ref_, stream, event_id, emoji, false,
                )
                .await;
            }
            #[cfg(not(feature = "chat"))]
            {
                let _ = (stream, event_id, emoji);
                let _ = tx
                    .send(ServerMessage::error(
                        ref_,
                        ErrorCode::BadRequest,
                        "reactions require the chat feature",
                    ))
                    .await;
            }
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
    streams: Vec<String>,
) {
    let tid = &ctx.tenant_id;
    for stream_id in streams {
        if !ctx.streams_claim.contains(&stream_id) {
            let _ = tx
                .send(ServerMessage::error(
                    ref_.clone(),
                    ErrorCode::Unauthorized,
                    format!("not authorized for stream {stream_id}"),
                ))
                .await;
            continue;
        }

        // For public streams, auto-add as member on first subscribe
        if state.streams.is_public(tid, &stream_id) {
            if !state.streams.is_member(tid, &stream_id, &ctx.user_id) {
                state.streams.add_member(tid, &stream_id, &ctx.user_id);
                let member = herald_core::member::Member {
                    stream_id: stream_id.clone(),
                    user_id: ctx.user_id.clone(),
                    role: herald_core::member::Role::Member,
                    joined_at: now_millis(),
                };
                let _ = store::members::insert(&*state.db, tid, &member).await;
            }
        } else if !state.streams.is_member(tid, &stream_id, &ctx.user_id) {
            let _ = tx
                .send(ServerMessage::error(
                    ref_.clone(),
                    ErrorCode::StreamNotFound,
                    format!("not a member of stream {stream_id}"),
                ))
                .await;
            continue;
        }

        state
            .streams
            .subscribe(tid, &stream_id, &ctx.user_id, ctx.conn_id);
        state
            .connections
            .add_stream_subscription(ctx.conn_id, &stream_id);

        let members = state.streams.get_members(tid, &stream_id);
        let mut member_presence = Vec::new();
        for uid in &members {
            if let Ok(Some(member)) = store::members::get(&*state.db, tid, &stream_id, uid).await {
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

        #[cfg(feature = "chat")]
        let cursor = crate::chat::store_cursors::get(&*state.db, tid, &stream_id, &ctx.user_id)
            .await
            .unwrap_or(0);
        #[cfg(not(feature = "chat"))]
        let cursor = store::cursors::get(&*state.db, tid, &stream_id, &ctx.user_id)
            .await
            .unwrap_or(0);
        let latest_seq = state.streams.current_seq(tid, &stream_id);

        let stream_id_ref = stream_id.clone();
        let _ = tx
            .send(ServerMessage::Subscribed {
                ref_: ref_.clone(),
                payload: SubscribedPayload {
                    stream: stream_id,
                    members: member_presence,
                    cursor,
                    latest_seq,
                },
            })
            .await;

        // Broadcast subscriber count update
        let count = state.streams.subscriber_count(tid, &stream_id_ref);
        let count_msg = ServerMessage::StreamSubscriberCount {
            payload: StreamSubscriberCountPayload {
                stream: stream_id_ref.clone(),
                count,
            },
        };
        fanout_to_stream(state, tid, &stream_id_ref, &count_msg, None);

        // Cache channel: deliver last event to new subscriber
        if let Some(cached) = state.streams.get_last_event(tid, &stream_id_ref) {
            let _ = tx.send(cached).await;
        }
    }
}

fn handle_unsubscribe(
    state: &Arc<AppState>,
    ctx: &mut ConnContext,
    _ref_: Option<String>,
    streams: Vec<String>,
) {
    for stream_id in streams {
        state
            .streams
            .unsubscribe(&ctx.tenant_id, &stream_id, &ctx.user_id, ctx.conn_id);
        state
            .connections
            .remove_stream_subscription(ctx.conn_id, &stream_id);

        // Broadcast subscriber count update
        let count = state.streams.subscriber_count(&ctx.tenant_id, &stream_id);
        let count_msg = ServerMessage::StreamSubscriberCount {
            payload: StreamSubscriberCountPayload {
                stream: stream_id.clone(),
                count,
            },
        };
        fanout_to_stream(state, &ctx.tenant_id, &stream_id, &count_msg, None);
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_publish(
    state: &Arc<AppState>,
    ctx: &ConnContext,
    tx: &mpsc::Sender<ServerMessage>,
    ref_: Option<String>,
    stream: String,
    body: String,
    meta: Option<serde_json::Value>,
    parent_id: Option<String>,
) {
    let tid = &ctx.tenant_id;

    if body.len() > 65_536 {
        let _ = tx
            .send(ServerMessage::error(
                ref_,
                ErrorCode::BadRequest,
                "event body exceeds maximum length",
            ))
            .await;
        return;
    }

    // Validate meta size (match HTTP 16KB limit)
    if let Some(ref m) = meta {
        if let Ok(s) = serde_json::to_string(m) {
            if s.len() > 16_384 {
                let _ = tx
                    .send(ServerMessage::error(
                        ref_,
                        ErrorCode::BadRequest,
                        "meta field exceeds maximum size (16384 bytes)",
                    ))
                    .await;
                return;
            }
        }
        if let Some(attachments) = m.get("attachments") {
            if let Some(arr) = attachments.as_array() {
                if arr.len() > 10 {
                    let _ = tx
                        .send(ServerMessage::error(
                            ref_,
                            ErrorCode::BadRequest,
                            "maximum 10 attachments per event",
                        ))
                        .await;
                    return;
                }
                for (i, att) in arr.iter().enumerate() {
                    if att.get("url").and_then(|v| v.as_str()).is_none() {
                        let _ = tx
                            .send(ServerMessage::error(
                                ref_,
                                ErrorCode::BadRequest,
                                format!("attachment[{i}] missing required 'url' field"),
                            ))
                            .await;
                        return;
                    }
                }
            }
        }
    }

    if !state.streams.is_member(tid, &stream, &ctx.user_id) {
        let _ = tx
            .send(ServerMessage::error(
                ref_,
                ErrorCode::NotSubscribed,
                "not a member of this stream",
            ))
            .await;
        return;
    }

    if state.streams.is_archived(tid, &stream) {
        let _ = tx
            .send(ServerMessage::error(
                ref_,
                ErrorCode::BadRequest,
                "stream is archived",
            ))
            .await;
        return;
    }

    if let Err(e) = state
        .authorize(tid, &ctx.user_id, "event.publish", &stream)
        .await
    {
        let _ = tx
            .send(ServerMessage::error(ref_, ErrorCode::Unauthorized, e))
            .await;
        return;
    }

    // Enforce events_per_month limit (only when metering enabled — Meterd defines the limit)
    if let Some(plan_limits) = state.get_plan_limits_cached(tid) {
        let current = state
            .tenant_metrics
            .entry(tid.to_string())
            .or_default()
            .events_published
            .load(Ordering::Relaxed);
        if current >= plan_limits.events_per_month {
            let _ = tx
                .send(ServerMessage::error(
                    ref_,
                    ErrorCode::RateLimited,
                    format!(
                        "monthly event limit reached ({}/{})",
                        current, plan_limits.events_per_month
                    ),
                ))
                .await;
            return;
        }
    }

    // Meterd quota check (remote, with circuit breaker — fails open)
    if let Some(ref metering) = state.metering {
        let result = metering.check_quota("events_published", tid, 1).await;
        if !result.allowed {
            let _ = tx
                .send(ServerMessage::error(
                    ref_,
                    ErrorCode::RateLimited,
                    "event quota exceeded",
                ))
                .await;
            return;
        }
        if result.soft_overage {
            tracing::warn!(
                tenant = tid,
                usage = ?result.current_usage,
                limit = ?result.limit,
                "soft quota overage — event allowed but tenant is over limit"
            );
        }
    }

    let total_start = std::time::Instant::now();

    // Sequence is allocated before storage. If the store write below fails,
    // this seq is consumed but no event exists at it — creating a gap in the
    // sequence. This is accepted: seq-based queries (list_after, list_before)
    // handle gaps correctly, and the gap is harmless to consumers. Avoiding
    // it would require post-write seq assignment which complicates the key
    // construction (event_key needs the seq).
    let seq = state.streams.next_seq(tid, &stream);
    let now = now_millis();
    let event_id = uuid::Uuid::new_v4().to_string();

    let event = Event {
        id: EventId(event_id.clone()),
        stream_id: stream.clone(),
        seq,
        sender: ctx.user_id.clone(),
        body: body.clone(),
        meta: meta.clone(),
        parent_id: parent_id.clone(),
        edited_at: None,
        sent_at: now,
    };

    // Store
    let store_start = std::time::Instant::now();
    if let Err(e) = store::events::insert(&*state.db, tid, &event, now + EVENT_TTL_MS).await {
        tracing::error!("failed to store event: {e}");
        let _ = tx
            .send(ServerMessage::error(
                ref_,
                ErrorCode::Internal,
                "storage error",
            ))
            .await;
        return;
    }
    state.metrics.event_store.observe_since(store_start);

    state
        .metrics
        .events_published
        .fetch_add(1, Ordering::Relaxed);
    state.increment_tenant_events(tid);

    if let Some(ref metering) = state.metering {
        metering.track("events_published", tid, 1.0, None);
    }

    state.event_bus.push_event(
        crate::admin_events::EventKind::Message,
        Some(tid.to_string()),
        serde_json::json!({
            "stream": &stream,
            "sender": &ctx.user_id,
            "event_id": &event_id,
            "seq": seq,
        }),
    );

    let _ = tx
        .send(ServerMessage::EventAck {
            ref_,
            payload: EventAckPayload {
                id: event_id.clone(),
                seq,
                sent_at: now,
            },
        })
        .await;

    // Fan-out
    let fanout_start = std::time::Instant::now();
    let new_event = ServerMessage::EventNew {
        payload: EventNewPayload {
            stream: stream.clone(),
            id: event_id.clone(),
            seq,
            sender: ctx.user_id.clone(),
            body: body.clone(),
            meta: meta.clone(),
            parent_id: parent_id.clone(),
            sent_at: now,
        },
    };
    fanout_to_stream(state, tid, &stream, &new_event, None);
    state.metrics.event_fanout.observe_since(fanout_start);

    // Cache channel: update last event for new subscribers
    state.streams.set_last_event(tid, &stream, new_event);

    state.fire_webhook(
        tid,
        crate::webhook::WebhookEvent {
            event: "event.new".to_string(),
            stream: stream.clone(),
            id: Some(event_id),
            seq: Some(seq),
            sender: Some(ctx.user_id.clone()),
            body: Some(body.clone()),
            meta,
            sent_at: Some(now),
            user_id: None,
            role: None,
        },
    );

    state.audit(tid, "event.publish", &stream, &ctx.user_id, "success");
    state.notify_offline_members(tid, &stream, &ctx.user_id, &body);

    state.metrics.event_total.observe_since(total_start);
}

async fn handle_fetch(
    state: &Arc<AppState>,
    ctx: &ConnContext,
    tx: &mpsc::Sender<ServerMessage>,
    ref_: Option<String>,
    stream: String,
    before: Option<u64>,
    limit: Option<u32>,
) {
    let tid = &ctx.tenant_id;

    if !state.streams.is_member(tid, &stream, &ctx.user_id) {
        let _ = tx
            .send(ServerMessage::error(
                ref_,
                ErrorCode::NotSubscribed,
                "not a member of this stream",
            ))
            .await;
        return;
    }

    let limit = limit.unwrap_or(DEFAULT_FETCH_LIMIT).min(MAX_FETCH_LIMIT);
    let before_seq = before.unwrap_or(i64::MAX as u64);

    let events = store::events::list_before(&*state.db, tid, &stream, before_seq, limit + 1)
        .await
        .unwrap_or_default();

    let has_more = events.len() > limit as usize;
    let events: Vec<EventNewPayload> = events
        .into_iter()
        .take(limit as usize)
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

    let _ = tx
        .send(ServerMessage::EventsBatch {
            ref_,
            payload: EventsBatchPayload {
                stream,
                events,
                has_more,
            },
        })
        .await;
}

async fn handle_ephemeral_trigger(
    state: &Arc<AppState>,
    ctx: &ConnContext,
    tx: &mpsc::Sender<ServerMessage>,
    ref_: Option<String>,
    stream: String,
    event: String,
    data: Option<serde_json::Value>,
) {
    let tid = &ctx.tenant_id;

    // Must be subscribed (member) to trigger ephemeral events
    if !state.streams.is_member(tid, &stream, &ctx.user_id) {
        let _ = tx
            .send(ServerMessage::error(
                ref_,
                ErrorCode::NotSubscribed,
                "not a member of this stream",
            ))
            .await;
        return;
    }

    if state.streams.is_archived(tid, &stream) {
        let _ = tx
            .send(ServerMessage::error(
                ref_,
                ErrorCode::BadRequest,
                "stream is archived",
            ))
            .await;
        return;
    }

    // Fan-out to all subscribers except sender
    let msg = ServerMessage::EventReceived {
        payload: EventReceivedPayload {
            stream: stream.clone(),
            event: event.clone(),
            sender: ctx.user_id.clone(),
            data,
        },
    };
    fanout_to_stream(state, tid, &stream, &msg, Some(ctx.conn_id));

    // Ack to sender (lightweight — no seq/id since ephemeral)
    if let Some(r) = ref_ {
        let _ = tx.send(ServerMessage::Pong { ref_: Some(r) }).await;
    }
}
