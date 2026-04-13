use serde::Serialize;
use utoipa::ToSchema;

// ---------------------------------------------------------------------------
// Error response (shared by nearly every endpoint)
// ---------------------------------------------------------------------------

#[derive(Serialize, ToSchema)]
pub struct ErrorResponse {
    /// Human-readable error message.
    pub error: String,
}

// ---------------------------------------------------------------------------
// Stream schemas
// ---------------------------------------------------------------------------

#[derive(Serialize, ToSchema)]
pub struct StreamListResponse {
    pub streams: Vec<herald_core::stream::Stream>,
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
}

// ---------------------------------------------------------------------------
// Member schemas
// ---------------------------------------------------------------------------

#[derive(Serialize, ToSchema)]
pub struct MemberListResponse {
    pub members: Vec<herald_core::member::Member>,
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
}

// ---------------------------------------------------------------------------
// Event schemas
// ---------------------------------------------------------------------------

/// Event as returned by the list events API (field names differ from storage).
#[derive(Serialize, ToSchema)]
pub struct EventView {
    pub id: String,
    pub stream: String,
    pub seq: u64,
    pub sender: String,
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edited_at: Option<i64>,
    pub sent_at: i64,
}

#[derive(Serialize, ToSchema)]
pub struct EventListResponse {
    pub events: Vec<EventView>,
    pub has_more: bool,
}

#[derive(Serialize, ToSchema)]
pub struct EventPublishResponse {
    pub id: String,
    pub seq: u64,
    pub sent_at: i64,
}

#[derive(Serialize, ToSchema)]
pub struct PurgeResponse {
    pub deleted: usize,
}

// ---------------------------------------------------------------------------
// Reaction schemas
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Presence schemas
// ---------------------------------------------------------------------------

#[derive(Serialize, ToSchema)]
pub struct UserPresenceResponse {
    pub user_id: String,
    pub status: herald_core::presence::PresenceStatus,
    pub connections: usize,
    /// Unix milliseconds when the user was last seen (last disconnect).
    /// Null when the user is currently online.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen_at: Option<i64>,
}

#[derive(Serialize, ToSchema)]
pub struct StreamPresenceMember {
    pub user_id: String,
    pub status: herald_core::presence::PresenceStatus,
    /// Unix milliseconds when the user was last seen (last disconnect).
    /// Null when the user is currently online.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen_at: Option<i64>,
}

#[derive(Serialize, ToSchema)]
pub struct StreamPresenceResponse {
    pub members: Vec<StreamPresenceMember>,
}

#[derive(Serialize, ToSchema)]
pub struct BatchPresenceUser {
    pub user_id: String,
    pub status: herald_core::presence::PresenceStatus,
    pub connections: usize,
    /// Unix milliseconds when the user was last seen (last disconnect).
    /// Null when the user is currently online.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen_at: Option<i64>,
}

#[derive(Serialize, ToSchema)]
pub struct BatchPresenceResponse {
    pub users: Vec<BatchPresenceUser>,
}

// ---------------------------------------------------------------------------
// Block schemas
// ---------------------------------------------------------------------------

#[derive(Serialize, ToSchema)]
pub struct BlockListResponse {
    pub blocked: Vec<String>,
}

// ---------------------------------------------------------------------------
// Health schemas
// ---------------------------------------------------------------------------

#[derive(Serialize, ToSchema)]
pub struct HealthResponse {
    /// `"ok"` or `"degraded"`.
    pub status: String,
    pub connections: usize,
    pub streams: usize,
    pub tenants: usize,
    pub uptime_secs: u64,
    pub storage: bool,
    pub sentry: bool,
}

#[derive(Serialize, ToSchema)]
pub struct LivenessResponse {
    /// Always `"alive"`.
    pub status: String,
}

#[derive(Serialize, ToSchema)]
pub struct ReadinessResponse {
    /// `"ready"` or `"not_ready"`.
    pub status: String,
    pub storage: bool,
    pub sentry: bool,
}

// ---------------------------------------------------------------------------
// Stats schemas
// ---------------------------------------------------------------------------

#[derive(Serialize, ToSchema)]
pub struct TenantStatsCurrent {
    pub connections: u64,
    pub events_published: u64,
    pub webhooks_sent: u64,
    pub streams: u64,
}

