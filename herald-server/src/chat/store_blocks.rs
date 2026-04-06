use shroudb_store::Store;

use crate::store::NS_BLOCKS;

/// Key: "{tenant}/{blocker}/{blocked}" -> "1"
fn block_key(tenant_id: &str, blocker: &str, blocked: &str) -> Vec<u8> {
    format!("{tenant_id}/{blocker}/{blocked}").into_bytes()
}

fn user_blocks_prefix(tenant_id: &str, blocker: &str) -> Vec<u8> {
    format!("{tenant_id}/{blocker}/").into_bytes()
}

pub async fn block<S: Store>(
    store: &S,
    tenant_id: &str,
    blocker: &str,
    blocked: &str,
) -> Result<(), anyhow::Error> {
    let key = block_key(tenant_id, blocker, blocked);
    store.put(NS_BLOCKS, &key, b"1", None).await?;
    Ok(())
}

pub async fn unblock<S: Store>(
    store: &S,
    tenant_id: &str,
    blocker: &str,
    blocked: &str,
) -> Result<bool, anyhow::Error> {
    let key = block_key(tenant_id, blocker, blocked);
    match store.delete(NS_BLOCKS, &key).await {
        Ok(_) => Ok(true),
        Err(shroudb_store::StoreError::NotFound) => Ok(false),
        Err(e) => Err(e.into()),
    }
}

pub async fn is_blocked<S: Store>(
    store: &S,
    tenant_id: &str,
    blocker: &str,
    blocked: &str,
) -> Result<bool, anyhow::Error> {
    let key = block_key(tenant_id, blocker, blocked);
    match store.get(NS_BLOCKS, &key, None).await {
        Ok(_) => Ok(true),
        Err(shroudb_store::StoreError::NotFound) => Ok(false),
        Err(e) => Err(e.into()),
    }
}

pub async fn list_blocked<S: Store>(
    store: &S,
    tenant_id: &str,
    blocker: &str,
) -> Result<Vec<String>, anyhow::Error> {
    let prefix = user_blocks_prefix(tenant_id, blocker);
    let mut blocked = Vec::new();
    let mut cursor = None;
    loop {
        let page = store
            .list(NS_BLOCKS, Some(&prefix), cursor.as_deref(), 100)
            .await?;
        for key in &page.keys {
            let key_str = String::from_utf8_lossy(key);
            // key: tenant/blocker/blocked
            if let Some(blocked_user) = key_str.rsplit('/').next() {
                blocked.push(blocked_user.to_string());
            }
        }
        if page.cursor.is_none() {
            break;
        }
        cursor = page.cursor;
    }
    Ok(blocked)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_block_unblock() {
        let store = crate::store::test_store().await;

        block(&*store, "t1", "alice", "bob").await.unwrap();
        assert!(is_blocked(&*store, "t1", "alice", "bob").await.unwrap());
        assert!(!is_blocked(&*store, "t1", "bob", "alice").await.unwrap());

        let blocked = list_blocked(&*store, "t1", "alice").await.unwrap();
        assert_eq!(blocked, vec!["bob"]);

        let removed = unblock(&*store, "t1", "alice", "bob").await.unwrap();
        assert!(removed);
        assert!(!is_blocked(&*store, "t1", "alice", "bob").await.unwrap());

        let removed = unblock(&*store, "t1", "alice", "bob").await.unwrap();
        assert!(!removed);
    }
}
