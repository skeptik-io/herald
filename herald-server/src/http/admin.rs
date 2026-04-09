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
use crate::http::validation;
use crate::integrations::AuditQuery;
use crate::state::{AppState, TenantConfig};
use crate::store;
use crate::store::tenants::Tenant;
use crate::ws::connection::now_millis;

#[derive(Deserialize)]
pub struct CreateTenantRequest {
    pub name: String,
    #[serde(default)]
    pub plan: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateTenantRequest {
    pub name: Option<String>,
    pub plan: Option<String>,
    pub config: Option<serde_json::Value>,
    pub event_ttl_days: Option<u32>,
}

pub async fn create_tenant(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateTenantRequest>,
) -> impl IntoResponse {
    if let Err(e) = validation::validate_name(&req.name, "tenant name") {
        return (*e).into_response();
    }

    let id = uuid::Uuid::new_v4().to_string();
    let (key, secret) = store::tenants::generate_tenant_credentials();
    let tenant = Tenant {
        id,
        name: req.name,
        key: key.clone(),
        secret: secret.clone(),
        plan: req.plan.unwrap_or_else(|| "free".to_string()),
        config: serde_json::json!({}),
        created_at: now_millis(),
    };

    if let Err(e) = store::tenants::insert(&*state.db, &tenant).await {
        tracing::error!(tenant = %tenant.id, "failed to create tenant: {e}");
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "failed to create tenant"})),
        )
            .into_response();
    }

    // Add to caches
    state
        .key_cache
        .insert(tenant.key.clone(), tenant.id.clone());
    state.tenant_cache.insert(
        tenant.id.clone(),
        TenantConfig {
            id: tenant.id.clone(),
            key: tenant.key.clone(),
            secret: tenant.secret.clone(),
            plan: tenant.plan.clone(),
            event_ttl_days: None,
            cached_at: std::time::Instant::now(),
        },
    );

    state.audit(
        &tenant.id,
        "tenant.create",
        "tenant",
        &tenant.id,
        "admin",
        "success",
    );

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id": tenant.id,
            "name": tenant.name,
            "plan": tenant.plan,
            "key": key,
            "secret": secret,
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
            "key": t.key,
            "config": t.config,
            "event_ttl_days": t.config.get("event_ttl_days").and_then(|v| v.as_u64()),
            "created_at": t.created_at,
        }))
        .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "tenant not found"})),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(tenant = %id, "failed to get tenant: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}

pub async fn list_tenants(
    State(state): State<Arc<AppState>>,
    Query(page): Query<crate::http::validation::PaginationQuery>,
) -> impl IntoResponse {
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
            let (limit, offset) = page.resolve();
            let total = list.len();
            let list: Vec<_> = list.into_iter().skip(offset).take(limit).collect();
            Json(serde_json::json!({"tenants": list, "total": total, "limit": limit, "offset": offset})).into_response()
        }
        Err(e) => {
            tracing::error!("failed to list tenants: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}

pub async fn update_tenant(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateTenantRequest>,
) -> impl IntoResponse {
    // Merge event_ttl_days into config JSON if provided
    let config = if let Some(ttl) = req.event_ttl_days {
        let mut cfg = req.config.unwrap_or_else(|| {
            // Start from existing config if no new config provided
            state
                .tenant_cache
                .get(&id)
                .map(|_| serde_json::json!({}))
                .unwrap_or_else(|| serde_json::json!({}))
        });
        // Read existing config from DB to preserve other fields
        if let Ok(Some(existing)) = store::tenants::get(&*state.db, &id).await {
            if let serde_json::Value::Object(ref existing_map) = existing.config {
                if let serde_json::Value::Object(ref mut new_map) = cfg {
                    for (k, v) in existing_map {
                        new_map.entry(k.clone()).or_insert(v.clone());
                    }
                }
            }
        }
        if let serde_json::Value::Object(ref mut map) = cfg {
            map.insert("event_ttl_days".to_string(), serde_json::json!(ttl));
        }
        Some(cfg)
    } else {
        req.config
    };

    match store::tenants::update(
        &*state.db,
        &id,
        req.name.as_deref(),
        req.plan.as_deref(),
        config.as_ref(),
    )
    .await
    {
        Ok(true) => {
            // Refresh cache from DB to pick up all changes
            if let Ok(Some(t)) = store::tenants::get(&*state.db, &id).await {
                state.tenant_cache.insert(
                    id.clone(),
                    TenantConfig {
                        id: t.id,
                        key: t.key,
                        secret: t.secret,
                        plan: t.plan,
                        event_ttl_days: t
                            .config
                            .get("event_ttl_days")
                            .and_then(|v| v.as_u64())
                            .map(|v| v as u32),
                        cached_at: std::time::Instant::now(),
                    },
                );
            }
            state.audit(&id, "tenant.update", "tenant", &id, "admin", "success");
            StatusCode::OK.into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "tenant not found"})),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(tenant = %id, "failed to update tenant: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}

pub async fn delete_tenant(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    // Get tenant key before deleting so we can clean up caches
    let tenant_key = state.tenant_cache.get(&id).map(|tc| tc.key.clone());
    match store::tenants::delete(&*state.db, &id, tenant_key.as_deref()).await {
        Ok(true) => {
            if let Some(ref key) = tenant_key {
                state.key_cache.remove(key);
            }
            state.tenant_cache.remove(&id);
            state.audit(&id, "tenant.delete", "tenant", &id, "admin", "success");
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "tenant not found"})),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(tenant = %id, "failed to delete tenant: {e}");
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

pub async fn create_api_token(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
    body: Option<Json<CreateTokenRequest>>,
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
            tracing::error!(tenant = %tenant_id, "failed to verify tenant for token creation: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response();
        }
    }

    let scope = body.and_then(|b| b.scope.clone());
    let token = uuid::Uuid::new_v4().to_string();
    if let Err(e) =
        store::tenants::create_token(&*state.db, &token, &tenant_id, scope.as_deref()).await
    {
        tracing::error!(tenant = %tenant_id, "failed to create api token: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "internal error"})),
        )
            .into_response();
    }

    state.audit(
        &tenant_id,
        "token.create",
        "token",
        &token,
        "admin",
        "success",
    );

    (
        StatusCode::CREATED,
        Json(serde_json::json!({"token": token, "scope": scope})),
    )
        .into_response()
}

pub async fn list_api_tokens(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
    Query(page): Query<crate::http::validation::PaginationQuery>,
) -> impl IntoResponse {
    match store::tenants::list_tokens(&*state.db, &tenant_id).await {
        Ok(tokens) => {
            let (limit, offset) = page.resolve();
            let total = tokens.len();
            let tokens: Vec<_> = tokens.into_iter().skip(offset).take(limit).collect();
            Json(serde_json::json!({"tokens": tokens, "total": total, "limit": limit, "offset": offset})).into_response()
        }
        Err(e) => {
            tracing::error!(tenant = %tenant_id, "failed to list api tokens: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}

pub async fn delete_api_token(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, token)): Path<(String, String)>,
) -> impl IntoResponse {
    match store::tenants::delete_token(&*state.db, &token, &tenant_id).await {
        Ok(true) => {
            state.audit(
                &tenant_id,
                "token.revoke",
                "token",
                &token,
                "admin",
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
            tracing::error!(tenant = %tenant_id, token = %token, "failed to delete api token: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
    }
}

/// List streams for a specific tenant (admin view).
pub async fn list_tenant_streams(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
    Query(page): Query<crate::http::validation::PaginationQuery>,
) -> impl IntoResponse {
    match store::streams::list_by_tenant(&*state.db, &tenant_id).await {
        Ok(streams) => {
            let (limit, offset) = page.resolve();
            let total = streams.len();
            let streams: Vec<_> = streams.into_iter().skip(offset).take(limit).collect();
            Json(serde_json::json!({"streams": streams, "total": total, "limit": limit, "offset": offset})).into_response()
        }
        Err(e) => {
            tracing::error!(tenant = %tenant_id, "failed to list tenant streams: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response()
        }
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
    let events_total = state
        .metrics
        .events_published
        .load(std::sync::atomic::Ordering::Relaxed);
    let today = state.event_bus.today_summary(events_total);

    Json(serde_json::json!({
        "today": today,
        "snapshots": snapshots,
    }))
}

/// GDPR: Purge ALL data for a tenant from all namespaces.
pub async fn purge_tenant_data(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    // Verify tenant exists
    match store::tenants::get(&*state.db, &id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "tenant not found"})),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!(tenant = %id, "purge: failed to look up tenant: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal error"})),
            )
                .into_response();
        }
    }

    // Remove from in-memory caches
    if let Some((_, tc)) = state.tenant_cache.remove(&id) {
        state.key_cache.remove(&tc.key);
    }
    state.tenant_metrics.remove(&id);

    match store::purge::purge_tenant(&*state.db, &id).await {
        Ok(deleted) => {
            tracing::info!(tenant = %id, deleted, "GDPR: tenant data purged");
            state.audit(&id, "tenant.purge", "tenant", &id, "admin", "success");
            Json(serde_json::json!({"deleted": deleted})).into_response()
        }
        Err(e) => {
            tracing::error!(tenant = %id, "purge failed: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "purge failed"})),
            )
                .into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// Audit log query
// ---------------------------------------------------------------------------

pub async fn query_audit(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(mut query): Query<AuditQuery>,
) -> impl IntoResponse {
    let Some(ref chronicle) = state.chronicle else {
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(serde_json::json!({"error": "audit logging not configured"})),
        )
            .into_response();
    };

    // Scope query to this tenant
    query.tenant_id = Some(id.clone());

    match chronicle.query(&query).await {
        Some(result) => Json(result).into_response(),
        None => (
            StatusCode::NOT_IMPLEMENTED,
            Json(serde_json::json!({"error": "audit query not supported in remote mode"})),
        )
            .into_response(),
    }
}

pub async fn count_audit(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(mut query): Query<AuditQuery>,
) -> impl IntoResponse {
    let Some(ref chronicle) = state.chronicle else {
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(serde_json::json!({"error": "audit logging not configured"})),
        )
            .into_response();
    };

    query.tenant_id = Some(id.clone());

    match chronicle.count(&query).await {
        Some(result) => Json(result).into_response(),
        None => (
            StatusCode::NOT_IMPLEMENTED,
            Json(serde_json::json!({"error": "audit count not supported in remote mode"})),
        )
            .into_response(),
    }
}
