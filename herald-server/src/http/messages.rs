use std::sync::Arc;

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use herald_core::message::{Message, MessageId};
use herald_core::protocol::*;

use crate::http::validation;
use crate::http::TenantId;
use crate::state::AppState;
use crate::store;
use crate::ws::connection::now_millis;
use crate::ws::fanout::fanout_to_room;

#[derive(Deserialize)]
pub struct InjectMessageRequest {
    pub sender: String,
    pub body: String,
    #[serde(default)]
    pub meta: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct ListMessagesQuery {
    pub before: Option<u64>,
    pub after: Option<u64>,
    pub limit: Option<u32>,
}

const MESSAGE_TTL_MS: i64 = 7 * 24 * 60 * 60 * 1000;
const MAX_LIMIT: u32 = 100;

pub async fn inject_message(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path(room_id): Path<String>,
    Json(req): Json<InjectMessageRequest>,
) -> impl IntoResponse {
    let tid = &tenant.0;

    if let Err(e) = validation::validate_id(&req.sender, "sender") {
        return (*e).into_response();
    }
    if let Err(e) = validation::validate_body(&req.body) {
        return (*e).into_response();
    }
    if let Err(e) = validation::validate_meta(&req.meta) {
        return (*e).into_response();
    }

    if !state.rooms.room_exists(tid, &room_id) {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "room not found"})),
        )
            .into_response();
    }

    if state.rooms.is_archived(tid, &room_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "room is archived"})),
        )
            .into_response();
    }

    let seq = state.rooms.next_seq(tid, &room_id);
    let now = now_millis();
    let msg_id = uuid::Uuid::new_v4().to_string();

    let message = Message {
        id: MessageId(msg_id.clone()),
        room_id: room_id.clone(),
        seq,
        sender: req.sender.clone(),
        body: req.body.clone(),
        meta: req.meta.clone(),
        sent_at: now,
    };

    if let Err(e) = store::messages::insert(&*state.db, tid, &message, now + MESSAGE_TTL_MS).await {
        tracing::error!(tenant = tid, room = %room_id, "failed to insert message: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "internal error"})),
        )
            .into_response();
    }

    state.increment_tenant_messages(tid);

    let new_msg = ServerMessage::MessageNew {
        payload: MessageNewPayload {
            room: room_id.clone(),
            id: msg_id.clone(),
            seq,
            sender: req.sender.clone(),
            body: req.body.clone(),
            meta: req.meta.clone(),
            sent_at: now,
        },
    };
    fanout_to_room(&state, tid, &room_id, &new_msg, None);

    state.fire_webhook(
        tid,
        crate::webhook::WebhookEvent {
            event: "message.new".to_string(),
            room: room_id.clone(),
            id: Some(msg_id.clone()),
            seq: Some(seq),
            sender: Some(req.sender),
            body: Some(req.body),
            meta: req.meta,
            sent_at: Some(now),
            user_id: None,
            role: None,
        },
    );

    (
        StatusCode::CREATED,
        Json(serde_json::json!({"id": msg_id, "seq": seq, "sent_at": now})),
    )
        .into_response()
}

pub async fn list_messages(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path(room_id): Path<String>,
    Query(q): Query<ListMessagesQuery>,
) -> impl IntoResponse {
    let tid = &tenant.0;
    let limit = q.limit.unwrap_or(50).min(MAX_LIMIT);

    let messages = if let Some(after) = q.after {
        store::messages::list_after(&*state.db, tid, &room_id, after, limit + 1)
            .await
            .unwrap_or_default()
    } else {
        let before = q.before.unwrap_or(i64::MAX as u64);
        store::messages::list_before(&*state.db, tid, &room_id, before, limit + 1)
            .await
            .unwrap_or_default()
    };

    let has_more = messages.len() > limit as usize;
    let result: Vec<serde_json::Value> = messages
        .into_iter()
        .take(limit as usize)
        .map(|m| {
            serde_json::json!({
                "id": m.id.0,
                "room": m.room_id,
                "seq": m.seq,
                "sender": m.sender,
                "body": m.body,
                "meta": m.meta,
                "sent_at": m.sent_at,
            })
        })
        .collect();

    Json(serde_json::json!({
        "messages": result,
        "has_more": has_more,
    }))
    .into_response()
}

pub async fn list_cursors(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path(room_id): Path<String>,
) -> impl IntoResponse {
    match store::cursors::list_by_room(&*state.db, &tenant.0, &room_id).await {
        Ok(cursors) => {
            let cursors: Vec<serde_json::Value> = cursors
                .into_iter()
                .map(|c| {
                    serde_json::json!({
                        "user_id": c.user_id,
                        "seq": c.seq,
                    })
                })
                .collect();
            Json(serde_json::json!({"cursors": cursors})).into_response()
        }
        Err(e) => {
            tracing::error!(tenant = %tenant.0, room = %room_id, "failed to list cursors: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}

pub async fn delete_message(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path((room_id, msg_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let tid = &tenant.0;

    match store::messages::delete_message(&*state.db, tid, &msg_id).await {
        Ok(Some(original)) => {
            let deleted_msg = ServerMessage::MessageDeleted {
                payload: herald_core::protocol::MessageDeletedPayload {
                    room: room_id.clone(),
                    id: msg_id.clone(),
                    seq: original.seq,
                },
            };
            fanout_to_room(&state, tid, &room_id, &deleted_msg, None);
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "message not found"})),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(tenant = tid, room = %room_id, msg = %msg_id, "failed to delete message: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}
