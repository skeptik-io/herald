pub mod chronicle;
pub mod circuit_breaker;
pub mod courier;

pub mod in_process_chronicle;
pub mod in_process_courier;
pub mod in_process_sentry;
pub mod resilient;
pub mod sentry;

use std::fmt;

// ---------------------------------------------------------------------------
// Sentry (Authorization)
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum SentryError {
    Connection(String),
    Denied(String),
    Operation(String),
}

impl fmt::Display for SentryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connection(e) => write!(f, "sentry connection error: {e}"),
            Self::Denied(reason) => write!(f, "access denied: {reason}"),
            Self::Operation(e) => write!(f, "sentry operation error: {e}"),
        }
    }
}

impl std::error::Error for SentryError {}

#[async_trait::async_trait]
pub trait SentryOps: Send + Sync {
    /// Evaluate an authorization request. Returns Ok(()) if permitted, Err if denied.
    async fn evaluate(
        &self,
        subject: &str,
        action: &str,
        resource: &str,
    ) -> Result<(), SentryError>;
}

/// Mock Sentry that always permits.
#[derive(Default)]
pub struct MockSentryOps;

#[async_trait::async_trait]
impl SentryOps for MockSentryOps {
    async fn evaluate(
        &self,
        _subject: &str,
        _action: &str,
        _resource: &str,
    ) -> Result<(), SentryError> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Courier (Offline Notifications)
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum CourierError {
    Connection(String),
    Operation(String),
}

impl fmt::Display for CourierError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connection(e) => write!(f, "courier connection error: {e}"),
            Self::Operation(e) => write!(f, "courier operation error: {e}"),
        }
    }
}

impl std::error::Error for CourierError {}

#[async_trait::async_trait]
pub trait CourierOps: Send + Sync {
    /// Deliver a notification to an offline user.
    async fn notify(
        &self,
        channel: &str,
        recipient: &str,
        subject: &str,
        body: &str,
    ) -> Result<(), CourierError>;
}

/// Mock Courier that records deliveries for test assertions.
#[derive(Default)]
pub struct MockCourierOps {
    deliveries: std::sync::Mutex<Vec<(String, String, String)>>,
}

impl MockCourierOps {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn deliveries(&self) -> Vec<(String, String, String)> {
        self.deliveries.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl CourierOps for MockCourierOps {
    async fn notify(
        &self,
        _channel: &str,
        recipient: &str,
        subject: &str,
        body: &str,
    ) -> Result<(), CourierError> {
        self.deliveries.lock().unwrap().push((
            recipient.to_string(),
            subject.to_string(),
            body.to_string(),
        ));
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Chronicle (Audit)
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum ChronicleError {
    Connection(String),
    Operation(String),
}

impl fmt::Display for ChronicleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connection(e) => write!(f, "chronicle connection error: {e}"),
            Self::Operation(e) => write!(f, "chronicle operation error: {e}"),
        }
    }
}

impl std::error::Error for ChronicleError {}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AuditEvent {
    pub operation: String,
    pub resource_type: String,
    pub resource_id: String,
    pub actor: String,
    pub result: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub metadata: std::collections::HashMap<String, String>,
}

#[async_trait::async_trait]
pub trait ChronicleOps: Send + Sync {
    /// Ingest an audit event.
    async fn ingest(&self, event: AuditEvent) -> Result<(), ChronicleError>;

    /// Query audit events matching a filter. Returns None if query is not supported
    /// (e.g. remote TCP mode — only in-process engines support query).
    async fn query(&self, filter: &AuditQuery) -> Option<AuditQueryResult> {
        let _ = filter;
        None
    }

    /// Count audit events matching a filter.
    async fn count(&self, filter: &AuditQuery) -> Option<AuditCountResult> {
        let _ = filter;
        None
    }
}

/// Query parameters for audit log filtering.
#[derive(Debug, Clone, Default, serde::Deserialize, utoipa::IntoParams)]
pub struct AuditQuery {
    /// Tenant ID — always set by the HTTP handler from the URL path.
    #[serde(default)]
    #[param(ignore)]
    pub tenant_id: Option<String>,
    pub resource_type: Option<String>,
    pub resource_id: Option<String>,
    pub actor: Option<String>,
    pub operation: Option<String>,
    pub result: Option<String>,
    pub since: Option<u64>,
    pub until: Option<u64>,
    pub limit: Option<usize>,
}

/// Query result with matching audit events.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AuditQueryResult {
    pub events: Vec<AuditEventView>,
    pub matched: usize,
}

/// Count result.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AuditCountResult {
    pub count: usize,
}

/// A serializable view of an audit event for API responses.
#[derive(Debug, Clone, serde::Serialize)]
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
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub metadata: std::collections::HashMap<String, String>,
}

/// Mock Chronicle that records events for test assertions.
#[derive(Default)]
pub struct MockChronicleOps {
    events: std::sync::Mutex<Vec<AuditEvent>>,
}

impl MockChronicleOps {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn events(&self) -> Vec<AuditEvent> {
        self.events.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl ChronicleOps for MockChronicleOps {
    async fn ingest(&self, event: AuditEvent) -> Result<(), ChronicleError> {
        self.events.lock().unwrap().push(event);
        Ok(())
    }
}
