use std::sync::Arc;

use axum::extract::{Extension, Query, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use crate::http::TenantId;
use crate::state::AppState;
use crate::ws::connection::now_millis;

pub async fn health(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let storage_health = state.db.engine().health();
    let storage_ok = matches!(storage_health, shroudb_storage::engine::HealthState::Ready);

    let status = if storage_ok { "ok" } else { "degraded" };

    let checks = serde_json::json!({
        "status": status,
        "connections": state.connections.total_connections(),
        "rooms": state.rooms.room_count(),
        "tenants": state.tenant_cache.len(),
        "uptime_secs": state.start_time.elapsed().as_secs(),
        "storage": storage_ok,
        "cipher": state.cipher.is_some(),
        "veil": state.veil.is_some(),
        "sentry": state.sentry.is_some(),
    });

    let http_status = if storage_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (http_status, Json(checks))
}

pub async fn metrics(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let body = state.metrics.format_prometheus(&state);
    (
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
}

#[derive(Deserialize)]
pub struct TenantStatsQuery {
    pub from: Option<i64>,
    pub to: Option<i64>,
}

/// Tenant-scoped stats endpoint. Returns snapshots for the authenticated tenant.
pub async fn tenant_stats(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Query(q): Query<TenantStatsQuery>,
) -> impl IntoResponse {
    let tid = &tenant.0;
    let now = now_millis();
    let from = q.from.unwrap_or(now - 86_400_000);
    let to = q.to.unwrap_or(now);

    let snapshots = state.event_bus.get_tenant_snapshots(tid, from, to);

    let connections = state.connections.tenant_connection_count(tid) as u64;
    let messages = state.tenant_messages_sent(tid);
    let webhooks = state.tenant_webhooks_sent(tid);
    let rooms = state.rooms.tenant_room_count(tid) as u64;

    Json(serde_json::json!({
        "current": {
            "connections": connections,
            "messages_sent": messages,
            "webhooks_sent": webhooks,
            "rooms": rooms,
        },
        "snapshots": snapshots,
    }))
}
