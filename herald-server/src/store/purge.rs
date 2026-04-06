//! GDPR data deletion — purge all data for a tenant or all events/reactions/cursors for a user.

use shroudb_store::Store;
use tracing::{info, warn};

use super::{
    NS_API_TOKENS, NS_BLOCKS, NS_CURSORS, NS_EVENTS, NS_MEMBERS, NS_REACTIONS, NS_STREAMS,
    NS_TENANTS,
};

/// Delete all keys with a given prefix in a namespace. Returns count of keys deleted.
async fn delete_prefix<S: Store>(store: &S, ns: &str, prefix: &[u8]) -> Result<u64, anyhow::Error> {
    let mut deleted = 0u64;
    let mut cursor = None;
    loop {
        let page = store.list(ns, Some(prefix), cursor.as_deref(), 500).await?;
        for key in &page.keys {
            match store.delete(ns, key).await {
                Ok(_) => deleted += 1,
                Err(shroudb_store::StoreError::NotFound) => {} // already gone
                Err(e) => warn!(ns, ?key, "purge: failed to delete key: {e}"),
            }
        }
        if page.cursor.is_none() {
            break;
        }
        cursor = page.cursor;
    }
    Ok(deleted)
}

/// Purge ALL data for a tenant across all namespaces.
///
/// Deletes: tenant record, API tokens, streams, members, events, cursors,
/// reactions, and blocks. Returns total keys deleted.
pub async fn purge_tenant<S: Store>(store: &S, tenant_id: &str) -> Result<u64, anyhow::Error> {
    let prefix = format!("{tenant_id}/").into_bytes();
    let mut total = 0u64;

    // Events (both seq-indexed and id-indexed keys)
    total += delete_prefix(store, NS_EVENTS, &prefix).await?;
    // Streams
    total += delete_prefix(store, NS_STREAMS, &prefix).await?;
    // Members
    total += delete_prefix(store, NS_MEMBERS, &prefix).await?;
    // Cursors
    total += delete_prefix(store, NS_CURSORS, &prefix).await?;
    // Reactions
    total += delete_prefix(store, NS_REACTIONS, &prefix).await?;
    // Blocks
    total += delete_prefix(store, NS_BLOCKS, &prefix).await?;

    // API tokens — these are keyed by token hash, not tenant prefix.
    // We need to scan all tokens and delete those belonging to this tenant.
    total += delete_tenant_tokens(store, tenant_id).await?;

    // Tenant record itself
    let tenant_key = tenant_id.as_bytes();
    match store.delete(NS_TENANTS, tenant_key).await {
        Ok(_) => total += 1,
        Err(shroudb_store::StoreError::NotFound) => {}
        Err(e) => warn!(
            tenant = tenant_id,
            "purge: failed to delete tenant record: {e}"
        ),
    }

    info!(tenant = tenant_id, deleted = total, "tenant data purged");
    Ok(total)
}

/// Delete all API tokens belonging to a tenant.
async fn delete_tenant_tokens<S: Store>(store: &S, tenant_id: &str) -> Result<u64, anyhow::Error> {
    let mut deleted = 0u64;
    let mut cursor = None;
    loop {
        let page = store
            .list(NS_API_TOKENS, None, cursor.as_deref(), 500)
            .await?;
        for key in &page.keys {
            // Token values contain the tenant_id — check before deleting
            if let Ok(entry) = store.get(NS_API_TOKENS, key, None).await {
                if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&entry.value) {
                    if val.get("tenant_id").and_then(|v| v.as_str()) == Some(tenant_id) {
                        match store.delete(NS_API_TOKENS, key).await {
                            Ok(_) => deleted += 1,
                            Err(e) => warn!(?key, "purge: failed to delete token: {e}"),
                        }
                    }
                }
            }
        }
        if page.cursor.is_none() {
            break;
        }
        cursor = page.cursor;
    }
    Ok(deleted)
}

