use std::sync::Arc;

use tokio::sync::mpsc;

use herald_core::error::ErrorCode;
use herald_core::protocol::*;

use crate::state::AppState;
use crate::store;
use crate::ws::connection::{now_millis, ConnContext};
use crate::ws::fanout::fanout_to_stream;

pub async fn handle_edit(
    state: &Arc<AppState>,
    ctx: &ConnContext,
    tx: &mpsc::Sender<ServerMessage>,
    ref_: Option<String>,
    stream: String,
    id: String,
    body: String,
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

    // Look up original to check sender
    let event = match store::events::get_by_id(&*state.db, tid, &id).await {
        Ok(Some(m)) => m,
        Ok(None) => {
            let _ = tx
                .send(ServerMessage::error(
                    ref_,
                    ErrorCode::BadRequest,
                    "event not found",
                ))
                .await;
            return;
        }
        Err(e) => {
            tracing::error!("failed to look up event {id}: {e}");
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

    // Only sender can edit (or Sentry authorize)
    if event.sender != ctx.user_id {
        if let Err(e) = state
            .authorize(tid, &ctx.user_id, "event.edit", &stream)
            .await
        {
            let _ = tx
                .send(ServerMessage::error(ref_, ErrorCode::Unauthorized, e))
                .await;
            return;
        }
    }

    let now = now_millis();
    match store::events::edit_event(&*state.db, tid, &id, &body, now).await {
        Ok(Some(updated)) => {
            let edited_event = ServerMessage::EventEdited {
                payload: EventEditedPayload {
                    stream: stream.clone(),
                    id: id.clone(),
                    seq: updated.seq,
                    body: body.clone(),
                    edited_at: now,
                },
            };
            fanout_to_stream(state, tid, &stream, &edited_event, None);

            let _ = tx
                .send(ServerMessage::EventAck {
                    ref_,
                    payload: EventAckPayload {
                        id,
                        seq: updated.seq,
                        sent_at: now,
                    },
                })
                .await;
        }
        Ok(None) => {
            let _ = tx
                .send(ServerMessage::error(
                    ref_,
                    ErrorCode::BadRequest,
                    "event not found",
                ))
                .await;
        }
        Err(e) => {
            tracing::error!("failed to edit event {id}: {e}");
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

pub async fn handle_delete(
    state: &Arc<AppState>,
    ctx: &ConnContext,
    tx: &mpsc::Sender<ServerMessage>,
    ref_: Option<String>,
    stream: String,
    id: String,
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

    // Look up the event to verify ownership
    let event = match store::events::get_by_id(&*state.db, tid, &id).await {
        Ok(Some(m)) => m,
        Ok(None) => {
            let _ = tx
                .send(ServerMessage::error(
                    ref_,
                    ErrorCode::BadRequest,
                    "event not found",
                ))
                .await;
            return;
        }
        Err(e) => {
            tracing::error!("failed to look up event {id}: {e}");
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

    // Only the sender can delete their own event (admin/owner enforcement could be added via Sentry)
    if event.sender != ctx.user_id {
        if let Err(e) = state
            .authorize(tid, &ctx.user_id, "event.delete", &stream)
            .await
        {
            let _ = tx
                .send(ServerMessage::error(ref_, ErrorCode::Unauthorized, e))
                .await;
            return;
        }
    }

    match store::events::delete_event(&*state.db, tid, &id).await {
        Ok(Some(original)) => {
            let deleted_event = ServerMessage::EventDeleted {
                payload: EventDeletedPayload {
                    stream: stream.clone(),
                    id: id.clone(),
                    seq: original.seq,
                },
            };
            fanout_to_stream(state, tid, &stream, &deleted_event, None);

            // Ack to sender
            let _ = tx
                .send(ServerMessage::EventAck {
                    ref_,
                    payload: EventAckPayload {
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
                    "event not found",
                ))
                .await;
        }
        Err(e) => {
            tracing::error!("failed to delete event {id}: {e}");
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

pub async fn handle_cursor_update(
    state: &Arc<AppState>,
    ctx: &ConnContext,
    stream: String,
    seq: u64,
) {
    let now = now_millis();
    let _ = crate::chat::store_cursors::upsert(
        &*state.db,
        &ctx.tenant_id,
        &stream,
        &ctx.user_id,
        seq,
        now,
    )
    .await;

    let msg = ServerMessage::CursorMoved {
        payload: CursorMovedPayload {
            stream: stream.clone(),
            user_id: ctx.user_id.clone(),
            seq,
        },
    };
    fanout_to_stream(state, &ctx.tenant_id, &stream, &msg, Some(ctx.conn_id));
}

pub async fn handle_presence_set(
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
    for stream_id in state
        .streams
        .get_member_streams(&ctx.tenant_id, &ctx.user_id)
    {
        fanout_to_stream(state, &ctx.tenant_id, &stream_id, &msg, None);
    }
}

pub fn handle_typing(state: &Arc<AppState>, ctx: &ConnContext, stream: &str, active: bool) {
    state
        .typing
        .set_typing(&ctx.tenant_id, stream, &ctx.user_id, active);
    let msg = ServerMessage::Typing {
        payload: TypingPayload {
            stream: stream.to_string(),
            user_id: ctx.user_id.clone(),
            active,
        },
    };
    fanout_to_stream(state, &ctx.tenant_id, stream, &msg, Some(ctx.conn_id));
}

#[allow(clippy::too_many_arguments)]
pub async fn handle_reaction(
    state: &Arc<AppState>,
    ctx: &ConnContext,
    tx: &mpsc::Sender<ServerMessage>,
    ref_: Option<String>,
    stream: String,
    event_id: String,
    emoji: String,
    add: bool,
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
    if emoji.len() > 32 {
        let _ = tx
            .send(ServerMessage::error(
                ref_,
                ErrorCode::BadRequest,
                "emoji too long",
            ))
            .await;
        return;
    }

    let result = if add {
        crate::chat::store_reactions::add(&*state.db, tid, &stream, &event_id, &emoji, &ctx.user_id)
            .await
    } else {
        crate::chat::store_reactions::remove(
            &*state.db,
            tid,
            &stream,
            &event_id,
            &emoji,
            &ctx.user_id,
        )
        .await
        .map(|_| ())
    };

    match result {
        Ok(()) => {
            let msg = ServerMessage::ReactionChanged {
                payload: ReactionChangedPayload {
                    stream: stream.clone(),
                    event_id,
                    emoji,
                    user_id: ctx.user_id.clone(),
                    action: if add {
                        "add".to_string()
                    } else {
                        "remove".to_string()
                    },
                },
            };
            fanout_to_stream(state, tid, &stream, &msg, None);
            if let Some(r) = ref_ {
                let _ = tx.send(ServerMessage::Pong { ref_: Some(r) }).await;
            }
        }
        Err(e) => {
            tracing::error!("reaction error: {e}");
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
