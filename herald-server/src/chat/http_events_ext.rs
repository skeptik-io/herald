use std::sync::Arc;

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use herald_core::protocol::*;

use crate::http::validation;
use crate::http::TenantId;
use crate::state::AppState;
use crate::ws::connection::now_millis;
use crate::ws::fanout::fanout_to_stream;

#[derive(Deserialize)]
pub struct EditEventRequest {
    pub body: String,
}

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
    match crate::store::events::edit_event(&*state.db, tid, &event_id, &req.body, now).await {
        Ok(Some(updated)) => {
            let edited_event = ServerMessage::EventEdited {
                payload: EventEditedPayload {
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

pub async fn delete_event(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path((stream_id, event_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let tid = &tenant.0;

    match crate::store::events::delete_event(&*state.db, tid, &event_id).await {
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

pub async fn get_reactions(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path((stream_id, event_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let tid = &tenant.0;
    match crate::chat::store_reactions::list_for_event(&*state.db, tid, &stream_id, &event_id).await
    {
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

pub async fn list_cursors(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path(stream_id): Path<String>,
) -> impl IntoResponse {
    match crate::chat::store_cursors::list_by_stream(&*state.db, &tenant.0, &stream_id).await {
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
