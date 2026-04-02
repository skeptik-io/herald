use std::sync::Arc;

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::Json;

use crate::state::AppState;

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
