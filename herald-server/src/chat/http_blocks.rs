use std::sync::Arc;

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use crate::http::TenantId;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct BlockRequest {
    pub user_id: String,
    pub blocked_id: String,
}

pub async fn block_user(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Json(req): Json<BlockRequest>,
) -> impl IntoResponse {
    let tid = &tenant.0;
    if let Err(e) = crate::http::validation::validate_id(&req.user_id, "user_id") {
        return (*e).into_response();
    }
    if let Err(e) = crate::http::validation::validate_id(&req.blocked_id, "blocked_id") {
        return (*e).into_response();
    }
    match crate::chat::store_blocks::block(&*state.db, tid, &req.user_id, &req.blocked_id).await {
        Ok(()) => StatusCode::CREATED.into_response(),
        Err(e) => {
            tracing::error!(tenant = tid, "block failed: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}

pub async fn unblock_user(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Json(req): Json<BlockRequest>,
) -> impl IntoResponse {
    let tid = &tenant.0;
    if let Err(e) = crate::http::validation::validate_id(&req.user_id, "user_id") {
        return (*e).into_response();
    }
    if let Err(e) = crate::http::validation::validate_id(&req.blocked_id, "blocked_id") {
        return (*e).into_response();
    }
    match crate::chat::store_blocks::unblock(&*state.db, tid, &req.user_id, &req.blocked_id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "block not found"})),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(tenant = tid, "unblock failed: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}

pub async fn list_blocked(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path(user_id): Path<String>,
) -> impl IntoResponse {
    let tid = &tenant.0;
    match crate::chat::store_blocks::list_blocked(&*state.db, tid, &user_id).await {
        Ok(blocked) => Json(serde_json::json!({"blocked": blocked})).into_response(),
        Err(e) => {
            tracing::error!(tenant = tid, "list blocked failed: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}
