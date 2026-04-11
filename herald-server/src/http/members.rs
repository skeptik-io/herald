use std::sync::Arc;

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use herald_core::member::{Member, Role};

use crate::http::validation;
use crate::http::TenantId;
use crate::state::AppState;
use crate::store;
use crate::ws::connection::now_millis;
use crate::ws::fanout::fanout_to_stream;

#[derive(Deserialize, utoipa::ToSchema)]
pub struct AddMemberRequest {
    pub user_id: String,
    #[serde(default)]
    pub role: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct UpdateMemberRequest {
    pub role: String,
}

#[utoipa::path(
    post, path = "/streams/{id}/members",
    tag = "members",
    params(("id" = String, Path, description = "Stream ID")),
    security(("basic_auth" = []), ("bearer_auth" = [])),
    request_body = AddMemberRequest,
    responses(
        (status = 201, description = "Member added", body = herald_core::member::Member),
        (status = 400, description = "Validation error", body = crate::http::openapi::ErrorResponse),
    ),
)]
pub async fn add_member(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path(stream_id): Path<String>,
    Json(req): Json<AddMemberRequest>,
) -> impl IntoResponse {
    let tid = &tenant.0;

    if let Err(e) = validation::validate_id(&req.user_id, "user_id") {
        return (*e).into_response();
    }

    let role = req
        .role
        .as_deref()
        .and_then(Role::from_str_loose)
        .unwrap_or_default();

    let member = Member {
        stream_id: stream_id.clone(),
        user_id: req.user_id.clone(),
        role,
        joined_at: now_millis(),
    };

    if let Err(e) = store::members::insert(&*state.db, tid, &member).await {
        tracing::error!(tenant = tid, stream = %stream_id, user = %req.user_id, "failed to add member: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "internal error"})),
        )
            .into_response();
    }

    state.streams.add_member(tid, &stream_id, &req.user_id);

    let msg = herald_core::protocol::ServerMessage::MemberJoined {
        payload: herald_core::protocol::MemberPayload {
            stream: stream_id.clone(),
            user_id: req.user_id.clone(),
            role,
        },
    };
    fanout_to_stream(&state, tid, &stream_id, &msg, None, None).await;

    state.fire_webhook(
        tid,
        crate::webhook::WebhookEvent {
            event: "member.joined".to_string(),
            stream: stream_id,
            id: None,
            seq: None,
            sender: None,
            body: None,
            meta: None,
            sent_at: None,
            user_id: Some(req.user_id),
            role: Some(role.as_str().to_string()),
        },
    );

    state.audit(
        tid,
        "member.add",
        "stream",
        &member.stream_id,
        "api",
        "success",
    );

    (StatusCode::CREATED, Json(member)).into_response()
}

#[utoipa::path(
    get, path = "/streams/{id}/members",
    tag = "members",
    params(("id" = String, Path, description = "Stream ID"), crate::http::validation::PaginationQuery),
    security(("basic_auth" = []), ("bearer_auth" = [])),
    responses(
        (status = 200, description = "List of members", body = crate::http::openapi::MemberListResponse),
    ),
)]
pub async fn list_members(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path(stream_id): Path<String>,
    Query(page): Query<crate::http::validation::PaginationQuery>,
) -> impl IntoResponse {
    match store::members::list_by_stream(&*state.db, &tenant.0, &stream_id).await {
        Ok(members) => {
            let (limit, offset) = page.resolve();
            let total = members.len();
            let members: Vec<_> = members.into_iter().skip(offset).take(limit).collect();
            Json(serde_json::json!({"members": members, "total": total, "limit": limit, "offset": offset})).into_response()
        }
        Err(e) => {
            tracing::error!(tenant = %tenant.0, stream = %stream_id, "failed to list members: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}

#[utoipa::path(
    delete, path = "/streams/{id}/members/{user_id}",
    tag = "members",
    params(
        ("id" = String, Path, description = "Stream ID"),
        ("user_id" = String, Path, description = "User ID"),
    ),
    security(("basic_auth" = []), ("bearer_auth" = [])),
    responses(
        (status = 204, description = "Member removed"),
        (status = 404, description = "Member not found", body = crate::http::openapi::ErrorResponse),
    ),
)]
pub async fn remove_member(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path((stream_id, user_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let tid = &tenant.0;
    match store::members::delete(&*state.db, tid, &stream_id, &user_id).await {
        Ok(true) => {
            state.streams.remove_member(tid, &stream_id, &user_id);

            let msg = herald_core::protocol::ServerMessage::MemberLeft {
                payload: herald_core::protocol::MemberPayload {
                    stream: stream_id.clone(),
                    user_id: user_id.clone(),
                    role: Role::Member,
                },
            };
            fanout_to_stream(&state, tid, &stream_id, &msg, None, None).await;

            state.audit(tid, "member.remove", "stream", &stream_id, "api", "success");

            state.fire_webhook(
                tid,
                crate::webhook::WebhookEvent {
                    event: "member.left".to_string(),
                    stream: stream_id,
                    id: None,
                    seq: None,
                    sender: None,
                    body: None,
                    meta: None,
                    sent_at: None,
                    user_id: Some(user_id),
                    role: None,
                },
            );

            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "member not found"})),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(tenant = tid, stream = %stream_id, user = %user_id, "failed to remove member: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}

#[utoipa::path(
    patch, path = "/streams/{id}/members/{user_id}",
    tag = "members",
    params(
        ("id" = String, Path, description = "Stream ID"),
        ("user_id" = String, Path, description = "User ID"),
    ),
    security(("basic_auth" = []), ("bearer_auth" = [])),
    request_body = UpdateMemberRequest,
    responses(
        (status = 200, description = "Member role updated"),
        (status = 400, description = "Invalid role", body = crate::http::openapi::ErrorResponse),
        (status = 404, description = "Member not found", body = crate::http::openapi::ErrorResponse),
    ),
)]
pub async fn update_member(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path((stream_id, user_id)): Path<(String, String)>,
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

    match store::members::update_role(&*state.db, &tenant.0, &stream_id, &user_id, role).await {
        Ok(true) => {
            state.audit(
                &tenant.0,
                "member.update",
                "stream",
                &stream_id,
                "api",
                "success",
            );
            StatusCode::OK.into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "member not found"})),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(tenant = %tenant.0, stream = %stream_id, user = %user_id, "failed to update member role: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}
