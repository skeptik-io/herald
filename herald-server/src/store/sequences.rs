//! Store-backed sequence allocation for clustered mode.
//!
//! Each `put` to a counter key returns a monotonically increasing version
//! number (1, 2, 3, ...). ShroudB guarantees per-key version uniqueness
//! across concurrent writers, making this safe for multi-instance use.

use shroudb_store::Store;

use super::NS_SEQUENCES;

fn counter_key(tenant_id: &str, stream_id: &str) -> Vec<u8> {
    format!("{tenant_id}/{stream_id}").into_bytes()
}

/// Allocate the next sequence number for a stream using the store's version counter.
/// Returns a unique, monotonically increasing sequence number.
pub async fn allocate<S: Store>(
    store: &S,
    tenant_id: &str,
    stream_id: &str,
) -> Result<u64, anyhow::Error> {
    let key = counter_key(tenant_id, stream_id);
    let version = store.put(NS_SEQUENCES, &key, b"_", None).await?;
    Ok(version)
}

/// Read the current sequence counter version (latest allocated seq).
/// Returns 0 if no sequences have been allocated for this stream.
pub async fn current<S: Store>(
    store: &S,
    tenant_id: &str,
    stream_id: &str,
) -> Result<u64, anyhow::Error> {
    let key = counter_key(tenant_id, stream_id);
    match store.versions(NS_SEQUENCES, &key, 1, None).await {
        Ok(versions) => Ok(versions.first().map(|v| v.version).unwrap_or(0)),
        Err(shroudb_store::StoreError::NotFound) => Ok(0),
        Err(e) => Err(e.into()),
    }
}

/// Advance the counter to at least `target_seq` by doing (target - current)
/// puts. Used during initialization when existing events have sequences
/// higher than the counter's current version.
pub async fn advance_to<S: Store>(
    store: &S,
    tenant_id: &str,
    stream_id: &str,
    target_seq: u64,
) -> Result<(), anyhow::Error> {
    let current_ver = current(store, tenant_id, stream_id).await?;
    if current_ver >= target_seq {
        return Ok(());
    }
    let needed = target_seq - current_ver;
    let key = counter_key(tenant_id, stream_id);
    for _ in 0..needed {
        store.put(NS_SEQUENCES, &key, b"_", None).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_allocate_sequential() {
        let store = crate::store::test_store().await;
        let s1 = allocate(&*store, "t1", "stream1").await.unwrap();
        let s2 = allocate(&*store, "t1", "stream1").await.unwrap();
        let s3 = allocate(&*store, "t1", "stream1").await.unwrap();
        assert_eq!(s1, 1);
        assert_eq!(s2, 2);
        assert_eq!(s3, 3);
    }

    #[tokio::test]
    async fn test_allocate_different_streams() {
        let store = crate::store::test_store().await;
        let a1 = allocate(&*store, "t1", "s1").await.unwrap();
        let b1 = allocate(&*store, "t1", "s2").await.unwrap();
        let a2 = allocate(&*store, "t1", "s1").await.unwrap();
        // Each stream has its own counter
        assert_eq!(a1, 1);
        assert_eq!(b1, 1);
        assert_eq!(a2, 2);
    }

    #[tokio::test]
    async fn test_current_empty() {
        let store = crate::store::test_store().await;
        let cur = current(&*store, "t1", "s1").await.unwrap();
        assert_eq!(cur, 0);
    }

    #[tokio::test]
    async fn test_current_after_allocations() {
        let store = crate::store::test_store().await;
        allocate(&*store, "t1", "s1").await.unwrap();
        allocate(&*store, "t1", "s1").await.unwrap();
        allocate(&*store, "t1", "s1").await.unwrap();
        let cur = current(&*store, "t1", "s1").await.unwrap();
        assert_eq!(cur, 3);
    }

    #[tokio::test]
    async fn test_advance_to() {
        let store = crate::store::test_store().await;
        advance_to(&*store, "t1", "s1", 10).await.unwrap();
        let cur = current(&*store, "t1", "s1").await.unwrap();
        assert_eq!(cur, 10);
        // Next allocation should be 11
        let next = allocate(&*store, "t1", "s1").await.unwrap();
        assert_eq!(next, 11);
    }

    #[tokio::test]
    async fn test_advance_to_already_ahead() {
        let store = crate::store::test_store().await;
        advance_to(&*store, "t1", "s1", 5).await.unwrap();
        // Advance to a lower value should be a no-op
        advance_to(&*store, "t1", "s1", 3).await.unwrap();
        let cur = current(&*store, "t1", "s1").await.unwrap();
        assert_eq!(cur, 5);
    }
}
