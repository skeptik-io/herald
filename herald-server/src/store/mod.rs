#[cfg(not(feature = "chat"))]
pub mod blocks;
#[cfg(not(feature = "chat"))]
pub mod cursors;
pub mod events;
pub mod members;
pub mod purge;
#[cfg(not(feature = "chat"))]
pub mod reactions;
pub mod sequences;
pub mod streams;
pub mod tenants;

use shroudb_store::Store;

pub const NS_TENANTS: &str = "herald.tenants";
pub const NS_API_TOKENS: &str = "herald.api_tokens";
pub const NS_STREAMS: &str = "herald.streams";
pub const NS_MEMBERS: &str = "herald.members";
pub const NS_EVENTS: &str = "herald.events";
pub const NS_CURSORS: &str = "herald.cursors";
pub const NS_REACTIONS: &str = "herald.reactions";
pub const NS_BLOCKS: &str = "herald.blocks";
pub const NS_SEQUENCES: &str = "herald.sequences";
pub const NS_TENANT_KEYS: &str = "herald.tenant_keys";

/// Initialize all namespaces on startup.
#[cfg(test)]
pub(crate) async fn test_store() -> std::sync::Arc<crate::store_backend::StoreBackend> {
    let dir = tempfile::tempdir().unwrap();
    let config = shroudb_storage::StorageEngineConfig {
        data_dir: dir.keep(),
        ..Default::default()
    };
    let engine = std::sync::Arc::new(
        shroudb_storage::StorageEngine::open(config, &shroudb_storage::EphemeralKey)
            .await
            .unwrap(),
    );
    let embedded = shroudb_storage::EmbeddedStore::new(engine, "test");
    let store = std::sync::Arc::new(crate::store_backend::StoreBackend::Embedded(embedded));
    init_namespaces(&*store).await.unwrap();
    store
}

pub async fn init_namespaces<S: Store>(store: &S) -> Result<(), shroudb_store::StoreError> {
    let config = shroudb_store::NamespaceConfig::default();
    for ns in [
        NS_TENANTS,
        NS_API_TOKENS,
        NS_STREAMS,
        NS_MEMBERS,
        NS_EVENTS,
        NS_CURSORS,
        NS_REACTIONS,
        NS_BLOCKS,
        NS_SEQUENCES,
        NS_TENANT_KEYS,
    ] {
        match store.namespace_create(ns, config.clone()).await {
            Ok(()) => {}
            Err(shroudb_store::StoreError::NamespaceExists(_)) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
