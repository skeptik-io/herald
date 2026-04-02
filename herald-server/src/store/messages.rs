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
