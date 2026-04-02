pub mod cursors;
pub mod members;
pub mod messages;
pub mod rooms;
pub mod tenants;

use shroudb_store::Store;

pub const NS_TENANTS: &str = "herald.tenants";
pub const NS_API_TOKENS: &str = "herald.api_tokens";
pub const NS_ROOMS: &str = "herald.rooms";
pub const NS_MEMBERS: &str = "herald.members";
pub const NS_MESSAGES: &str = "herald.messages";
pub const NS_CURSORS: &str = "herald.cursors";

/// Initialize all namespaces on startup.
pub async fn init_namespaces<S: Store>(store: &S) -> Result<(), shroudb_store::StoreError> {
    let config = shroudb_store::NamespaceConfig::default();
    for ns in [
        NS_TENANTS,
        NS_API_TOKENS,
        NS_ROOMS,
        NS_MEMBERS,
        NS_MESSAGES,
        NS_CURSORS,
    ] {
        match store.namespace_create(ns, config.clone()).await {
            Ok(()) => {}
            Err(shroudb_store::StoreError::NamespaceExists(_)) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
