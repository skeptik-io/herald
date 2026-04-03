use std::sync::Arc;

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use herald_core::room::{EncryptionMode, Room, RoomId};

use crate::http::TenantId;
use crate::state::AppState;
use crate::store;
use crate::ws::connection::now_millis;

#[derive(Deserialize)]
pub struct CreateRoomRequest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub encryption_mode: Option<String>,
    #[serde(default)]
    pub meta: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct UpdateRoomRequest {
    pub name: Option<String>,
    pub meta: Option<serde_json::Value>,
}

pub async fn create_room(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Json(req): Json<CreateRoomRequest>,
) -> impl IntoResponse {
    let tid = &tenant.0;
    let enc = req
        .encryption_mode
        .as_deref()
        .and_then(EncryptionMode::from_str_loose)
        .unwrap_or_default();

    let room = Room {
        id: RoomId(req.id),
        name: req.name,
        encryption_mode: enc,
        meta: req.meta,
        created_at: now_millis(),
    };

    if enc == EncryptionMode::ServerEncrypted {
        if let Some(ref cipher) = state.cipher {
            let keyring = format!("herald-{tid}-room-{}", room.id.as_str());
            if let Err(e) = cipher.create_keyring(&keyring).await {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": format!("failed to create keyring: {e}")})),
                )
                    .into_response();
            }
        } else {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "cipher not configured — cannot create encrypted room"})),
            )
                .into_response();
        }
    }

    if let Some(ref veil) = state.veil {
        let index = format!("herald-{tid}-room-{}", room.id.as_str());
        if let Err(e) = veil.create_index(&index).await {
            tracing::warn!("failed to create veil index: {e}");
        }
    }

    if let Err(e) = store::rooms::insert(&*state.db, tid, &room).await {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": format!("failed to create room: {e}")})),
        )
            .into_response();
    }

    state.rooms.create_room(tid, room.id.as_str(), 0, enc);

    state.audit(tid, "room.create", room.id.as_str(), "api", "success");

    (StatusCode::CREATED, Json(room)).into_response()
}

pub async fn list_rooms(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
) -> impl IntoResponse {
    match store::rooms::list_by_tenant(&*state.db, &tenant.0).await {
        Ok(rooms) => Json(serde_json::json!({ "rooms": rooms })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn get_room(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match store::rooms::get(&*state.db, &tenant.0, &id).await {
        Ok(Some(room)) => Json(room).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "room not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn update_room(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path(id): Path<String>,
    Json(req): Json<UpdateRoomRequest>,
) -> impl IntoResponse {
    match store::rooms::update(
        &*state.db,
        &tenant.0,
        &id,
        req.name.as_deref(),
        req.meta.as_ref(),
    )
    .await
    {
        Ok(true) => StatusCode::OK.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "room not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn delete_room(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let tid = &tenant.0;
    match store::rooms::delete(&*state.db, tid, &id).await {
        Ok(true) => {
            let msg = herald_core::protocol::ServerMessage::RoomDeleted {
                payload: herald_core::protocol::RoomEventPayload { room: id.clone() },
            };
            crate::ws::fanout::fanout_to_room(&state, tid, &id, &msg, None);
            state.rooms.remove_room(tid, &id);
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "room not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}
