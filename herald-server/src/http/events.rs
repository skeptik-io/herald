use std::sync::Arc;

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use herald_core::event::{Event, EventId};
use herald_core::protocol::*;

use crate::http::validation;
use crate::http::TenantId;
use crate::state::AppState;
use crate::store;
use crate::ws::connection::now_millis;
use crate::ws::fanout::fanout_to_stream;

#[derive(Deserialize)]
pub struct InjectEventRequest {
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
pub struct ListEventsQuery {
    pub before: Option<u64>,
    pub after: Option<u64>,
    pub limit: Option<u32>,
    pub thread: Option<String>,
}

const EVENT_TTL_MS: i64 = 7 * 24 * 60 * 60 * 1000;
const MAX_LIMIT: u32 = 100;

pub async fn inject_event(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path(stream_id): Path<String>,
    Json(req): Json<InjectEventRequest>,
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

    if !state.streams.stream_exists(tid, &stream_id) {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "stream not found"})),
        )
            .into_response();
    }

    if state.streams.is_archived(tid, &stream_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "stream is archived"})),
        )
            .into_response();
    }

    // Meterd quota check (remote, with circuit breaker — fails open)
    let mut quota_warning: Option<String> = None;
    if let Some(ref metering) = state.metering {
        let result = metering.check_quota("events_published", tid, 1).await;
        if !result.allowed {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({"error": "event quota exceeded"})),
            )
                .into_response();
        }
        if result.soft_overage {
            let msg = format!(
                "usage {} of {} (soft cap)",
                result.current_usage.unwrap_or(0),
                result.limit.unwrap_or(0)
            );
            tracing::warn!(
                tenant = tid,
                warning = %msg,
                "soft quota overage — event allowed but tenant is over limit"
            );
            quota_warning = Some(msg);
        }
    }

    // See ws/handler.rs handle_publish for why seq gaps on storage failure are accepted.
    let seq = state.streams.next_seq(tid, &stream_id);
    let now = now_millis();
    let event_id = uuid::Uuid::new_v4().to_string();

    let event = Event {
        id: EventId(event_id.clone()),
        stream_id: stream_id.clone(),
        seq,
        sender: req.sender.clone(),
        body: req.body.clone(),
        meta: req.meta.clone(),
        parent_id: req.parent_id.clone(),
        edited_at: None,
        sent_at: now,
    };

    if let Err(e) = store::events::insert(&*state.db, tid, &event, now + EVENT_TTL_MS).await {
        tracing::error!(tenant = tid, stream = %stream_id, "failed to insert event: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "internal error"})),
        )
            .into_response();
    }

    state.increment_tenant_events(tid);

    if let Some(ref metering) = state.metering {
        metering.track("events_published", tid, 1.0, None);
    }

    let new_event = ServerMessage::EventNew {
        payload: EventNewPayload {
            stream: stream_id.clone(),
            id: event_id.clone(),
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
    fanout_to_stream(
        &state,
        tid,
        &stream_id,
        &new_event,
        exclude,
        Some(&req.sender),
    )
    .await;

    // Cache channel: update last event for new subscribers
    state.streams.set_last_event(tid, &stream_id, new_event);

    state.fire_webhook(
        tid,
        crate::webhook::WebhookEvent {
            event: "event.new".to_string(),
            stream: stream_id.clone(),
            id: Some(event_id.clone()),
            seq: Some(seq),
            sender: Some(req.sender),
            body: Some(req.body),
            meta: req.meta,
            sent_at: Some(now),
            user_id: None,
            role: None,
        },
    );

    let mut response = (
        StatusCode::CREATED,
        Json(serde_json::json!({"id": event_id, "seq": seq, "sent_at": now})),
    )
        .into_response();

    if let Some(warning) = quota_warning {
        if let Ok(val) = warning.parse() {
            response.headers_mut().insert("X-Herald-Quota-Warning", val);
        }
    }

    response
}

pub async fn list_events(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path(stream_id): Path<String>,
    Query(q): Query<ListEventsQuery>,
) -> impl IntoResponse {
    let tid = &tenant.0;
    let limit = q.limit.unwrap_or(50).min(MAX_LIMIT);

    let events = if let Some(after) = q.after {
        store::events::list_after(&*state.db, tid, &stream_id, after, limit + 1)
            .await
            .unwrap_or_default()
    } else {
        let before = q.before.unwrap_or(i64::MAX as u64);
        store::events::list_before(&*state.db, tid, &stream_id, before, limit + 1)
            .await
            .unwrap_or_default()
    };

    let events = if let Some(ref thread_id) = q.thread {
        events
            .into_iter()
            .filter(|m| m.parent_id.as_deref() == Some(thread_id.as_str()))
            .collect()
    } else {
        events
    };

    let has_more = events.len() > limit as usize;
    let result: Vec<serde_json::Value> = events
        .into_iter()
        .take(limit as usize)
        .map(|m| {
            let mut obj = serde_json::json!({
                "id": m.id.0,
                "stream": m.stream_id,
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
        "events": result,
        "has_more": has_more,
    }))
    .into_response()
}

#[cfg(not(feature = "chat"))]
pub async fn list_cursors(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path(stream_id): Path<String>,
) -> impl IntoResponse {
    match store::cursors::list_by_stream(&*state.db, &tenant.0, &stream_id).await {
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
            tracing::error!(tenant = %tenant.0, stream = %stream_id, "failed to list cursors: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}

#[cfg(not(feature = "chat"))]
#[derive(Deserialize)]
pub struct EditEventRequest {
    pub body: String,
}

#[cfg(not(feature = "chat"))]
pub async fn edit_event(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path((stream_id, event_id)): Path<(String, String)>,
    Json(req): Json<EditEventRequest>,
) -> impl IntoResponse {
    let tid = &tenant.0;

    if let Err(e) = validation::validate_body(&req.body) {
        return (*e).into_response();
    }

    let now = now_millis();
    match store::events::edit_event(&*state.db, tid, &event_id, &req.body, now).await {
        Ok(Some(updated)) => {
            let edited_event = ServerMessage::EventEdited {
                payload: herald_core::protocol::EventEditedPayload {
                    stream: stream_id.clone(),
                    id: event_id.clone(),
                    seq: updated.seq,
                    body: req.body,
                    edited_at: now,
                },
            };
            fanout_to_stream(&state, tid, &stream_id, &edited_event, None, None).await;
            StatusCode::OK.into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "event not found"})),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(tenant = tid, event = %event_id, "failed to edit event: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}

#[cfg(not(feature = "chat"))]
pub async fn get_reactions(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path((stream_id, event_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let tid = &tenant.0;
    match store::reactions::list_for_event(&*state.db, tid, &stream_id, &event_id).await {
        Ok(reactions) => Json(serde_json::json!({"reactions": reactions})).into_response(),
        Err(e) => {
            tracing::error!(tenant = tid, event = %event_id, "failed to list reactions: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct TriggerEphemeralRequest {
    pub event: String,
    #[serde(default)]
    pub data: Option<serde_json::Value>,
    #[serde(default)]
    pub exclude_connection: Option<u64>,
}

pub async fn trigger_ephemeral(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path(stream_id): Path<String>,
    Json(req): Json<TriggerEphemeralRequest>,
) -> impl IntoResponse {
    let tid = &tenant.0;

    if !state.streams.stream_exists(tid, &stream_id) {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "stream not found"})),
        )
            .into_response();
    }

    let exclude = req
        .exclude_connection
        .map(crate::registry::connection::ConnId);

    let msg = ServerMessage::EventReceived {
        payload: EventReceivedPayload {
            stream: stream_id.clone(),
            event: req.event.clone(),
            sender: "_server".to_string(),
            data: req.data,
        },
    };
    fanout_to_stream(&state, tid, &stream_id, &msg, exclude, None).await;

    StatusCode::NO_CONTENT.into_response()
}

#[cfg(not(feature = "chat"))]
pub async fn delete_event(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path((stream_id, event_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let tid = &tenant.0;

    match store::events::delete_event(&*state.db, tid, &event_id).await {
        Ok(Some(original)) => {
            let deleted_event = ServerMessage::EventDeleted {
                payload: herald_core::protocol::EventDeletedPayload {
                    stream: stream_id.clone(),
                    id: event_id.clone(),
                    seq: original.seq,
                },
            };
            fanout_to_stream(&state, tid, &stream_id, &deleted_event, None, None).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "event not found"})),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(tenant = tid, stream = %stream_id, event = %event_id, "failed to delete event: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}
