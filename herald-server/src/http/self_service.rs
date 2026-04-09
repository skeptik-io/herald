use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::Json;
use futures_util::stream::Stream;
use serde::Deserialize;

use crate::admin_events::ErrorCategory;
use crate::http::TenantId;
use crate::state::AppState;
use crate::store;
use axum::Extension;

/// GET /self/connections — tenant's active connection count.
pub async fn list_connections(
    State(state): State<Arc<AppState>>,
    Extension(tenant_id): Extension<TenantId>,
) -> impl IntoResponse {
    let count = state.connections.tenant_connection_count(&tenant_id.0);
    Json(serde_json::json!({
        "tenant_id": tenant_id.0,
        "connections": count,
    }))
}

#[derive(Deserialize)]
pub struct EventsQuery {
    pub limit: Option<usize>,
    pub after_id: Option<u64>,
}

/// GET /self/events — tenant's recent admin events.
pub async fn list_events(
    State(state): State<Arc<AppState>>,
    Extension(tenant_id): Extension<TenantId>,
    Query(q): Query<EventsQuery>,
) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(100).min(500);
    let events = state
        .event_bus
        .recent_events_for_tenant(&tenant_id.0, limit, q.after_id);
    Json(serde_json::json!({"events": events}))
}

/// GET /self/events/stream — SSE stream of tenant's events.
pub async fn events_stream(
    State(state): State<Arc<AppState>>,
    Extension(tenant_id): Extension<TenantId>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.event_bus.subscribe();
    let tid = tenant_id.0;

    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if event.tenant_id.as_deref() == Some(tid.as_str()) {
                        if let Ok(json) = serde_json::to_string(&event) {
                            yield Ok(Event::default().data(json));
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[derive(Deserialize)]
pub struct ErrorsQuery {
    pub category: Option<String>,
    pub limit: Option<usize>,
}

/// GET /self/errors — tenant's recent errors.
pub async fn list_errors(
    State(state): State<Arc<AppState>>,
    Extension(tenant_id): Extension<TenantId>,
    Query(q): Query<ErrorsQuery>,
) -> impl IntoResponse {
    let category = q.category.as_deref().and_then(|c| match c {
        "client" => Some(ErrorCategory::Client),
        "webhook" => Some(ErrorCategory::Webhook),
        "http" => Some(ErrorCategory::Http),
        _ => None,
    });
    let limit = q.limit.unwrap_or(100).min(500);
    let errors = state
        .event_bus
        .recent_errors_for_tenant(&tenant_id.0, category, limit);
    Json(serde_json::json!({"errors": errors}))
}

/// POST /self/secret/rotate — rotate the tenant's secret, return new credentials.
pub async fn rotate_secret(
    State(state): State<Arc<AppState>>,
    Extension(tenant_id): Extension<TenantId>,
) -> impl IntoResponse {
    match store::tenants::rotate_secret(&*state.db, &tenant_id.0).await {
        Ok(Some((key, new_secret))) => {
            // Update caches
            if let Some(mut tc) = state.tenant_cache.get_mut(&tenant_id.0) {
                tc.secret = new_secret.clone();
                tc.cached_at = std::time::Instant::now();
            }

            state.audit(
                &tenant_id.0,
                "tenant.secret_rotate",
                "tenant",
                &tenant_id.0,
                "tenant",
                "success",
            );

            Json(serde_json::json!({
                "key": key,
                "secret": new_secret,
            }))
            .into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "tenant not found"})),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(tenant = %tenant_id.0, "secret rotation failed: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct CreateTokenRequest {
    #[serde(default)]
    pub scope: Option<String>,
}

/// POST /self/tokens — create an API token for the authenticated tenant.
pub async fn create_token(
    State(state): State<Arc<AppState>>,
    Extension(tenant_id): Extension<TenantId>,
    Json(req): Json<CreateTokenRequest>,
) -> impl IntoResponse {
    let token = uuid::Uuid::new_v4().to_string();
    match store::tenants::create_token(&*state.db, &token, &tenant_id.0, req.scope.as_deref()).await
    {
        Ok(()) => {
            state.audit(
                &tenant_id.0,
                "token.create",
                "token",
                &token,
                "tenant",
                "success",
            );
            (
                StatusCode::CREATED,
                Json(serde_json::json!({"token": token, "scope": req.scope})),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!(tenant = %tenant_id.0, "create token failed: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}

/// GET /self/tokens — list API tokens for the authenticated tenant.
pub async fn list_tokens(
    State(state): State<Arc<AppState>>,
    Extension(tenant_id): Extension<TenantId>,
) -> impl IntoResponse {
    match store::tenants::list_tokens(&*state.db, &tenant_id.0).await {
        Ok(tokens) => Json(serde_json::json!({"tokens": tokens})).into_response(),
        Err(e) => {
            tracing::error!(tenant = %tenant_id.0, "list tokens failed: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}

/// DELETE /self/tokens/{token} — revoke an API token for the authenticated tenant.
pub async fn delete_token(
    State(state): State<Arc<AppState>>,
    Extension(tenant_id): Extension<TenantId>,
    Path(token): Path<String>,
) -> impl IntoResponse {
    match store::tenants::delete_token(&*state.db, &token, &tenant_id.0).await {
        Ok(true) => {
            state.audit(
                &tenant_id.0,
                "token.revoke",
                "token",
                &token,
                "tenant",
                "success",
            );
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "token not found"})),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(tenant = %tenant_id.0, "delete token failed: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}
