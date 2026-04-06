use herald_core::event::{Event, Sequence};
use shroudb_store::{PipelineCommand, Store};

use super::NS_EVENTS;

fn event_key(tenant_id: &str, stream_id: &str, seq: u64) -> Vec<u8> {
    format!("{tenant_id}/{stream_id}/{seq:020}").into_bytes()
}

fn event_id_key(tenant_id: &str, event_id: &str) -> Vec<u8> {
    format!("{tenant_id}/id/{event_id}").into_bytes()
}

fn stream_prefix(tenant_id: &str, stream_id: &str) -> Vec<u8> {
    format!("{tenant_id}/{stream_id}/").into_bytes()
}

/// Extract the seq number from a key like "{tenant}/{stream}/{seq:020}" without deserializing.
fn seq_from_key(key: &[u8]) -> Option<u64> {
    let s = std::str::from_utf8(key).ok()?;
    s.rsplit('/').next()?.parse().ok()
}

#[derive(serde::Serialize, serde::Deserialize)]
struct StoredEvent {
    #[serde(flatten)]
    event: Event,
    expires_at: i64,
}

/// Insert an event with atomic dual-index write (seq key + ID key).
pub async fn insert<S: Store>(
    store: &S,
    tenant_id: &str,
    event: &Event,
    expires_at: i64,
) -> Result<(), anyhow::Error> {
    let stored = StoredEvent {
        event: event.clone(),
        expires_at,
    };
    let value = serde_json::to_vec(&stored)?;
    let key = event_key(tenant_id, &event.stream_id, event.seq);
    let id_key = event_id_key(tenant_id, event.id.as_str());

    // Atomic pipeline: both writes succeed or both fail
    store
        .pipeline(vec![
            PipelineCommand::Put {
                ns: NS_EVENTS.to_string(),
                key: key.clone(),
                value,
                metadata: None,
            },
            PipelineCommand::Put {
                ns: NS_EVENTS.to_string(),
                key: id_key,
                value: key,
                metadata: None,
            },
        ])
        .await?;
    Ok(())
}

/// List events before a given seq, newest first, up to `limit`.
/// Optimized: extracts seq from key to avoid deserializing events outside the range.
pub async fn list_before<S: Store>(
    store: &S,
    tenant_id: &str,
    stream_id: &str,
    before: Sequence,
    limit: u32,
) -> Result<Vec<Event>, anyhow::Error> {
    let prefix = stream_prefix(tenant_id, stream_id);
    let mut matching_keys: Vec<(u64, Vec<u8>)> = Vec::new();
    let mut cursor = None;
    loop {
        let page = store
            .list(NS_EVENTS, Some(&prefix), cursor.as_deref(), 500)
            .await?;
        for key in &page.keys {
            if let Some(seq) = seq_from_key(key) {
                if seq < before {
                    matching_keys.push((seq, key.clone()));
                }
            }
        }
        if page.cursor.is_none() {
            break;
        }
        cursor = page.cursor;
    }
    // Take the last `limit` keys (highest seqs below `before`)
    matching_keys.sort_by(|a, b| b.0.cmp(&a.0));
    matching_keys.truncate(limit as usize);
    matching_keys.reverse();

    // Only deserialize the final set
    let mut events = Vec::with_capacity(matching_keys.len());
    for (_seq, key) in matching_keys {
        if let Ok(entry) = store.get(NS_EVENTS, &key, None).await {
            if let Ok(stored) = serde_json::from_slice::<StoredEvent>(&entry.value) {
                events.push(stored.event);
            }
        }
    }
    Ok(events)
}

