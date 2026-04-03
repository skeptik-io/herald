use herald_core::message::{Message, Sequence};
use shroudb_store::Store;

use super::NS_MESSAGES;

fn msg_key(tenant_id: &str, room_id: &str, seq: u64) -> Vec<u8> {
    format!("{tenant_id}/{room_id}/{seq:020}").into_bytes()
}

fn msg_id_key(tenant_id: &str, msg_id: &str) -> Vec<u8> {
    format!("{tenant_id}/id/{msg_id}").into_bytes()
}

fn room_prefix(tenant_id: &str, room_id: &str) -> Vec<u8> {
    format!("{tenant_id}/{room_id}/").into_bytes()
}

#[derive(serde::Serialize, serde::Deserialize)]
struct StoredMessage {
    #[serde(flatten)]
    msg: Message,
    expires_at: i64,
}

pub async fn insert<S: Store>(
    store: &S,
    tenant_id: &str,
    msg: &Message,
    expires_at: i64,
) -> Result<(), anyhow::Error> {
    let stored = StoredMessage {
        msg: msg.clone(),
        expires_at,
    };
    let value = serde_json::to_vec(&stored)?;
    let key = msg_key(tenant_id, &msg.room_id, msg.seq);
    store.put(NS_MESSAGES, &key, &value, None).await?;

    let id_key = msg_id_key(tenant_id, msg.id.as_str());
    store.put(NS_MESSAGES, &id_key, &key, None).await?;
    Ok(())
}

pub async fn list_before<S: Store>(
    store: &S,
    tenant_id: &str,
    room_id: &str,
    before: Sequence,
    limit: u32,
) -> Result<Vec<Message>, anyhow::Error> {
    let prefix = room_prefix(tenant_id, room_id);
    let mut all_msgs = Vec::new();
    let mut cursor = None;
    loop {
        let page = store
            .list(NS_MESSAGES, Some(&prefix), cursor.as_deref(), 500)
            .await?;
        for key in &page.keys {
            if let Ok(entry) = store.get(NS_MESSAGES, key, None).await {
                if let Ok(stored) = serde_json::from_slice::<StoredMessage>(&entry.value) {
                    if stored.msg.seq < before {
                        all_msgs.push(stored.msg);
                    }
                }
            }
        }
        if page.cursor.is_none() {
            break;
        }
        cursor = page.cursor;
    }
    all_msgs.sort_by(|a, b| b.seq.cmp(&a.seq));
    all_msgs.truncate(limit as usize);
    all_msgs.reverse();
    Ok(all_msgs)
}

pub async fn list_after<S: Store>(
    store: &S,
    tenant_id: &str,
    room_id: &str,
    after: Sequence,
    limit: u32,
) -> Result<Vec<Message>, anyhow::Error> {
    let prefix = room_prefix(tenant_id, room_id);
    let mut msgs = Vec::new();
    let mut cursor = None;
    loop {
        let page = store
            .list(NS_MESSAGES, Some(&prefix), cursor.as_deref(), 500)
            .await?;
        for key in &page.keys {
            if let Ok(entry) = store.get(NS_MESSAGES, key, None).await {
                if let Ok(stored) = serde_json::from_slice::<StoredMessage>(&entry.value) {
                    if stored.msg.seq > after {
                        msgs.push(stored.msg);
                    }
                }
            }
        }
        if page.cursor.is_none() {
            break;
        }
        cursor = page.cursor;
    }
    msgs.sort_by_key(|m| m.seq);
    msgs.truncate(limit as usize);
    Ok(msgs)
}

pub async fn list_since_time<S: Store>(
    store: &S,
    tenant_id: &str,
    room_id: &str,
    since_ms: i64,
    limit: u32,
) -> Result<Vec<Message>, anyhow::Error> {
    let prefix = room_prefix(tenant_id, room_id);
    let mut msgs = Vec::new();
    let mut cursor = None;
    loop {
        let page = store
            .list(NS_MESSAGES, Some(&prefix), cursor.as_deref(), 500)
            .await?;
        for key in &page.keys {
            if let Ok(entry) = store.get(NS_MESSAGES, key, None).await {
                if let Ok(stored) = serde_json::from_slice::<StoredMessage>(&entry.value) {
                    if stored.msg.sent_at > since_ms {
                        msgs.push(stored.msg);
                    }
                }
            }
        }
        if page.cursor.is_none() {
            break;
        }
        cursor = page.cursor;
    }
    msgs.sort_by_key(|m| m.seq);
    msgs.truncate(limit as usize);
    Ok(msgs)
}

