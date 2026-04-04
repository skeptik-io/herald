use herald_core::stream::Stream;
use shroudb_store::Store;

use super::NS_STREAMS;

/// Key format: "{tenant_id}/{stream_id}"
fn stream_key(tenant_id: &str, stream_id: &str) -> Vec<u8> {
    format!("{tenant_id}/{stream_id}").into_bytes()
}

fn tenant_prefix(tenant_id: &str) -> Vec<u8> {
    format!("{tenant_id}/").into_bytes()
}

pub async fn insert<S: Store>(
    store: &S,
    tenant_id: &str,
    stream: &Stream,
) -> Result<(), anyhow::Error> {
    let key = stream_key(tenant_id, stream.id.as_str());
    let value = serde_json::to_vec(stream)?;
    store.put(NS_STREAMS, &key, &value, None).await?;
    Ok(())
}

pub async fn get<S: Store>(
    store: &S,
    tenant_id: &str,
    id: &str,
) -> Result<Option<Stream>, anyhow::Error> {
    let key = stream_key(tenant_id, id);
    match store.get(NS_STREAMS, &key, None).await {
        Ok(entry) => Ok(Some(serde_json::from_slice(&entry.value)?)),
        Err(shroudb_store::StoreError::NotFound) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub async fn list_by_tenant<S: Store>(
    store: &S,
    tenant_id: &str,
) -> Result<Vec<Stream>, anyhow::Error> {
    let prefix = tenant_prefix(tenant_id);
    let mut streams = Vec::new();
    let mut cursor = None;
    loop {
        let page = store
            .list(NS_STREAMS, Some(&prefix), cursor.as_deref(), 100)
            .await?;
        for key in &page.keys {
            if let Ok(entry) = store.get(NS_STREAMS, key, None).await {
                if let Ok(r) = serde_json::from_slice::<Stream>(&entry.value) {
                    streams.push(r);
                }
            }
        }
        if page.cursor.is_none() {
            break;
        }
        cursor = page.cursor;
    }
    Ok(streams)
}

/// List all streams across all tenants (for startup hydration).
pub async fn list_all<S: Store>(store: &S) -> Result<Vec<(String, Stream)>, anyhow::Error> {
    let mut results = Vec::new();
    let mut cursor = None;
    loop {
        let page = store.list(NS_STREAMS, None, cursor.as_deref(), 100).await?;
        for key in &page.keys {
            if let Ok(entry) = store.get(NS_STREAMS, key, None).await {
                let key_str = String::from_utf8_lossy(key);
                if let Some((tid, _)) = key_str.split_once('/') {
                    if let Ok(r) = serde_json::from_slice::<Stream>(&entry.value) {
                        results.push((tid.to_string(), r));
                    }
                }
            }
        }
        if page.cursor.is_none() {
            break;
        }
        cursor = page.cursor;
    }
    Ok(results)
}

pub async fn update<S: Store>(
    store: &S,
    tenant_id: &str,
    id: &str,
    name: Option<&str>,
    meta: Option<&serde_json::Value>,
    archived: Option<bool>,
) -> Result<bool, anyhow::Error> {
    let key = stream_key(tenant_id, id);
    let entry = match store.get(NS_STREAMS, &key, None).await {
        Ok(e) => e,
        Err(shroudb_store::StoreError::NotFound) => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    let mut stream: Stream = serde_json::from_slice(&entry.value)?;
    if let Some(n) = name {
        stream.name = n.to_string();
    }
    if let Some(m) = meta {
        stream.meta = Some(m.clone());
    }
    if let Some(a) = archived {
        stream.archived = a;
    }
    let value = serde_json::to_vec(&stream)?;
    store.put(NS_STREAMS, &key, &value, None).await?;
    Ok(true)
}

pub async fn delete<S: Store>(store: &S, tenant_id: &str, id: &str) -> Result<bool, anyhow::Error> {
    let key = stream_key(tenant_id, id);
    match store.delete(NS_STREAMS, &key).await {
        Ok(_) => Ok(true),
        Err(shroudb_store::StoreError::NotFound) => Ok(false),
        Err(e) => Err(e.into()),
    }
    // Note: members, events, cursors cleanup should be handled by caller
}

#[cfg(test)]
mod tests {
    use super::*;
    use herald_core::stream::StreamId;

    #[tokio::test]
    async fn test_stream_crud() {
        let store = crate::store::test_store().await;
        let stream = Stream {
            id: StreamId("r1".into()),
            name: "Stream 1".into(),
            meta: None,
            archived: false,
            public: false,
            created_at: 1000,
        };
        insert(&*store, "t1", &stream).await.unwrap();

        let got = get(&*store, "t1", "r1").await.unwrap().unwrap();
        assert_eq!(got.name, "Stream 1");

        assert!(get(&*store, "t1", "nope").await.unwrap().is_none());
        assert!(get(&*store, "t2", "r1").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_stream_list_by_tenant() {
        let store = crate::store::test_store().await;
        for i in 0..3 {
            let r = Stream {
                id: StreamId(format!("r{i}")),
                name: format!("R{i}"),
                meta: None,
                archived: false,
                public: false,
                created_at: 1000,
            };
            insert(&*store, "t1", &r).await.unwrap();
        }
        let r = Stream {
            id: StreamId("other".into()),
            name: "Other".into(),
            meta: None,
            archived: false,
            public: false,
            created_at: 1000,
        };
        insert(&*store, "t2", &r).await.unwrap();

        let streams = list_by_tenant(&*store, "t1").await.unwrap();
        assert_eq!(streams.len(), 3);

        let streams = list_by_tenant(&*store, "t2").await.unwrap();
        assert_eq!(streams.len(), 1);
    }

    #[tokio::test]
    async fn test_stream_update_and_archive() {
        let store = crate::store::test_store().await;
        let r = Stream {
            id: StreamId("r1".into()),
            name: "Old".into(),
            meta: None,
            archived: false,
            public: false,
            created_at: 1000,
        };
        insert(&*store, "t1", &r).await.unwrap();

        update(&*store, "t1", "r1", Some("New"), None, Some(true))
            .await
            .unwrap();
        let got = get(&*store, "t1", "r1").await.unwrap().unwrap();
        assert_eq!(got.name, "New");
        assert!(got.archived);
    }

    #[tokio::test]
    async fn test_stream_delete() {
        let store = crate::store::test_store().await;
        let r = Stream {
            id: StreamId("r1".into()),
            name: "R".into(),
            meta: None,
            archived: false,
            public: false,
            created_at: 1000,
        };
        insert(&*store, "t1", &r).await.unwrap();

        assert!(delete(&*store, "t1", "r1").await.unwrap());
        assert!(get(&*store, "t1", "r1").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_stream_list_all() {
        let store = crate::store::test_store().await;
        let r1 = Stream {
            id: StreamId("r1".into()),
            name: "R1".into(),
            meta: None,
            archived: false,
            public: false,
            created_at: 1000,
        };
        let r2 = Stream {
            id: StreamId("r2".into()),
            name: "R2".into(),
            meta: None,
            archived: false,
            public: false,
            created_at: 1000,
        };
        insert(&*store, "t1", &r1).await.unwrap();
        insert(&*store, "t2", &r2).await.unwrap();

        let all = list_all(&*store).await.unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|(tid, _)| tid == "t1"));
        assert!(all.iter().any(|(tid, _)| tid == "t2"));
    }
}
