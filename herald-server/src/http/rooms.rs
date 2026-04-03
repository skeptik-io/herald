use std::sync::Arc;

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use herald_core::room::{Room, RoomId};

use crate::http::validation;
use crate::http::TenantId;
use crate::state::AppState;
use crate::store;
use crate::ws::connection::now_millis;

#[derive(Deserialize)]
pub struct CreateRoomRequest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub meta: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct UpdateRoomRequest {
    pub name: Option<String>,
    pub meta: Option<serde_json::Value>,
    pub archived: Option<bool>,
}

pub async fn create_room(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Json(req): Json<CreateRoomRequest>,
) -> impl IntoResponse {
    if let Err(e) = validation::validate_id(&req.id, "room id") {
        return (*e).into_response();
    }
    if let Err(e) = validation::validate_name(&req.name, "room name") {
        return (*e).into_response();
    }
    if let Err(e) = validation::validate_meta(&req.meta) {
        return (*e).into_response();
    }

    let tid = &tenant.0;

    // Check for existing room before insert (KV put is upsert)
    if let Ok(Some(_)) = store::rooms::get(&*state.db, tid, &req.id).await {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "failed to create room"})),
        )
            .into_response();
    }

    let room = Room {
        id: RoomId(req.id),
        name: req.name,
        meta: req.meta,
        archived: false,
        created_at: now_millis(),
    };

    if let Err(e) = store::rooms::insert(&*state.db, tid, &room).await {
        tracing::error!(tenant = tid, room = %room.id.as_str(), "failed to create room: {e}");
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "failed to create room"})),
        )
            .into_response();
    }

    state.rooms.create_room(tid, room.id.as_str(), 0);

    state.audit(tid, "room.create", room.id.as_str(), "api", "success");

    (StatusCode::CREATED, Json(room)).into_response()
}

pub async fn list_rooms(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Query(page): Query<crate::http::validation::PaginationQuery>,
) -> impl IntoResponse {
    match store::rooms::list_by_tenant(&*state.db, &tenant.0).await {
        Ok(rooms) => {
            let (limit, offset) = page.resolve();
            let total = rooms.len();
            let rooms: Vec<_> = rooms.into_iter().skip(offset).take(limit).collect();
            Json(serde_json::json!({ "rooms": rooms, "total": total, "limit": limit, "offset": offset })).into_response()
        }
        Err(e) => {
            tracing::error!(tenant = %tenant.0, "failed to list rooms: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
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
        Err(e) => {
            tracing::error!(tenant = %tenant.0, room = %id, "failed to get room: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
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
        req.archived,
    )
    .await
    {
        Ok(true) => {
            if let Some(archived) = req.archived {
                state.rooms.set_archived(&tenant.0, &id, archived);
            }
            StatusCode::OK.into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "room not found"})),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(tenant = %tenant.0, room = %id, "failed to update room: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
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
        Err(e) => {
            tracing::error!(tenant = tid, room = %id, "failed to delete room: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}
