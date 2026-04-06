//! Unified store backend — wraps either EmbeddedStore or RemoteStore behind
//! the `Store` trait so Herald can run against local WAL or a shared ShroudB server.

use shroudb_client::RemoteStore;
use shroudb_storage::EmbeddedStore;
use shroudb_store::*;

pub enum StoreBackend {
    Embedded(EmbeddedStore),
    Remote(RemoteStore),
}

impl StoreBackend {
    /// Returns the embedded storage engine health, or Ready for remote (health is the server's concern).
    pub fn storage_health(&self) -> shroudb_storage::engine::HealthState {
        match self {
            Self::Embedded(s) => s.engine().health(),
            Self::Remote(_) => shroudb_storage::engine::HealthState::Ready,
        }
    }
}

/// Unified subscription type.
pub enum BackendSubscription {
    Embedded(shroudb_storage::embedded_store::EmbeddedSubscription),
    Remote(shroudb_client::remote_store::RemoteSubscription),
}

impl Subscription for BackendSubscription {
    async fn recv(&mut self) -> Option<SubscriptionEvent> {
        match self {
            Self::Embedded(s) => s.recv().await,
            Self::Remote(s) => s.recv().await,
        }
    }
}

impl Store for StoreBackend {
    type Subscription = BackendSubscription;

    async fn put(
        &self,
        ns: &str,
        key: &[u8],
        value: &[u8],
        metadata: Option<Metadata>,
    ) -> Result<u64, StoreError> {
        match self {
            Self::Embedded(s) => s.put(ns, key, value, metadata).await,
            Self::Remote(s) => s.put(ns, key, value, metadata).await,
        }
    }

    async fn get(&self, ns: &str, key: &[u8], version: Option<u64>) -> Result<Entry, StoreError> {
        match self {
            Self::Embedded(s) => s.get(ns, key, version).await,
            Self::Remote(s) => s.get(ns, key, version).await,
        }
    }

    async fn delete(&self, ns: &str, key: &[u8]) -> Result<u64, StoreError> {
        match self {
            Self::Embedded(s) => s.delete(ns, key).await,
            Self::Remote(s) => s.delete(ns, key).await,
        }
    }

    async fn list(
        &self,
        ns: &str,
        prefix: Option<&[u8]>,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Page, StoreError> {
        match self {
            Self::Embedded(s) => s.list(ns, prefix, cursor, limit).await,
            Self::Remote(s) => s.list(ns, prefix, cursor, limit).await,
        }
    }

    async fn versions(
        &self,
        ns: &str,
        key: &[u8],
        limit: usize,
        from_version: Option<u64>,
    ) -> Result<Vec<VersionInfo>, StoreError> {
        match self {
            Self::Embedded(s) => s.versions(ns, key, limit, from_version).await,
            Self::Remote(s) => s.versions(ns, key, limit, from_version).await,
        }
    }

    async fn namespace_create(&self, ns: &str, config: NamespaceConfig) -> Result<(), StoreError> {
        match self {
            Self::Embedded(s) => s.namespace_create(ns, config).await,
            Self::Remote(s) => s.namespace_create(ns, config).await,
        }
    }

    async fn namespace_drop(&self, ns: &str, force: bool) -> Result<(), StoreError> {
        match self {
            Self::Embedded(s) => s.namespace_drop(ns, force).await,
            Self::Remote(s) => s.namespace_drop(ns, force).await,
        }
    }

    async fn namespace_list(&self, cursor: Option<&str>, limit: usize) -> Result<Page, StoreError> {
        match self {
            Self::Embedded(s) => s.namespace_list(cursor, limit).await,
            Self::Remote(s) => s.namespace_list(cursor, limit).await,
        }
    }

    async fn namespace_info(&self, ns: &str) -> Result<NamespaceInfo, StoreError> {
        match self {
            Self::Embedded(s) => s.namespace_info(ns).await,
            Self::Remote(s) => s.namespace_info(ns).await,
        }
    }

    async fn namespace_alter(&self, ns: &str, config: NamespaceConfig) -> Result<(), StoreError> {
        match self {
            Self::Embedded(s) => s.namespace_alter(ns, config).await,
            Self::Remote(s) => s.namespace_alter(ns, config).await,
        }
    }

    async fn namespace_validate(&self, ns: &str) -> Result<Vec<ValidationReport>, StoreError> {
        match self {
            Self::Embedded(s) => s.namespace_validate(ns).await,
            Self::Remote(s) => s.namespace_validate(ns).await,
        }
    }

    async fn pipeline(
        &self,
        commands: Vec<PipelineCommand>,
    ) -> Result<Vec<PipelineResult>, StoreError> {
        match self {
            Self::Embedded(s) => s.pipeline(commands).await,
            Self::Remote(s) => s.pipeline(commands).await,
        }
    }

    async fn subscribe(
        &self,
        ns: &str,
        filter: SubscriptionFilter,
    ) -> Result<Self::Subscription, StoreError> {
        match self {
            Self::Embedded(s) => Ok(BackendSubscription::Embedded(
                s.subscribe(ns, filter).await?,
            )),
            Self::Remote(s) => Ok(BackendSubscription::Remote(s.subscribe(ns, filter).await?)),
        }
    }
}