#[derive(Serialize, ToSchema)]
pub struct TenantStatsResponse {
    pub current: TenantStatsCurrent,
    pub snapshots: Vec<serde_json::Value>,
}

#[derive(Serialize, ToSchema)]
pub struct TodaySummary {
    pub peak_connections: u64,
    pub events_today: u64,
    pub webhooks_today: u64,
}

#[derive(Serialize, ToSchema)]
pub struct AdminStatsResponse {
    pub today: TodaySummary,
    pub snapshots: Vec<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Tenant schemas
// ---------------------------------------------------------------------------

#[derive(Serialize, ToSchema)]
pub struct TenantCreateResponse {
    pub id: String,
    pub name: String,
    pub plan: String,
    pub key: String,
    pub secret: String,
    pub created_at: i64,
}

#[derive(Serialize, ToSchema)]
pub struct TenantGetResponse {
    pub id: String,
    pub name: String,
    pub plan: String,
    pub key: String,
    pub config: serde_json::Value,
    pub event_ttl_days: Option<u64>,
    pub created_at: i64,
}

#[derive(Serialize, ToSchema)]
pub struct TenantListItem {
    pub id: String,
    pub name: String,
    pub plan: String,
    pub created_at: i64,
}

#[derive(Serialize, ToSchema)]
pub struct TenantListResponse {
    pub tenants: Vec<TenantListItem>,
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
}

// ---------------------------------------------------------------------------
// Token schemas
// ---------------------------------------------------------------------------

#[derive(Serialize, ToSchema)]
pub struct TokenCreateResponse {
    pub token: String,
    pub scope: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct TokenView {
    pub token: String,
    pub scope: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct TokenListResponse {
    pub tokens: Vec<TokenView>,
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
}

#[derive(Serialize, ToSchema)]
pub struct SelfTokenListResponse {
    pub tokens: Vec<TokenView>,
}

// ---------------------------------------------------------------------------
// Connection schemas
// ---------------------------------------------------------------------------

#[derive(Serialize, ToSchema)]
pub struct TenantConnectionCount {
    pub tenant_id: String,
    pub connections: usize,
}

#[derive(Serialize, ToSchema)]
pub struct ConnectionListResponse {
    pub total: usize,
    pub by_tenant: Vec<TenantConnectionCount>,
}

#[derive(Serialize, ToSchema)]
pub struct SelfConnectionResponse {
    pub tenant_id: String,
    pub connections: usize,
}

// ---------------------------------------------------------------------------
// Admin event / error schemas
// ---------------------------------------------------------------------------

#[derive(Serialize, ToSchema)]
pub struct AdminEventListResponse {
    pub events: Vec<serde_json::Value>,
}

#[derive(Serialize, ToSchema)]
pub struct ErrorListResponse {
    pub errors: Vec<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Secret rotation
// ---------------------------------------------------------------------------

#[derive(Serialize, ToSchema)]
pub struct RotateSecretResponse {
    pub key: String,
    pub secret: String,
}

// ---------------------------------------------------------------------------
// Audit schemas
// ---------------------------------------------------------------------------

#[derive(Serialize, ToSchema)]
pub struct AuditEventView {
    pub id: String,
    pub timestamp: u64,
    pub operation: String,
    pub resource_type: String,
    pub resource_id: String,
    pub actor: String,
    pub result: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<serde_json::Value>,
}

#[derive(Serialize, ToSchema)]
pub struct AuditQueryResponse {
    pub events: Vec<AuditEventView>,
    pub matched: usize,
}

#[derive(Serialize, ToSchema)]
pub struct AuditCountResponse {
    pub count: usize,
}

// ---------------------------------------------------------------------------
// OpenAPI document
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// OpenAPI document
// ---------------------------------------------------------------------------

#[cfg(feature = "openapi")]
#[derive(utoipa::OpenApi)]
#[openapi(
    info(
        title = "Herald API",
        description = "Realtime event transport with built-in authorization. Herald is a delivery layer, not a database — events are retained in an internal catch-up buffer (default 7-day TTL, configurable via `store.event_ttl_days`) for reconnect recovery, then pruned hourly. Requests for events older than the TTL return empty ranges silently. Long-term history, search, and audit belong in your application's own database.",
        version = "2.0.0",
        license(name = "Proprietary"),
    ),
    servers((url = "/", description = "Local server")),
    tags(
        (name = "health", description = "Health and readiness probes"),
        (name = "streams", description = "Stream CRUD operations"),
        (name = "members", description = "Stream membership management"),
        (name = "events", description = "Event publishing and retrieval"),
        (name = "presence", description = "User presence queries"),
        (name = "blocks", description = "User blocking"),
        (name = "admin", description = "Admin API — tenant and system management"),
        (name = "self-service", description = "Tenant self-service operations"),
    ),
    paths(
        crate::http::health::health, crate::http::health::liveness,
        crate::http::health::readiness, crate::http::health::metrics,
        crate::http::health::tenant_stats,
        crate::http::streams::create_stream, crate::http::streams::list_streams,
        crate::http::streams::get_stream, crate::http::streams::update_stream,
        crate::http::streams::delete_stream,
        crate::http::members::add_member, crate::http::members::list_members,
        crate::http::members::remove_member, crate::http::members::update_member,
        crate::http::events::inject_event, crate::http::events::list_events,
        crate::http::events::trigger_ephemeral, crate::http::events::purge_user_events,
        crate::engines::presence::http::user_presence, crate::engines::presence::http::stream_presence,
        crate::engines::presence::http::batch_presence, crate::engines::presence::http::admin_set_presence,
        crate::engines::chat::http_blocks::block_user, crate::engines::chat::http_blocks::unblock_user,
        crate::engines::chat::http_blocks::list_blocked,
        crate::http::admin::create_tenant, crate::http::admin::list_tenants,
        crate::http::admin::get_tenant, crate::http::admin::update_tenant,
        crate::http::admin::delete_tenant, crate::http::admin::purge_tenant_data,
        crate::http::admin::create_api_token, crate::http::admin::list_api_tokens,
        crate::http::admin::delete_api_token, crate::http::admin::list_tenant_streams,
        crate::http::admin::query_audit, crate::http::admin::count_audit,
        crate::http::admin::list_connections, crate::http::admin::list_events,
        crate::http::admin::events_stream, crate::http::admin::list_errors,
        crate::http::admin::get_stats,
        crate::http::self_service::list_connections, crate::http::self_service::list_events,
        crate::http::self_service::events_stream, crate::http::self_service::list_errors,
        crate::http::self_service::rotate_secret, crate::http::self_service::create_token,
        crate::http::self_service::list_tokens, crate::http::self_service::delete_token,
    ),
    components(schemas(
        herald_core::stream::Stream, herald_core::stream::StreamId,
        herald_core::event::Event, herald_core::event::EventId,
        herald_core::member::Member, herald_core::member::Role,
        herald_core::cursor::Cursor, herald_core::presence::PresenceStatus,
        herald_core::error::ErrorCode, herald_core::error::HeraldError,
        crate::http::streams::CreateStreamRequest, crate::http::streams::UpdateStreamRequest,
        crate::http::events::InjectEventRequest, crate::http::events::TriggerEphemeralRequest,
        crate::http::members::AddMemberRequest, crate::http::members::UpdateMemberRequest,
        crate::engines::chat::http_blocks::BlockRequest,
        crate::engines::presence::http::AdminSetPresenceRequest,
        crate::http::admin::CreateTenantRequest, crate::http::admin::UpdateTenantRequest,
        crate::http::admin::CreateTokenRequest, crate::http::self_service::CreateTokenRequest,
        ErrorResponse, StreamListResponse, MemberListResponse,
        EventView, EventListResponse, EventPublishResponse, PurgeResponse,
        UserPresenceResponse, StreamPresenceMember, StreamPresenceResponse,
        BatchPresenceUser, BatchPresenceResponse, BlockListResponse,
        HealthResponse, LivenessResponse, ReadinessResponse,
        TenantStatsCurrent, TenantStatsResponse, AdminStatsResponse, TodaySummary,
        TenantCreateResponse, TenantGetResponse, TenantListItem, TenantListResponse,
        TokenCreateResponse, TokenView, TokenListResponse, SelfTokenListResponse,
        TenantConnectionCount, ConnectionListResponse, SelfConnectionResponse,
        AdminEventListResponse, ErrorListResponse, RotateSecretResponse,
        AuditEventView, AuditQueryResponse, AuditCountResponse,
    )),
    security(("basic_auth" = []), ("bearer_auth" = []), ("admin_auth" = [])),
    modifiers(&SecurityAddon),
)]
#[cfg(feature = "openapi")]
pub struct ApiDoc;

#[cfg(feature = "openapi")]
struct SecurityAddon;

#[cfg(feature = "openapi")]
impl utoipa::Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "basic_auth",
            utoipa::openapi::security::SecurityScheme::Http(utoipa::openapi::security::Http::new(
                utoipa::openapi::security::HttpAuthScheme::Basic,
            )),
        );
        components.add_security_scheme(
            "bearer_auth",
            utoipa::openapi::security::SecurityScheme::Http(utoipa::openapi::security::Http::new(
                utoipa::openapi::security::HttpAuthScheme::Bearer,
            )),
        );
        components.add_security_scheme(
            "admin_auth",
            utoipa::openapi::security::SecurityScheme::Http(utoipa::openapi::security::Http::new(
                utoipa::openapi::security::HttpAuthScheme::Bearer,
            )),
        );
    }
}