pub async fn get_by_id<S: Store>(
    store: &S,
    tenant_id: &str,
    id: &str,
) -> Result<Option<Message>, anyhow::Error> {
    let id_key = msg_id_key(tenant_id, id);
    let seq_key = match store.get(NS_MESSAGES, &id_key, None).await {
        Ok(entry) => entry.value,
        Err(shroudb_store::StoreError::NotFound) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    match store.get(NS_MESSAGES, &seq_key, None).await {
        Ok(entry) => {
            let stored: StoredMessage = serde_json::from_slice(&entry.value)?;
            Ok(Some(stored.msg))
        }
        Err(shroudb_store::StoreError::NotFound) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub async fn delete_message<S: Store>(
    store: &S,
    tenant_id: &str,
    msg_id: &str,
) -> Result<Option<Message>, anyhow::Error> {
    // Look up by ID
    let id_key = msg_id_key(tenant_id, msg_id);
    let seq_key = match store.get(NS_MESSAGES, &id_key, None).await {
        Ok(entry) => entry.value,
        Err(shroudb_store::StoreError::NotFound) => return Ok(None),
        Err(e) => return Err(e.into()),
    };

    // Get the message
    let entry = match store.get(NS_MESSAGES, &seq_key, None).await {
        Ok(e) => e,
        Err(shroudb_store::StoreError::NotFound) => return Ok(None),
        Err(e) => return Err(e.into()),
    };

    let mut stored: StoredMessage = serde_json::from_slice(&entry.value)?;
    let original = stored.msg.clone();

    // Soft delete: clear body, set meta to indicate deletion
    stored.msg.body = String::new();
    stored.msg.meta = Some(serde_json::json!({"deleted": true}));

    let value = serde_json::to_vec(&stored)?;
    store.put(NS_MESSAGES, &seq_key, &value, None).await?;

    Ok(Some(original))
}

pub async fn latest_seq<S: Store>(
    store: &S,
    tenant_id: &str,
    room_id: &str,
) -> Result<Sequence, anyhow::Error> {
    let prefix = room_prefix(tenant_id, room_id);
    let mut max_seq: u64 = 0;
    let mut cursor = None;
    loop {
        let page = store
            .list(NS_MESSAGES, Some(&prefix), cursor.as_deref(), 500)
            .await?;
        for key in &page.keys {
            let key_str = String::from_utf8_lossy(key);
            if let Some(seq_str) = key_str.rsplit('/').next() {
                if let Ok(seq) = seq_str.parse::<u64>() {
                    max_seq = max_seq.max(seq);
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

pub async fn delete_expired<S: Store>(store: &S, now_ms: i64) -> Result<u64, anyhow::Error> {
    let mut deleted = 0u64;
    let mut cursor = None;
    loop {
        let page = store
            .list(NS_MESSAGES, None, cursor.as_deref(), 500)
            .await?;
        for key in &page.keys {
            let key_str = String::from_utf8_lossy(key);
            if key_str.contains("/id/") {
                continue;
            }
            if let Ok(entry) = store.get(NS_MESSAGES, key, None).await {
                if let Ok(stored) = serde_json::from_slice::<StoredMessage>(&entry.value) {
                    if stored.expires_at < now_ms {
                        let _ = store.delete(NS_MESSAGES, key).await;
                        let tid = key_str.split('/').next().unwrap_or("");
                        let id_key = msg_id_key(tid, stored.msg.id.as_str());
                        let _ = store.delete(NS_MESSAGES, &id_key).await;
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
    use herald_core::message::MessageId;

    fn make_msg(id: &str, room: &str, seq: u64, sent_at: i64) -> Message {
        Message {
            id: MessageId(id.into()),
            room_id: room.into(),
            seq,
            sender: "user1".into(),
            body: "hello".into(),
            meta: None,
            sent_at,
        }
    }

    #[tokio::test]
    async fn test_message_insert_and_get() {
        let store = crate::store::test_store().await;
        let msg = make_msg("m1", "r1", 1, 1000);
        insert(&*store, "t1", &msg, 99999).await.unwrap();

        let got = get_by_id(&*store, "t1", "m1").await.unwrap().unwrap();
        assert_eq!(got.body, "hello");
        assert_eq!(got.seq, 1);

        assert!(get_by_id(&*store, "t1", "nope").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_message_list_before() {
        let store = crate::store::test_store().await;
        for i in 1..=5 {
            let msg = make_msg(&format!("m{i}"), "r1", i, 1000 + i as i64);
            insert(&*store, "t1", &msg, 99999).await.unwrap();
        }

        let msgs = list_before(&*store, "t1", "r1", 4, 10).await.unwrap();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].seq, 1);
    }

    #[tokio::test]
    async fn test_message_list_after() {
        let store = crate::store::test_store().await;
        for i in 1..=5 {
            let msg = make_msg(&format!("m{i}"), "r1", i, 1000 + i as i64);
            insert(&*store, "t1", &msg, 99999).await.unwrap();
        }

        let msgs = list_after(&*store, "t1", "r1", 3, 10).await.unwrap();
        assert_eq!(msgs.len(), 2);
    }

    #[tokio::test]
    async fn test_message_soft_delete() {
        let store = crate::store::test_store().await;
        let msg = make_msg("m1", "r1", 1, 1000);
        insert(&*store, "t1", &msg, 99999).await.unwrap();

        let original = delete_message(&*store, "t1", "m1").await.unwrap().unwrap();
        assert_eq!(original.body, "hello");

        let got = get_by_id(&*store, "t1", "m1").await.unwrap().unwrap();
        assert_eq!(got.body, "");
        assert_eq!(got.meta, Some(serde_json::json!({"deleted": true})));
    }

    #[tokio::test]
    async fn test_message_latest_seq() {
        let store = crate::store::test_store().await;
        for i in 1..=3 {
            let msg = make_msg(&format!("m{i}"), "r1", i, 1000);
            insert(&*store, "t1", &msg, 99999).await.unwrap();
        }

        let seq = latest_seq(&*store, "t1", "r1").await.unwrap();
        assert_eq!(seq, 3);
    }

    #[tokio::test]
    async fn test_message_list_since_time() {
        let store = crate::store::test_store().await;
        for i in 1..=5 {
            let msg = make_msg(&format!("m{i}"), "r1", i, 1000 + i as i64 * 100);
            insert(&*store, "t1", &msg, 99999).await.unwrap();
        }

        let msgs = list_since_time(&*store, "t1", "r1", 1300, 10)
            .await
            .unwrap();
        assert_eq!(msgs.len(), 2); // sent_at 1400 and 1500
    }
}
