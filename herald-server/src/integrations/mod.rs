pub mod chronicle;
pub mod cipher;
pub mod circuit_breaker;
pub mod courier;

pub mod embedded_cipher;
pub mod embedded_sentry;
pub mod embedded_veil;
pub mod resilient;
pub mod sentry;
pub mod veil;

use std::fmt;

use base64::{engine::general_purpose::STANDARD as B64, Engine};

// ---------------------------------------------------------------------------
// Cipher
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum CipherError {
    Unavailable,
    Connection(String),
    Operation(String),
}

impl fmt::Display for CipherError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => write!(f, "cipher not configured"),
            Self::Connection(e) => write!(f, "cipher connection error: {e}"),
            Self::Operation(e) => write!(f, "cipher operation error: {e}"),
        }
    }
}

impl std::error::Error for CipherError {}

#[async_trait::async_trait]
pub trait CipherOps: Send + Sync {
    async fn create_keyring(&self, name: &str) -> Result<(), CipherError>;
    async fn encrypt(
        &self,
        keyring: &str,
        plaintext: &str,
        context: Option<&str>,
    ) -> Result<String, CipherError>;
    async fn decrypt(
        &self,
        keyring: &str,
        ciphertext: &str,
        context: Option<&str>,
    ) -> Result<String, CipherError>;
}

#[derive(Default)]
pub struct MockCipherOps {
    keyrings: std::sync::Mutex<std::collections::HashSet<String>>,
}

impl MockCipherOps {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn has_keyring(&self, name: &str) -> bool {
        self.keyrings.lock().unwrap().contains(name)
    }
}

#[async_trait::async_trait]
impl CipherOps for MockCipherOps {
    async fn create_keyring(&self, name: &str) -> Result<(), CipherError> {
        self.keyrings.lock().unwrap().insert(name.to_string());
        Ok(())
    }

    async fn encrypt(
        &self,
        _keyring: &str,
        plaintext: &str,
        _context: Option<&str>,
    ) -> Result<String, CipherError> {
        Ok(format!("enc:{}", B64.encode(plaintext.as_bytes())))
    }

    async fn decrypt(
        &self,
        _keyring: &str,
        ciphertext: &str,
        _context: Option<&str>,
    ) -> Result<String, CipherError> {
        let encoded = ciphertext
            .strip_prefix("enc:")
            .ok_or_else(|| CipherError::Operation("not mock-encrypted".to_string()))?;
        let bytes = B64
            .decode(encoded)
            .map_err(|e| CipherError::Operation(e.to_string()))?;
        String::from_utf8(bytes).map_err(|e| CipherError::Operation(e.to_string()))
    }
}

// ---------------------------------------------------------------------------
// Veil
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum VeilError {
    Unavailable,
    Connection(String),
    Operation(String),
}

impl fmt::Display for VeilError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => write!(f, "veil not configured"),
            Self::Connection(e) => write!(f, "veil connection error: {e}"),
            Self::Operation(e) => write!(f, "veil operation error: {e}"),
        }
    }
}

impl std::error::Error for VeilError {}

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub id: String,
    pub score: f64,
}

#[async_trait::async_trait]
pub trait VeilOps: Send + Sync {
    /// Create a blind index. Idempotent.
    async fn create_index(&self, name: &str) -> Result<(), VeilError>;

    /// Index a message body. `entry_id` should be unique per message (e.g., message ID).
    async fn put(&self, index: &str, entry_id: &str, plaintext: &str) -> Result<(), VeilError>;

    /// Delete an entry from the index.
    async fn delete(&self, index: &str, entry_id: &str) -> Result<(), VeilError>;

    /// Search the index. Returns matching entry IDs with scores.
    async fn search(
        &self,
        index: &str,
        query: &str,
        limit: Option<usize>,
    ) -> Result<Vec<SearchHit>, VeilError>;
}

/// Mock Veil that stores plaintext entries in-memory and does simple substring matching.
#[derive(Default)]
pub struct MockVeilOps {
    indexes: std::sync::Mutex<std::collections::HashMap<String, Vec<(String, String)>>>,
}

impl MockVeilOps {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait::async_trait]
impl VeilOps for MockVeilOps {
    async fn create_index(&self, name: &str) -> Result<(), VeilError> {
        self.indexes
            .lock()
            .unwrap()
            .entry(name.to_string())
            .or_default();
        Ok(())
    }

    async fn put(&self, index: &str, entry_id: &str, plaintext: &str) -> Result<(), VeilError> {
        self.indexes
            .lock()
            .unwrap()
            .entry(index.to_string())
            .or_default()
            .push((entry_id.to_string(), plaintext.to_lowercase()));
        Ok(())
    }

    async fn delete(&self, index: &str, entry_id: &str) -> Result<(), VeilError> {
        if let Some(entries) = self.indexes.lock().unwrap().get_mut(index) {
            entries.retain(|(id, _)| id != entry_id);
        }
        Ok(())
    }

    async fn search(
        &self,
        index: &str,
        query: &str,
        limit: Option<usize>,
    ) -> Result<Vec<SearchHit>, VeilError> {
        let indexes = self.indexes.lock().unwrap();
        let entries = match indexes.get(index) {
            Some(e) => e,
            None => return Ok(vec![]),
        };
        let query_lower = query.to_lowercase();
        let mut results: Vec<SearchHit> = entries
            .iter()
            .filter(|(_, text)| text.contains(&query_lower))
            .map(|(id, _)| SearchHit {
                id: id.clone(),
                score: 1.0,
            })
            .collect();
        if let Some(limit) = limit {
            results.truncate(limit);
        }
        Ok(results)
    }
}

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
    pub resource: String,
    pub actor: String,
    pub result: String,
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub metadata: std::collections::HashMap<String, String>,
}

#[async_trait::async_trait]
pub trait ChronicleOps: Send + Sync {
    /// Ingest an audit event.
    async fn ingest(&self, event: AuditEvent) -> Result<(), ChronicleError>;
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
