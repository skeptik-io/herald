use std::sync::LazyLock;

use dashmap::DashMap;
use herald_core::cursor::Cursor;
use herald_core::event::Sequence;
use shroudb_store::Store;
use tokio::sync::Mutex;

use super::NS_CURSORS;

fn cursor_key(tenant_id: &str, stream_id: &str, user_id: &str) -> Vec<u8> {
    format!("{tenant_id}/{stream_id}/{user_id}").into_bytes()
}

fn stream_prefix(tenant_id: &str, stream_id: &str) -> Vec<u8> {
    format!("{tenant_id}/{stream_id}/").into_bytes()
}

/// Per-key lock to prevent read-modify-write races on cursor MAX semantics.
static CURSOR_LOCKS: LazyLock<DashMap<Vec<u8>, std::sync::Arc<Mutex<()>>>> =
    LazyLock::new(DashMap::new);

pub async fn upsert<S: Store>(
    store: &S,
    tenant_id: &str,
    stream_id: &str,
    user_id: &str,
    seq: Sequence,
    now_ms: i64,
) -> Result<(), anyhow::Error> {
    let key = cursor_key(tenant_id, stream_id, user_id);

    let lock = CURSOR_LOCKS
        .entry(key.clone())
        .or_insert_with(|| std::sync::Arc::new(Mutex::new(())))
        .clone();
    let _guard = lock.lock().await;

    let current_seq = match store.get(NS_CURSORS, &key, None).await {
        Ok(entry) => {
            let c: Cursor = serde_json::from_slice(&entry.value)?;
            c.seq
        }
        Err(shroudb_store::StoreError::NotFound) => 0,
        Err(e) => return Err(e.into()),
    };

    let new_seq = seq.max(current_seq);
    let cursor = Cursor {
        stream_id: stream_id.to_string(),
        user_id: user_id.to_string(),
        seq: new_seq,
        updated_at: now_ms,
    };
    let value = serde_json::to_vec(&cursor)?;
    store.put(NS_CURSORS, &key, &value, None).await?;
    Ok(())
}

pub async fn get<S: Store>(
    store: &S,
    tenant_id: &str,
    stream_id: &str,
    user_id: &str,
) -> Result<Sequence, anyhow::Error> {
    let key = cursor_key(tenant_id, stream_id, user_id);
    match store.get(NS_CURSORS, &key, None).await {
        Ok(entry) => {
            let c: Cursor = serde_json::from_slice(&entry.value)?;
            Ok(c.seq)
        }
        Err(shroudb_store::StoreError::NotFound) => Ok(0),
        Err(e) => Err(e.into()),
    }
}

pub async fn list_by_stream<S: Store>(
    store: &S,
    tenant_id: &str,
    stream_id: &str,
) -> Result<Vec<Cursor>, anyhow::Error> {
    let prefix = stream_prefix(tenant_id, stream_id);
    let mut cursors = Vec::new();
    let mut cursor = None;
    loop {
        let page = store
            .list(NS_CURSORS, Some(&prefix), cursor.as_deref(), 100)
            .await?;
        for key in &page.keys {
            if let Ok(entry) = store.get(NS_CURSORS, key, None).await {
                if let Ok(c) = serde_json::from_slice::<Cursor>(&entry.value) {
                    cursors.push(c);
                }
            }
        }
        if page.cursor.is_none() {
            break;
        }
        cursor = page.cursor;
    }
    Ok(cursors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cursor_upsert_and_get() {
        let store = crate::store::test_store().await;
        upsert(&*store, "t1", "r1", "u1", 5, 1000).await.unwrap();

        let seq = get(&*store, "t1", "r1", "u1").await.unwrap();
        assert_eq!(seq, 5);

        // Missing cursor returns 0
        let seq = get(&*store, "t1", "r1", "nope").await.unwrap();
        assert_eq!(seq, 0);
    }

    #[tokio::test]
    async fn test_cursor_max_semantics() {
        let store = crate::store::test_store().await;
        upsert(&*store, "t1", "r1", "u1", 10, 1000).await.unwrap();
        // Lower seq should not overwrite
        upsert(&*store, "t1", "r1", "u1", 5, 2000).await.unwrap();

        let seq = get(&*store, "t1", "r1", "u1").await.unwrap();
        assert_eq!(seq, 10);

        // Higher seq should overwrite
        upsert(&*store, "t1", "r1", "u1", 20, 3000).await.unwrap();
        let seq = get(&*store, "t1", "r1", "u1").await.unwrap();
        assert_eq!(seq, 20);
    }

    #[tokio::test]
    async fn test_cursor_list_by_stream() {
        let store = crate::store::test_store().await;
        for i in 0..3 {
            upsert(&*store, "t1", "r1", &format!("u{i}"), i + 1, 1000)
                .await
                .unwrap();
        }
        // Different stream
        upsert(&*store, "t1", "r2", "u0", 1, 1000).await.unwrap();

        let cursors = list_by_stream(&*store, "t1", "r1").await.unwrap();
        assert_eq!(cursors.len(), 3);

        let cursors = list_by_stream(&*store, "t1", "r2").await.unwrap();
        assert_eq!(cursors.len(), 1);
    }

    #[tokio::test]
    async fn test_cursor_tenant_isolation() {
        let store = crate::store::test_store().await;
        upsert(&*store, "t1", "r1", "u1", 5, 1000).await.unwrap();
        upsert(&*store, "t2", "r1", "u1", 10, 1000).await.unwrap();

        let seq1 = get(&*store, "t1", "r1", "u1").await.unwrap();
        let seq2 = get(&*store, "t2", "r1", "u1").await.unwrap();
        assert_eq!(seq1, 5);
        assert_eq!(seq2, 10);
    }

    #[tokio::test]
    async fn test_cursor_concurrent_max_semantics() {
        let store = std::sync::Arc::new(crate::store::test_store().await);
        upsert(&**store, "t1", "r1", "u1", 0, 1000).await.unwrap();

        let mut handles = Vec::new();
        for seq in 1..=50u64 {
            let s = store.clone();
            handles.push(tokio::spawn(async move {
                upsert(&**s, "t1", "r1", "u1", seq, 1000 + seq as i64)
                    .await
                    .unwrap();
            }));
        }
        for h in handles {
            h.await.unwrap();
        }

        let seq = get(&**store, "t1", "r1", "u1").await.unwrap();
        assert_eq!(seq, 50, "concurrent MAX must converge to highest value");
    }
}