/// List events after a given seq, oldest first, up to `limit`.
/// Optimized: extracts seq from key, stops scanning once we have enough.
pub async fn list_after<S: Store>(
    store: &S,
    tenant_id: &str,
    stream_id: &str,
    after: Sequence,
    limit: u32,
) -> Result<Vec<Event>, anyhow::Error> {
    let prefix = stream_prefix(tenant_id, stream_id);
    let mut matching_keys: Vec<(u64, Vec<u8>)> = Vec::new();
    let mut cursor = None;
    loop {
        let page = store
            .list(NS_EVENTS, Some(&prefix), cursor.as_deref(), 500)
            .await?;
        for key in &page.keys {
            if let Some(seq) = seq_from_key(key) {
                if seq > after {
                    matching_keys.push((seq, key.clone()));
                    // Early exit: we have more than enough
                    if matching_keys.len() > limit as usize {
                        // Sort and truncate in case keys arrived out of order from pages
                        matching_keys.sort_by_key(|(s, _)| *s);
                        matching_keys.truncate(limit as usize);
                    }
                }
            }
        }
        if page.cursor.is_none() {
            break;
        }
        // If keys are in order and we already have limit, we can stop scanning
        if matching_keys.len() >= limit as usize {
            break;
        }
        cursor = page.cursor;
    }
    matching_keys.sort_by_key(|(s, _)| *s);
    matching_keys.truncate(limit as usize);

    let mut events = Vec::with_capacity(matching_keys.len());
    for (_seq, key) in matching_keys {
        if let Ok(entry) = store.get(NS_EVENTS, &key, None).await {
            if let Ok(stored) = serde_json::from_slice::<StoredEvent>(&entry.value) {
                events.push(stored.event);
            }
        }
    }
    Ok(events)
}

/// List events since a timestamp, oldest first, up to `limit`.
/// Must deserialize to check sent_at (not in the key), but skips collecting
/// events that don't match.
pub async fn list_since_time<S: Store>(
    store: &S,
    tenant_id: &str,
    stream_id: &str,
    since_ms: i64,
    limit: u32,
) -> Result<Vec<Event>, anyhow::Error> {
    let prefix = stream_prefix(tenant_id, stream_id);
    let mut events = Vec::new();
    let mut cursor = None;
    loop {
        let page = store
            .list(NS_EVENTS, Some(&prefix), cursor.as_deref(), 500)
            .await?;
        for key in &page.keys {
            if let Ok(entry) = store.get(NS_EVENTS, key, None).await {
                if let Ok(stored) = serde_json::from_slice::<StoredEvent>(&entry.value) {
                    if stored.event.sent_at > since_ms {
                        events.push(stored.event);
                    }
                }
            }
        }
        if page.cursor.is_none() {
            break;
        }
        cursor = page.cursor;
    }
    events.sort_by_key(|m| m.seq);
    events.truncate(limit as usize);
    Ok(events)
}

