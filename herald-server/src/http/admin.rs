use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

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