/// Generate the OpenAPI JSON spec as a string.
#[cfg(feature = "openapi")]
pub fn generate_json() -> String {
    use utoipa::OpenApi;
    ApiDoc::openapi()
        .to_pretty_json()
        .expect("failed to serialize OpenAPI spec to JSON")
}

/// Generate the OpenAPI YAML spec as a string.
#[cfg(feature = "openapi")]
pub fn generate_yaml() -> String {
    use utoipa::OpenApi;
    ApiDoc::openapi()
        .to_yaml()
        .expect("failed to serialize OpenAPI spec to YAML")
}

#[cfg(all(test, feature = "openapi"))]
mod tests {
    use super::*;
    use utoipa::OpenApi;

    #[test]
    fn spec_generates_valid_json() {
        let json = generate_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("invalid JSON");
        assert_eq!(parsed["openapi"].as_str(), Some("3.1.0"));
        assert_eq!(parsed["info"]["title"].as_str(), Some("Herald API"));
        assert_eq!(parsed["info"]["version"].as_str(), Some("2.0.0"));
    }

    #[test]
    fn spec_has_all_paths() {
        let spec = ApiDoc::openapi();
        let paths = spec.paths.paths;

        // Spot-check key endpoints exist
        assert!(paths.contains_key("/health"), "missing /health");
        assert!(paths.contains_key("/streams"), "missing /streams");
        assert!(
            paths.contains_key("/streams/{id}"),
            "missing /streams/{{id}}"
        );
        assert!(
            paths.contains_key("/streams/{id}/events"),
            "missing /streams/{{id}}/events"
        );
        assert!(
            paths.contains_key("/streams/{id}/members"),
            "missing /streams/{{id}}/members"
        );
        assert!(
            paths.contains_key("/admin/tenants"),
            "missing /admin/tenants"
        );
        assert!(paths.contains_key("/self/tokens"), "missing /self/tokens");
        assert!(
            paths.contains_key("/streams/{id}/events/{event_id}"),
            "missing /streams/{{id}}/events/{{event_id}}"
        );
    }

    #[test]
    fn generate_spec_files() {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let repo_root = std::path::Path::new(&manifest_dir).parent().unwrap();

        let yaml = generate_yaml();
        std::fs::write(repo_root.join("openapi.yaml"), &yaml)
            .expect("failed to write openapi.yaml");

        let json = generate_json();
        std::fs::write(repo_root.join("openapi.json"), &json)
            .expect("failed to write openapi.json");
    }

    #[test]
    fn spec_has_security_schemes() {
        let spec = ApiDoc::openapi();
        let components = spec.components.expect("missing components");
        let schemes = components.security_schemes;
        assert!(
            schemes.contains_key("basic_auth"),
            "missing basic_auth scheme"
        );
        assert!(
            schemes.contains_key("bearer_auth"),
            "missing bearer_auth scheme"
        );
        assert!(
            schemes.contains_key("admin_auth"),
            "missing admin_auth scheme"
        );
    }
}
