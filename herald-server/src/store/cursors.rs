use herald_core::cursor::Cursor;
use herald_core::message::Sequence;
use shroudb_store::Store;

use super::NS_CURSORS;

fn cursor_key(tenant_id: &str, room_id: &str, user_id: &str) -> Vec<u8> {
    format!("{tenant_id}/{room_id}/{user_id}").into_bytes()
}

fn room_prefix(tenant_id: &str, room_id: &str) -> Vec<u8> {
    format!("{tenant_id}/{room_id}/").into_bytes()
}

pub async fn upsert<S: Store>(
    store: &S,
    tenant_id: &str,
    room_id: &str,
    user_id: &str,
    seq: Sequence,
    now_ms: i64,
) -> Result<(), anyhow::Error> {
    let key = cursor_key(tenant_id, room_id, user_id);

    // Read current to implement MAX semantics
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
        room_id: room_id.to_string(),
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
    room_id: &str,
    user_id: &str,
) -> Result<Sequence, anyhow::Error> {
    let key = cursor_key(tenant_id, room_id, user_id);
    match store.get(NS_CURSORS, &key, None).await {
        Ok(entry) => {
            let c: Cursor = serde_json::from_slice(&entry.value)?;
            Ok(c.seq)
        }
        Err(shroudb_store::StoreError::NotFound) => Ok(0),
        Err(e) => Err(e.into()),
    }
}

pub async fn list_by_room<S: Store>(
    store: &S,
    tenant_id: &str,
    room_id: &str,
) -> Result<Vec<Cursor>, anyhow::Error> {
    let prefix = room_prefix(tenant_id, room_id);
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
