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
    #[serde(default)]
    pub exclude_connection: Option<u64>,
    #[serde(default)]
    pub parent_id: Option<String>,
}

#[derive(Deserialize)]
pub struct ListMessagesQuery {
    pub before: Option<u64>,
    pub after: Option<u64>,
    pub limit: Option<u32>,
    pub thread: Option<String>,
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
    if let Err(e) = validation::validate_attachments(&req.meta) {
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
        parent_id: req.parent_id.clone(),
        edited_at: None,
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
            parent_id: req.parent_id.clone(),
            sent_at: now,
        },
    };
    let exclude = req
        .exclude_connection
        .map(crate::registry::connection::ConnId);
    fanout_to_room(&state, tid, &room_id, &new_msg, exclude);

    // Cache channel: update last event for new subscribers
    state.rooms.set_last_event(tid, &room_id, new_msg);

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

    let messages = if let Some(ref thread_id) = q.thread {
        messages
            .into_iter()
            .filter(|m| m.parent_id.as_deref() == Some(thread_id.as_str()))
            .collect()
    } else {
        messages
    };

    let has_more = messages.len() > limit as usize;
    let result: Vec<serde_json::Value> = messages
        .into_iter()
        .take(limit as usize)
        .map(|m| {
            let mut obj = serde_json::json!({
                "id": m.id.0,
                "room": m.room_id,
                "seq": m.seq,
                "sender": m.sender,
                "body": m.body,
                "meta": m.meta,
                "sent_at": m.sent_at,
            });
            if let Some(ref parent_id) = m.parent_id {
                obj["parent_id"] = serde_json::json!(parent_id);
            }
            if let Some(edited_at) = m.edited_at {
                obj["edited_at"] = serde_json::json!(edited_at);
            }
            obj
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

#[derive(Deserialize)]
pub struct EditMessageRequest {
    pub body: String,
}

pub async fn edit_message(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path((room_id, msg_id)): Path<(String, String)>,
    Json(req): Json<EditMessageRequest>,
) -> impl IntoResponse {
    let tid = &tenant.0;

    if let Err(e) = validation::validate_body(&req.body) {
        return (*e).into_response();
    }

    let now = now_millis();
    match store::messages::edit_message(&*state.db, tid, &msg_id, &req.body, now).await {
        Ok(Some(updated)) => {
            let edited_msg = ServerMessage::MessageEdited {
                payload: herald_core::protocol::MessageEditedPayload {
                    room: room_id.clone(),
                    id: msg_id.clone(),
                    seq: updated.seq,
                    body: req.body,
                    edited_at: now,
                },
            };
            fanout_to_room(&state, tid, &room_id, &edited_msg, None);
            StatusCode::OK.into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "message not found"})),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(tenant = tid, msg = %msg_id, "failed to edit message: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}

pub async fn get_reactions(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path((room_id, msg_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let tid = &tenant.0;
    match store::reactions::list_for_message(&*state.db, tid, &room_id, &msg_id).await {
        Ok(reactions) => Json(serde_json::json!({"reactions": reactions})).into_response(),
        Err(e) => {
            tracing::error!(tenant = tid, msg = %msg_id, "failed to list reactions: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct TriggerEventRequest {
    pub event: String,
    #[serde(default)]
    pub data: Option<serde_json::Value>,
    #[serde(default)]
    pub exclude_connection: Option<u64>,
}

pub async fn trigger_event(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path(room_id): Path<String>,
    Json(req): Json<TriggerEventRequest>,
) -> impl IntoResponse {
    let tid = &tenant.0;

    if !state.rooms.room_exists(tid, &room_id) {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "room not found"})),
        )
            .into_response();
    }

    let exclude = req
        .exclude_connection
        .map(crate::registry::connection::ConnId);

    let msg = ServerMessage::EventReceived {
        payload: EventReceivedPayload {
            room: room_id.clone(),
            event: req.event.clone(),
            sender: "_server".to_string(),
            data: req.data,
        },
    };
    fanout_to_room(&state, tid, &room_id, &msg, exclude);

    StatusCode::NO_CONTENT.into_response()
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
