use std::sync::Arc;

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use herald_core::member::{Member, Role};

use crate::http::TenantId;
use crate::state::AppState;
use crate::store;
use crate::ws::connection::now_millis;
use crate::ws::fanout::fanout_to_room;

#[derive(Deserialize)]
pub struct AddMemberRequest {
    pub user_id: String,
    #[serde(default)]
    pub role: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateMemberRequest {
    pub role: String,
}

pub async fn add_member(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path(room_id): Path<String>,
    Json(req): Json<AddMemberRequest>,
) -> impl IntoResponse {
    let tid = &tenant.0;
    let role = req
        .role
        .as_deref()
        .and_then(Role::from_str_loose)
        .unwrap_or_default();

    let member = Member {
        room_id: room_id.clone(),
        user_id: req.user_id.clone(),
        role,
        joined_at: now_millis(),
    };

    if let Err(e) = store::members::insert(&*state.db, tid, &member).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response();
    }

    state.rooms.add_member(tid, &room_id, &req.user_id);

    let msg = herald_core::protocol::ServerMessage::MemberJoined {
        payload: herald_core::protocol::MemberPayload {
            room: room_id.clone(),
            user_id: req.user_id.clone(),
            role,
        },
    };
    fanout_to_room(&state, tid, &room_id, &msg, None);

    state.fire_webhook(crate::webhook::WebhookEvent {
        event: "member.joined".to_string(),
        room: room_id,
        id: None,
        seq: None,
        sender: None,
        body: None,
        meta: None,
        sent_at: None,
        user_id: Some(req.user_id),
        role: Some(role.as_str().to_string()),
    });

    (StatusCode::CREATED, Json(member)).into_response()
}

pub async fn list_members(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path(room_id): Path<String>,
) -> impl IntoResponse {
    match store::members::list_by_room(&*state.db, &tenant.0, &room_id).await {
        Ok(members) => Json(serde_json::json!({"members": members})).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn remove_member(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path((room_id, user_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let tid = &tenant.0;
    match store::members::delete(&*state.db, tid, &room_id, &user_id).await {
        Ok(true) => {
            state.rooms.remove_member(tid, &room_id, &user_id);

            let msg = herald_core::protocol::ServerMessage::MemberLeft {
                payload: herald_core::protocol::MemberPayload {
                    room: room_id.clone(),
                    user_id: user_id.clone(),
                    role: Role::Member,
                },
            };
            fanout_to_room(&state, tid, &room_id, &msg, None);

            state.fire_webhook(crate::webhook::WebhookEvent {
                event: "member.left".to_string(),
                room: room_id,
                id: None,
                seq: None,
                sender: None,
                body: None,
                meta: None,
                sent_at: None,
                user_id: Some(user_id),
                role: None,
            });

            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "member not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn update_member(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path((room_id, user_id)): Path<(String, String)>,
    Json(req): Json<UpdateMemberRequest>,
) -> impl IntoResponse {
    let role = match Role::from_str_loose(&req.role) {
        Some(r) => r,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "invalid role"})),
            )
                .into_response();
        }
    };

    match store::members::update_role(&*state.db, &tenant.0, &room_id, &user_id, role).await {
        Ok(true) => StatusCode::OK.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "member not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}
