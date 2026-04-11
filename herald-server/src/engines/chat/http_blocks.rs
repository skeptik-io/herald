use std::sync::Arc;

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use crate::http::TenantId;
use crate::state::AppState;

#[derive(Deserialize, utoipa::ToSchema)]
pub struct BlockRequest {
    pub user_id: String,
    pub blocked_id: String,
}

#[utoipa::path(
    post, path = "/blocks",
    tag = "blocks",
    security(("basic_auth" = []), ("bearer_auth" = [])),
    request_body = BlockRequest,
    responses(
        (status = 201, description = "User blocked"),
        (status = 400, description = "Validation error", body = crate::http::openapi::ErrorResponse),
    ),
)]
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
    match crate::store::blocks::block(&*state.db, tid, &req.user_id, &req.blocked_id).await {
        Ok(()) => {
            state.audit(
                tid,
                "user.block",
                "user",
                &req.blocked_id,
                &req.user_id,
                "success",
            );
            StatusCode::CREATED.into_response()
        }
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

#[utoipa::path(
    delete, path = "/blocks",
    tag = "blocks",
    security(("basic_auth" = []), ("bearer_auth" = [])),
    request_body = BlockRequest,
    responses(
        (status = 204, description = "User unblocked"),
        (status = 400, description = "Validation error", body = crate::http::openapi::ErrorResponse),
        (status = 404, description = "Block not found", body = crate::http::openapi::ErrorResponse),
    ),
)]
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
    match crate::store::blocks::unblock(&*state.db, tid, &req.user_id, &req.blocked_id).await {
        Ok(true) => {
            state.audit(
                tid,
                "user.unblock",
                "user",
                &req.blocked_id,
                &req.user_id,
                "success",
            );
            StatusCode::NO_CONTENT.into_response()
        }
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

#[utoipa::path(
    get, path = "/blocks/{user_id}",
    tag = "blocks",
    params(("user_id" = String, Path, description = "User ID")),
    security(("basic_auth" = []), ("bearer_auth" = [])),
    responses(
        (status = 200, description = "List of blocked users", body = crate::http::openapi::BlockListResponse),
    ),
)]
pub async fn list_blocked(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path(user_id): Path<String>,
) -> impl IntoResponse {
    let tid = &tenant.0;
    match crate::store::blocks::list_blocked(&*state.db, tid, &user_id).await {
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
