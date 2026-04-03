use std::sync::Arc;

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use herald_core::message::{Message, MessageId};
use herald_core::protocol::*;

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

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: String,
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

    if !state.rooms.room_exists(tid, &room_id) {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "room not found"})),
        )
            .into_response();
    }

    let seq = state.rooms.next_seq(tid, &room_id);
    let now = now_millis();
    let msg_id = uuid::Uuid::new_v4().to_string();

    let stored_body = match state.encrypt_body(tid, &room_id, &req.body).await {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("encryption failed: {e}")})),
            )
                .into_response();
        }
    };

    let message = Message {
        id: MessageId(msg_id.clone()),
        room_id: room_id.clone(),
        seq,
        sender: req.sender.clone(),
        body: stored_body,
        meta: req.meta.clone(),
        sent_at: now,
    };

    if let Err(e) = store::messages::insert(&*state.db, tid, &message, now + MESSAGE_TTL_MS).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response();
    }

    state.index_message(tid, &room_id, &msg_id, &req.body).await;
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
    let mut result = Vec::with_capacity(limit as usize);
    for m in messages.into_iter().take(limit as usize) {
        let body = match state.decrypt_body(tid, &room_id, &m.body).await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(id = %m.id.0, "skipping message: decrypt failed: {e}");
                continue;
            }
        };
        result.push(serde_json::json!({
            "id": m.id.0,
            "room": m.room_id,
            "seq": m.seq,
            "sender": m.sender,
            "body": body,
            "meta": m.meta,
            "sent_at": m.sent_at,
        }));
    }

    Json(serde_json::json!({
        "messages": result,
        "has_more": has_more,
    }))
    .into_response()
}

pub async fn search_messages(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path(room_id): Path<String>,
    Query(q): Query<SearchQuery>,
) -> impl IntoResponse {
    let tid = &tenant.0;
    let limit = q.limit.unwrap_or(20).min(MAX_LIMIT);

    let hits = match state
        .search_messages(tid, &room_id, &q.q, Some(limit as usize))
        .await
    {
        Ok(h) => h,
        Err(e) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": e})),
            )
                .into_response();
        }
    };

    let mut messages = Vec::new();
    for hit in &hits {
        if let Ok(Some(msg)) = store::messages::get_by_id(&*state.db, tid, &hit.id).await {
            messages.push(msg);
        }
    }

    let mut result = Vec::new();
    for m in messages {
        let body = match state.decrypt_body(tid, &room_id, &m.body).await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(id = %m.id.0, "skipping search result: decrypt failed: {e}");
                continue;
            }
        };
        result.push(serde_json::json!({
            "id": m.id.0,
            "room": m.room_id,
            "seq": m.seq,
            "sender": m.sender,
            "body": body,
            "meta": m.meta,
            "sent_at": m.sent_at,
        }));
    }

    Json(serde_json::json!({
        "messages": result,
        "has_more": false,
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
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}