/// Purge all events by a specific user within a stream, plus their cursors,
/// reactions, and blocks. Returns total keys deleted.
pub async fn purge_user_data<S: Store>(
    store: &S,
    tenant_id: &str,
    stream_id: &str,
    user_id: &str,
) -> Result<u64, anyhow::Error> {
    let mut total = 0u64;

    // Events — scan and delete events where sender == user_id
    let event_prefix = format!("{tenant_id}/{stream_id}/").into_bytes();
    let mut cursor = None;
    loop {
        let page = store
            .list(NS_EVENTS, Some(&event_prefix), cursor.as_deref(), 500)
            .await?;
        for key in &page.keys {
            if let Ok(entry) = store.get(NS_EVENTS, key, None).await {
                if let Ok(stored) = serde_json::from_slice::<serde_json::Value>(&entry.value) {
                    if stored.get("sender").and_then(|v| v.as_str()) == Some(user_id) {
                        // Also delete the ID-index key
                        if let Some(event_id) = stored.get("id").and_then(|v| v.as_str()) {
                            let id_key = format!("{tenant_id}/id/{event_id}").into_bytes();
                            let _ = store.delete(NS_EVENTS, &id_key).await;
                            total += 1;
                        }
                        match store.delete(NS_EVENTS, key).await {
                            Ok(_) => total += 1,
                            Err(e) => warn!(?key, "purge: failed to delete event: {e}"),
                        }
                    }
                }
            }
        }
        if page.cursor.is_none() {
            break;
        }
        cursor = page.cursor;
    }

    // Cursor for this user in this stream
    let cursor_key = format!("{tenant_id}/{stream_id}/{user_id}").into_bytes();
    match store.delete(NS_CURSORS, &cursor_key).await {
        Ok(_) => total += 1,
        Err(shroudb_store::StoreError::NotFound) => {}
        Err(e) => warn!("purge: failed to delete cursor: {e}"),
    }

    // Reactions by this user in this stream (prefix: {tenant}/{stream}/)
    // Need to scan all reactions in the stream and filter by user_id segment
    let reaction_prefix = format!("{tenant_id}/{stream_id}/").into_bytes();
    let user_suffix = format!("/{user_id}");
    let mut rcursor = None;
    loop {
        let page = store
            .list(
                NS_REACTIONS,
                Some(&reaction_prefix),
                rcursor.as_deref(),
                500,
            )
            .await?;
        for key in &page.keys {
            // Key format: {tenant}/{stream}/{event_id}/{emoji}/{user_id}
            let key_str = String::from_utf8_lossy(key);
            if key_str.ends_with(&user_suffix) {
                match store.delete(NS_REACTIONS, key).await {
                    Ok(_) => total += 1,
                    Err(e) => warn!(?key, "purge: failed to delete reaction: {e}"),
                }
            }
        }
        if page.cursor.is_none() {
            break;
        }
        rcursor = page.cursor;
    }

    // Blocks involving this user (as blocker or blocked)
    // Blocker prefix: {tenant}/{user_id}/
    let blocker_prefix = format!("{tenant_id}/{user_id}/").into_bytes();
    total += delete_prefix(store, NS_BLOCKS, &blocker_prefix).await?;
    // Also scan for blocks where this user is the blocked party
    // Key: {tenant}/{blocker}/{user_id} — need to scan all blocks for tenant
    let block_prefix = format!("{tenant_id}/").into_bytes();
    let blocked_suffix = format!("/{user_id}");
    let mut bcursor = None;
    loop {
        let page = store
            .list(NS_BLOCKS, Some(&block_prefix), bcursor.as_deref(), 500)
            .await?;
        for key in &page.keys {
            let key_str = String::from_utf8_lossy(key);
            if key_str.ends_with(&blocked_suffix) {
                match store.delete(NS_BLOCKS, key).await {
                    Ok(_) => total += 1,
                    Err(e) => warn!(?key, "purge: failed to delete block: {e}"),
                }
            }
        }
        if page.cursor.is_none() {
            break;
        }
        bcursor = page.cursor;
    }

    // Member record
    let member_key = format!("{tenant_id}/{stream_id}/{user_id}").into_bytes();
    match store.delete(NS_MEMBERS, &member_key).await {
        Ok(_) => total += 1,
        Err(shroudb_store::StoreError::NotFound) => {}
        Err(e) => warn!("purge: failed to delete member: {e}"),
    }

    info!(
        tenant = tenant_id,
        stream = stream_id,
        user = user_id,
        deleted = total,
        "user data purged"
    );
    Ok(total)
}