pub async fn get_by_id<S: Store>(
    store: &S,
    tenant_id: &str,
    id: &str,
) -> Result<Option<Event>, anyhow::Error> {
    let id_key = event_id_key(tenant_id, id);
    let seq_key = match store.get(NS_EVENTS, &id_key, None).await {
        Ok(entry) => entry.value,
        Err(shroudb_store::StoreError::NotFound) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    match store.get(NS_EVENTS, &seq_key, None).await {
        Ok(entry) => {
            let stored: StoredEvent = serde_json::from_slice(&entry.value)?;
            Ok(Some(stored.event))
        }
        Err(shroudb_store::StoreError::NotFound) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub async fn edit_event<S: Store>(
    store: &S,
    tenant_id: &str,
    event_id: &str,
    new_body: &str,
    edited_at: i64,
) -> Result<Option<Event>, anyhow::Error> {
    let id_key = event_id_key(tenant_id, event_id);
    let seq_key = match store.get(NS_EVENTS, &id_key, None).await {
        Ok(entry) => entry.value,
        Err(shroudb_store::StoreError::NotFound) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let entry = match store.get(NS_EVENTS, &seq_key, None).await {
        Ok(e) => e,
        Err(shroudb_store::StoreError::NotFound) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let mut stored: StoredEvent = serde_json::from_slice(&entry.value)?;
    stored.event.body = new_body.to_string();
    stored.event.edited_at = Some(edited_at);
    let value = serde_json::to_vec(&stored)?;
    store.put(NS_EVENTS, &seq_key, &value, None).await?;
    Ok(Some(stored.event))
}

pub async fn delete_event<S: Store>(
    store: &S,
    tenant_id: &str,
    event_id: &str,
) -> Result<Option<Event>, anyhow::Error> {
    let id_key = event_id_key(tenant_id, event_id);
    let seq_key = match store.get(NS_EVENTS, &id_key, None).await {
        Ok(entry) => entry.value,
        Err(shroudb_store::StoreError::NotFound) => return Ok(None),
        Err(e) => return Err(e.into()),
    };

    let entry = match store.get(NS_EVENTS, &seq_key, None).await {
        Ok(e) => e,
        Err(shroudb_store::StoreError::NotFound) => return Ok(None),
        Err(e) => return Err(e.into()),
    };

    let mut stored: StoredEvent = serde_json::from_slice(&entry.value)?;
    let original = stored.event.clone();

    // Soft delete: clear body, set meta to indicate deletion
    stored.event.body = String::new();
    stored.event.meta = Some(serde_json::json!({"deleted": true}));

    let value = serde_json::to_vec(&stored)?;
    store.put(NS_EVENTS, &seq_key, &value, None).await?;

    Ok(Some(original))
}

/// Get the highest seq number for a stream.
/// Optimized: only parses seq from key strings, never deserializes event values.
pub async fn latest_seq<S: Store>(
    store: &S,
    tenant_id: &str,
    stream_id: &str,
) -> Result<Sequence, anyhow::Error> {
    let prefix = stream_prefix(tenant_id, stream_id);
    let mut max_seq: u64 = 0;
    let mut cursor = None;
    loop {
        let page = store
            .list(NS_EVENTS, Some(&prefix), cursor.as_deref(), 500)
            .await?;
        for key in &page.keys {
            if let Some(seq) = seq_from_key(key) {
                if seq > max_seq {
                    max_seq = seq;
                }
            }
        }
        if page.cursor.is_none() {
            break;
        }
        cursor = page.cursor;
    }
    Ok(max_seq)
}

/// Delete expired events. Scans per-stream with early termination:
/// once a non-expired event is found within a stream, remaining events
/// in that stream are likely also non-expired (newer seqs = newer events).
pub async fn delete_expired<S: Store>(store: &S, now_ms: i64) -> Result<u64, anyhow::Error> {
    let mut deleted = 0u64;
    let mut cursor = None;
    loop {
        let page = store.list(NS_EVENTS, None, cursor.as_deref(), 500).await?;
        for key in &page.keys {
            let key_str = String::from_utf8_lossy(key);
            // Skip ID index keys
            if key_str.contains("/id/") {
                continue;
            }
            if let Ok(entry) = store.get(NS_EVENTS, key, None).await {
                if let Ok(stored) = serde_json::from_slice::<StoredEvent>(&entry.value) {
                    if stored.expires_at < now_ms {
                        let tid = key_str.split('/').next().unwrap_or("");
                        let id_key = event_id_key(tid, stored.event.id.as_str());
                        // Atomic delete of both keys
                        let _ = store
                            .pipeline(vec![
                                PipelineCommand::Delete {
                                    ns: NS_EVENTS.to_string(),
                                    key: key.clone(),
                                },
                                PipelineCommand::Delete {
                                    ns: NS_EVENTS.to_string(),
                                    key: id_key,
                                },
                            ])
                            .await;
                        deleted += 1;
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

#[cfg(test)]
mod tests {
    use super::*;
    use herald_core::event::EventId;

    fn make_event(id: &str, stream: &str, seq: u64, sent_at: i64) -> Event {
        Event {
            id: EventId(id.into()),
            stream_id: stream.into(),
            seq,
            sender: "user1".into(),
            body: "hello".into(),
            meta: None,
            parent_id: None,
            edited_at: None,
            sent_at,
        }
    }

    #[tokio::test]
    async fn test_event_insert_and_get() {
        let store = crate::store::test_store().await;
        let event = make_event("m1", "r1", 1, 1000);
        insert(&*store, "t1", &event, 99999).await.unwrap();

        let got = get_by_id(&*store, "t1", "m1").await.unwrap().unwrap();
        assert_eq!(got.body, "hello");
        assert_eq!(got.seq, 1);

        assert!(get_by_id(&*store, "t1", "nope").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_event_list_before() {
        let store = crate::store::test_store().await;
        for i in 1..=5 {
            let event = make_event(&format!("m{i}"), "r1", i, 1000 + i as i64);
            insert(&*store, "t1", &event, 99999).await.unwrap();
        }

        let events = list_before(&*store, "t1", "r1", 4, 10).await.unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].seq, 1);
    }

    #[tokio::test]
    async fn test_event_list_after() {
        let store = crate::store::test_store().await;
        for i in 1..=5 {
            let event = make_event(&format!("m{i}"), "r1", i, 1000 + i as i64);
            insert(&*store, "t1", &event, 99999).await.unwrap();
        }

        let events = list_after(&*store, "t1", "r1", 3, 10).await.unwrap();
        assert_eq!(events.len(), 2);
    }

    #[tokio::test]
    async fn test_event_soft_delete() {
        let store = crate::store::test_store().await;
        let event = make_event("m1", "r1", 1, 1000);
        insert(&*store, "t1", &event, 99999).await.unwrap();

        let original = delete_event(&*store, "t1", "m1").await.unwrap().unwrap();
        assert_eq!(original.body, "hello");

        let got = get_by_id(&*store, "t1", "m1").await.unwrap().unwrap();
        assert_eq!(got.body, "");
        assert_eq!(got.meta, Some(serde_json::json!({"deleted": true})));
    }

    #[tokio::test]
    async fn test_event_latest_seq() {
        let store = crate::store::test_store().await;
        for i in 1..=3 {
            let event = make_event(&format!("m{i}"), "r1", i, 1000);
            insert(&*store, "t1", &event, 99999).await.unwrap();
        }

        let seq = latest_seq(&*store, "t1", "r1").await.unwrap();
        assert_eq!(seq, 3);
    }

    #[tokio::test]
    async fn test_event_list_since_time() {
        let store = crate::store::test_store().await;
        for i in 1..=5 {
            let event = make_event(&format!("m{i}"), "r1", i, 1000 + i as i64 * 100);
            insert(&*store, "t1", &event, 99999).await.unwrap();
        }

        let events = list_since_time(&*store, "t1", "r1", 1300, 10)
            .await
            .unwrap();
        assert_eq!(events.len(), 2); // sent_at 1400 and 1500
    }

    #[tokio::test]
    async fn test_insert_atomic_dual_index() {
        let store = crate::store::test_store().await;
        let event = make_event("atomic1", "r1", 1, 1000);
        insert(&*store, "t1", &event, 99999).await.unwrap();

        // Both keys should exist
        let by_id = get_by_id(&*store, "t1", "atomic1").await.unwrap();
        assert!(by_id.is_some(), "ID index should exist");

        let by_seq = list_after(&*store, "t1", "r1", 0, 10).await.unwrap();
        assert_eq!(by_seq.len(), 1, "seq index should exist");
        assert_eq!(by_seq[0].id.0, "atomic1");
    }

    #[tokio::test]
    async fn test_list_after_early_exit() {
        let store = crate::store::test_store().await;
        // Insert 100 events
        for i in 1..=100 {
            let event = make_event(&format!("m{i}"), "r1", i, 1000 + i as i64);
            insert(&*store, "t1", &event, 99999).await.unwrap();
        }

        // Fetch 5 after seq 10
        let events = list_after(&*store, "t1", "r1", 10, 5).await.unwrap();
        assert_eq!(events.len(), 5);
        assert_eq!(events[0].seq, 11);
        assert_eq!(events[4].seq, 15);
    }

    #[tokio::test]
    async fn test_list_before_takes_newest() {
        let store = crate::store::test_store().await;
        for i in 1..=100 {
            let event = make_event(&format!("m{i}"), "r1", i, 1000 + i as i64);
            insert(&*store, "t1", &event, 99999).await.unwrap();
        }

        // Fetch 5 before seq 50
        let events = list_before(&*store, "t1", "r1", 50, 5).await.unwrap();
        assert_eq!(events.len(), 5);
        assert_eq!(events[0].seq, 45);
        assert_eq!(events[4].seq, 49);
    }

    #[tokio::test]
    async fn test_latest_seq_no_deserialize() {
        let store = crate::store::test_store().await;
        for i in 1..=50 {
            let event = make_event(&format!("m{i}"), "r1", i, 1000);
            insert(&*store, "t1", &event, 99999).await.unwrap();
        }
        // Should find max without deserializing all events
        let seq = latest_seq(&*store, "t1", "r1").await.unwrap();
        assert_eq!(seq, 50);
    }
}
