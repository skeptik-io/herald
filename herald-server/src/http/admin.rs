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
use crate::state::{AppState, TenantConfig};
use crate::store;
use crate::store::tenants::Tenant;
use crate::ws::connection::now_millis;

#[derive(Deserialize)]
pub struct CreateTenantRequest {
    pub id: String,
    pub name: String,
    pub jwt_secret: String,
    #[serde(default)]
    pub jwt_issuer: Option<String>,
    #[serde(default)]
    pub plan: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateTenantRequest {
    pub name: Option<String>,
    pub plan: Option<String>,
    pub config: Option<serde_json::Value>,
}

pub async fn create_tenant(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateTenantRequest>,
) -> impl IntoResponse {
    let tenant = Tenant {
        id: req.id,
        name: req.name,
        jwt_secret: req.jwt_secret,
        jwt_issuer: req.jwt_issuer,
        plan: req.plan.unwrap_or_else(|| "free".to_string()),
        config: serde_json::json!({}),
        created_at: now_millis(),
    };

    if let Err(e) = store::tenants::insert(&*state.db, &tenant).await {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": format!("failed to create tenant: {e}")})),
        )
            .into_response();
    }

    // Add to cache
    state.tenant_cache.insert(
        tenant.id.clone(),
        TenantConfig {
            id: tenant.id.clone(),
            jwt_secret: tenant.jwt_secret.clone(),
            jwt_issuer: tenant.jwt_issuer.clone(),
            plan: tenant.plan.clone(),
        },
    );

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id": tenant.id,
            "name": tenant.name,
            "plan": tenant.plan,
            "created_at": tenant.created_at,
        })),
    )
        .into_response()
}

pub async fn get_tenant(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match store::tenants::get(&*state.db, &id).await {
        Ok(Some(t)) => Json(serde_json::json!({
            "id": t.id,
            "name": t.name,
            "plan": t.plan,
            "jwt_issuer": t.jwt_issuer,
            "config": t.config,
            "created_at": t.created_at,
        }))
        .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "tenant not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn list_tenants(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match store::tenants::list(&*state.db).await {
        Ok(tenants) => {
            let list: Vec<serde_json::Value> = tenants
                .into_iter()
                .map(|t| {
                    serde_json::json!({
                        "id": t.id,
                        "name": t.name,
                        "plan": t.plan,
                        "created_at": t.created_at,
                    })
                })
                .collect();
            Json(serde_json::json!({"tenants": list})).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn update_tenant(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateTenantRequest>,
) -> impl IntoResponse {
    match store::tenants::update(
        &*state.db,
        &id,
        req.name.as_deref(),
        req.plan.as_deref(),
        req.config.as_ref(),
    )
    .await
    {
        Ok(true) => {
            // Update cache
            if let Some(mut cached) = state.tenant_cache.get_mut(&id) {
                if let Some(ref plan) = req.plan {
                    cached.plan = plan.clone();
                }
            }
            StatusCode::OK.into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "tenant not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn delete_tenant(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match store::tenants::delete(&*state.db, &id).await {
        Ok(true) => {
            state.tenant_cache.remove(&id);
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "tenant not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn create_api_token(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
) -> impl IntoResponse {
    // Verify tenant exists
    match store::tenants::get(&*state.db, &tenant_id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "tenant not found"})),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    }

    let token = uuid::Uuid::new_v4().to_string();
    if let Err(e) = store::tenants::create_token(&*state.db, &token, &tenant_id).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response();
    }

    (
        StatusCode::CREATED,
        Json(serde_json::json!({"token": token})),
    )
        .into_response()
}

pub async fn list_api_tokens(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
) -> impl IntoResponse {
    match store::tenants::list_tokens(&*state.db, &tenant_id).await {
        Ok(tokens) => Json(serde_json::json!({"tokens": tokens})).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn delete_api_token(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, token)): Path<(String, String)>,
) -> impl IntoResponse {
    match store::tenants::delete_token(&*state.db, &token, &tenant_id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "token not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// List rooms for a specific tenant (admin view).
pub async fn list_tenant_rooms(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
) -> impl IntoResponse {
    match store::rooms::list_by_tenant(&*state.db, &tenant_id).await {
        Ok(rooms) => Json(serde_json::json!({"rooms": rooms})).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// List active connections.
pub async fn list_connections(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let mut connections = Vec::new();
    let total = state.connections.total_connections();

    // Iterate tenants from cache and get per-tenant counts
    for entry in state.tenant_cache.iter() {
        let tid = entry.key();
        let count = state.connections.tenant_connection_count(tid);
        if count > 0 {
            connections.push(serde_json::json!({
                "tenant_id": tid,
                "connections": count,
            }));
        }
    }

    Json(serde_json::json!({
        "total": total,
        "by_tenant": connections,
    }))
}

#[derive(Deserialize)]
pub struct EventsQuery {
    pub limit: Option<usize>,
    pub after_id: Option<u64>,
}

/// List recent admin events.
pub async fn list_events(
    State(state): State<Arc<AppState>>,
    Query(q): Query<EventsQuery>,
) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(100).min(500);
    let events = state.event_bus.recent_events(limit, q.after_id);
    Json(serde_json::json!({"events": events}))
}

/// SSE stream of admin events.
pub async fn events_stream(
    State(state): State<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.event_bus.subscribe();

    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if let Ok(json) = serde_json::to_string(&event) {
                        yield Ok(Event::default().data(json));
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

/// List recent errors.
pub async fn list_errors(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ErrorsQuery>,
) -> impl IntoResponse {
    let category = q.category.as_deref().and_then(|c| match c {
        "client" => Some(ErrorCategory::Client),
        "webhook" => Some(ErrorCategory::Webhook),
        "http" => Some(ErrorCategory::Http),
        _ => None,
    });
    let limit = q.limit.unwrap_or(100).min(500);
    let errors = state.event_bus.recent_errors(category, limit);
    Json(serde_json::json!({"errors": errors}))
}

#[derive(Deserialize)]
pub struct StatsQuery {
    pub from: Option<i64>,
    pub to: Option<i64>,
}

/// Get stats snapshots and today's summary.
pub async fn get_stats(
    State(state): State<Arc<AppState>>,
    Query(q): Query<StatsQuery>,
) -> impl IntoResponse {
    let now = now_millis();
    let from = q.from.unwrap_or(now - 86_400_000); // default: last 24h
    let to = q.to.unwrap_or(now);
    let snapshots = state.event_bus.get_snapshots(from, to);
    let messages_total = state
        .metrics
        .messages_sent
        .load(std::sync::atomic::Ordering::Relaxed);
    let today = state.event_bus.today_summary(messages_total);

    Json(serde_json::json!({
        "today": today,
        "snapshots": snapshots,
    }))
}
