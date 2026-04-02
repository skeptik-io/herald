use std::sync::Arc;
use std::time::Duration;

use super::circuit_breaker::CircuitBreaker;
use super::*;

const DEFAULT_FAILURE_THRESHOLD: u32 = 5;
const DEFAULT_COOLDOWN: Duration = Duration::from_secs(30);
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Wraps a CipherOps with circuit breaker + timeout.
pub struct ResilientCipher {
    inner: Arc<dyn CipherOps>,
    cb: CircuitBreaker,
    timeout: Duration,
}

impl ResilientCipher {
    pub fn new(inner: Arc<dyn CipherOps>) -> Self {
        Self {
            inner,
            cb: CircuitBreaker::new("cipher", DEFAULT_FAILURE_THRESHOLD, DEFAULT_COOLDOWN),
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

#[async_trait::async_trait]
impl CipherOps for ResilientCipher {
    async fn create_keyring(&self, name: &str) -> Result<(), CipherError> {
        self.cb.check().map_err(CipherError::Connection)?;
        match tokio::time::timeout(self.timeout, self.inner.create_keyring(name)).await {
            Ok(Ok(v)) => {
                self.cb.record_success();
                Ok(v)
            }
            Ok(Err(e)) => {
                self.cb.record_failure();
                Err(e)
            }
            Err(_) => {
                self.cb.record_failure();
                Err(CipherError::Connection("request timeout".to_string()))
            }
        }
    }

    async fn encrypt(
        &self,
        keyring: &str,
        plaintext: &str,
        context: Option<&str>,
    ) -> Result<String, CipherError> {
        self.cb.check().map_err(CipherError::Connection)?;
        match tokio::time::timeout(
            self.timeout,
            self.inner.encrypt(keyring, plaintext, context),
        )
        .await
        {
            Ok(Ok(v)) => {
                self.cb.record_success();
                Ok(v)
            }
            Ok(Err(e)) => {
                self.cb.record_failure();
                Err(e)
            }
            Err(_) => {
                self.cb.record_failure();
                Err(CipherError::Connection("request timeout".to_string()))
            }
        }
    }

    async fn decrypt(
        &self,
        keyring: &str,
        ciphertext: &str,
        context: Option<&str>,
    ) -> Result<String, CipherError> {
        self.cb.check().map_err(CipherError::Connection)?;
        match tokio::time::timeout(
            self.timeout,
            self.inner.decrypt(keyring, ciphertext, context),
        )
        .await
        {
            Ok(Ok(v)) => {
                self.cb.record_success();
                Ok(v)
            }
            Ok(Err(e)) => {
                self.cb.record_failure();
                Err(e)
            }
            Err(_) => {
                self.cb.record_failure();
                Err(CipherError::Connection("request timeout".to_string()))
            }
        }
    }
}

/// Wraps a VeilOps with circuit breaker + timeout.
pub struct ResilientVeil {
    inner: Arc<dyn VeilOps>,
    cb: CircuitBreaker,
    timeout: Duration,
}

impl ResilientVeil {
    pub fn new(inner: Arc<dyn VeilOps>) -> Self {
        Self {
            inner,
            cb: CircuitBreaker::new("veil", DEFAULT_FAILURE_THRESHOLD, DEFAULT_COOLDOWN),
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

#[async_trait::async_trait]
impl VeilOps for ResilientVeil {
    async fn create_index(&self, name: &str) -> Result<(), VeilError> {
        self.cb.check().map_err(VeilError::Connection)?;
        match tokio::time::timeout(self.timeout, self.inner.create_index(name)).await {
            Ok(Ok(v)) => {
                self.cb.record_success();
                Ok(v)
            }
            Ok(Err(e)) => {
                self.cb.record_failure();
                Err(e)
            }
            Err(_) => {
                self.cb.record_failure();
                Err(VeilError::Connection("request timeout".to_string()))
            }
        }
    }

    async fn put(&self, index: &str, entry_id: &str, plaintext: &str) -> Result<(), VeilError> {
        self.cb.check().map_err(VeilError::Connection)?;
        match tokio::time::timeout(self.timeout, self.inner.put(index, entry_id, plaintext)).await {
            Ok(Ok(v)) => {
                self.cb.record_success();
                Ok(v)
            }
            Ok(Err(e)) => {
                self.cb.record_failure();
                Err(e)
            }
            Err(_) => {
                self.cb.record_failure();
                Err(VeilError::Connection("request timeout".to_string()))
            }
        }
    }

    async fn delete(&self, index: &str, entry_id: &str) -> Result<(), VeilError> {
        self.cb.check().map_err(VeilError::Connection)?;
        match tokio::time::timeout(self.timeout, self.inner.delete(index, entry_id)).await {
            Ok(Ok(v)) => {
                self.cb.record_success();
                Ok(v)
            }
            Ok(Err(e)) => {
                self.cb.record_failure();
                Err(e)
            }
            Err(_) => {
                self.cb.record_failure();
                Err(VeilError::Connection("request timeout".to_string()))
            }
        }
    }

    async fn search(
        &self,
        index: &str,
        query: &str,
        limit: Option<usize>,
    ) -> Result<Vec<SearchHit>, VeilError> {
        self.cb.check().map_err(VeilError::Connection)?;
        match tokio::time::timeout(self.timeout, self.inner.search(index, query, limit)).await {
            Ok(Ok(v)) => {
                self.cb.record_success();
                Ok(v)
            }
            Ok(Err(e)) => {
                self.cb.record_failure();
                Err(e)
            }
            Err(_) => {
                self.cb.record_failure();
                Err(VeilError::Connection("request timeout".to_string()))
            }
        }
    }
}

/// Wraps a SentryOps with circuit breaker + timeout.
/// When circuit is open, **permits by default** (fail-open for authz to avoid blocking all messages).
pub struct ResilientSentry {
    inner: Arc<dyn SentryOps>,
    cb: CircuitBreaker,
    timeout: Duration,
}

impl ResilientSentry {
    pub fn new(inner: Arc<dyn SentryOps>) -> Self {
        Self {
            inner,
            cb: CircuitBreaker::new("sentry", DEFAULT_FAILURE_THRESHOLD, DEFAULT_COOLDOWN),
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

#[async_trait::async_trait]
impl SentryOps for ResilientSentry {
    async fn evaluate(
        &self,
        subject: &str,
        action: &str,
        resource: &str,
    ) -> Result<(), SentryError> {
        // Sentry fail-open: when circuit is open, permit the request
        // rather than blocking all messages
        if self.cb.check().is_err() {
            tracing::warn!(
                subject = subject,
                action = action,
                "sentry circuit open — permitting by default"
            );
            return Ok(());
        }
        match tokio::time::timeout(self.timeout, self.inner.evaluate(subject, action, resource))
            .await
        {
            Ok(Ok(())) => {
                self.cb.record_success();
                Ok(())
            }
            Ok(Err(SentryError::Denied(reason))) => {
                // Denial is a successful response from Sentry — don't trip circuit
                self.cb.record_success();
                Err(SentryError::Denied(reason))
            }
            Ok(Err(e)) => {
                self.cb.record_failure();
                Err(e)
            }
            Err(_) => {
                self.cb.record_failure();
                tracing::warn!("sentry request timeout — permitting by default");
                Ok(())
            }
        }
    }
}

/// Wraps a CourierOps with circuit breaker + timeout.
/// Fire-and-forget — failures are logged, not propagated.
pub struct ResilientCourier {
    inner: Arc<dyn CourierOps>,
    cb: CircuitBreaker,
    timeout: Duration,
}

impl ResilientCourier {
    pub fn new(inner: Arc<dyn CourierOps>) -> Self {
        Self {
            inner,
            cb: CircuitBreaker::new("courier", DEFAULT_FAILURE_THRESHOLD, DEFAULT_COOLDOWN),
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

#[async_trait::async_trait]
impl CourierOps for ResilientCourier {
    async fn notify(
        &self,
        channel: &str,
        recipient: &str,
        subject: &str,
        body: &str,
    ) -> Result<(), CourierError> {
        if self.cb.check().is_err() {
            return Err(CourierError::Connection("circuit open".to_string()));
        }
        match tokio::time::timeout(
            self.timeout,
            self.inner.notify(channel, recipient, subject, body),
        )
        .await
        {
            Ok(Ok(())) => {
                self.cb.record_success();
                Ok(())
            }
            Ok(Err(e)) => {
                self.cb.record_failure();
                Err(e)
            }
            Err(_) => {
                self.cb.record_failure();
                Err(CourierError::Connection("request timeout".to_string()))
            }
        }
    }
}

/// Wraps a ChronicleOps with circuit breaker + timeout.
/// Fire-and-forget — failures are logged, not propagated.
pub struct ResilientChronicle {
    inner: Arc<dyn ChronicleOps>,
    cb: CircuitBreaker,
    timeout: Duration,
}

impl ResilientChronicle {
    pub fn new(inner: Arc<dyn ChronicleOps>) -> Self {
        Self {
            inner,
            cb: CircuitBreaker::new("chronicle", DEFAULT_FAILURE_THRESHOLD, DEFAULT_COOLDOWN),
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

#[async_trait::async_trait]
impl ChronicleOps for ResilientChronicle {
    async fn ingest(&self, event: AuditEvent) -> Result<(), ChronicleError> {
        if self.cb.check().is_err() {
            return Err(ChronicleError::Connection("circuit open".to_string()));
        }
        match tokio::time::timeout(self.timeout, self.inner.ingest(event)).await {
            Ok(Ok(())) => {
                self.cb.record_success();
                Ok(())
            }
            Ok(Err(e)) => {
                self.cb.record_failure();
                Err(e)
            }
            Err(_) => {
                self.cb.record_failure();
                Err(ChronicleError::Connection("request timeout".to_string()))
            }
        }
    }
}
