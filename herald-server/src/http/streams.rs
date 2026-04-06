use std::sync::Arc;

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use herald_core::stream::{Stream, StreamId};

use crate::http::validation;
use crate::http::TenantId;
use crate::state::AppState;
use crate::store;
use crate::ws::connection::now_millis;

#[derive(Deserialize)]
pub struct CreateStreamRequest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub meta: Option<serde_json::Value>,
    #[serde(default)]
    pub public: Option<bool>,
}

#[derive(Deserialize)]
pub struct UpdateStreamRequest {
    pub name: Option<String>,
    pub meta: Option<serde_json::Value>,
    pub archived: Option<bool>,
}

pub async fn create_stream(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Json(req): Json<CreateStreamRequest>,
) -> impl IntoResponse {
    if let Err(e) = validation::validate_id(&req.id, "stream id") {
        return (*e).into_response();
    }
    if let Err(e) = validation::validate_name(&req.name, "stream name") {
        return (*e).into_response();
    }
    if let Err(e) = validation::validate_meta(&req.meta) {
        return (*e).into_response();
    }

    let tid = &tenant.0;

    // Enforce per-plan stream limit
    let max_streams = state
        .tenant_cache
        .get(tid)
        .and_then(|tc| crate::config::PlanLimits::for_plan(&tc.plan))
        .map(|pl| pl.max_streams)
        .unwrap_or(state.config.tenant_limits.max_streams_per_tenant);
    match store::streams::list_by_tenant(&*state.db, tid).await {
        Ok(existing) if existing.len() >= max_streams as usize => {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"error": format!("stream limit reached ({max_streams})")})),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!(tenant = tid, "failed to count streams: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response();
        }
        _ => {}
    }

    // Check for existing stream before insert (KV put is upsert)
    if let Ok(Some(_)) = store::streams::get(&*state.db, tid, &req.id).await {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "failed to create stream"})),
        )
            .into_response();
    }

    let stream = Stream {
        id: StreamId(req.id),
        name: req.name,
        meta: req.meta,
        archived: false,
        public: req.public.unwrap_or(false),
        created_at: now_millis(),
    };

    if let Err(e) = store::streams::insert(&*state.db, tid, &stream).await {
        tracing::error!(tenant = tid, stream = %stream.id.as_str(), "failed to create stream: {e}");
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "failed to create stream"})),
        )
            .into_response();
    }

    state
        .streams
        .create_stream(tid, stream.id.as_str(), 0, stream.public);

    state.audit(tid, "stream.create", stream.id.as_str(), "api", "success");

    (StatusCode::CREATED, Json(stream)).into_response()
}

pub async fn list_streams(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Query(page): Query<crate::http::validation::PaginationQuery>,
) -> impl IntoResponse {
    match store::streams::list_by_tenant(&*state.db, &tenant.0).await {
        Ok(streams) => {
            let (limit, offset) = page.resolve();
            let total = streams.len();
            let streams: Vec<_> = streams.into_iter().skip(offset).take(limit).collect();
            Json(serde_json::json!({ "streams": streams, "total": total, "limit": limit, "offset": offset })).into_response()
        }
        Err(e) => {
            tracing::error!(tenant = %tenant.0, "failed to list streams: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}

pub async fn get_stream(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match store::streams::get(&*state.db, &tenant.0, &id).await {
        Ok(Some(stream)) => Json(stream).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "stream not found"})),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(tenant = %tenant.0, stream = %id, "failed to get stream: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}

pub async fn update_stream(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path(id): Path<String>,
    Json(req): Json<UpdateStreamRequest>,
) -> impl IntoResponse {
    match store::streams::update(
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
                state.streams.set_archived(&tenant.0, &id, archived);
            }
            StatusCode::OK.into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "stream not found"})),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(tenant = %tenant.0, stream = %id, "failed to update stream: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}

pub async fn delete_stream(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let tid = &tenant.0;
    match store::streams::delete(&*state.db, tid, &id).await {
        Ok(true) => {
            let msg = herald_core::protocol::ServerMessage::StreamDeleted {
                payload: herald_core::protocol::StreamEventPayload { stream: id.clone() },
            };
            crate::ws::fanout::fanout_to_stream(&state, tid, &id, &msg, None);
            state.streams.remove_stream(tid, &id);
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "stream not found"})),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(tenant = tid, stream = %id, "failed to delete stream: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}
