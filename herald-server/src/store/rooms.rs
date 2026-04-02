use herald_core::room::Room;
use shroudb_store::Store;

use super::NS_ROOMS;

/// Key format: "{tenant_id}/{room_id}"
fn room_key(tenant_id: &str, room_id: &str) -> Vec<u8> {
    format!("{tenant_id}/{room_id}").into_bytes()
}

fn tenant_prefix(tenant_id: &str) -> Vec<u8> {
    format!("{tenant_id}/").into_bytes()
}

pub async fn insert<S: Store>(
    store: &S,
    tenant_id: &str,
    room: &Room,
) -> Result<(), anyhow::Error> {
    let key = room_key(tenant_id, room.id.as_str());
    let value = serde_json::to_vec(room)?;
    store.put(NS_ROOMS, &key, &value, None).await?;
    Ok(())
}

pub async fn get<S: Store>(
    store: &S,
    tenant_id: &str,
    id: &str,
) -> Result<Option<Room>, anyhow::Error> {
    let key = room_key(tenant_id, id);
    match store.get(NS_ROOMS, &key, None).await {
        Ok(entry) => Ok(Some(serde_json::from_slice(&entry.value)?)),
        Err(shroudb_store::StoreError::NotFound) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub async fn list_by_tenant<S: Store>(
    store: &S,
    tenant_id: &str,
) -> Result<Vec<Room>, anyhow::Error> {
    let prefix = tenant_prefix(tenant_id);
    let mut rooms = Vec::new();
    let mut cursor = None;
    loop {
        let page = store
            .list(NS_ROOMS, Some(&prefix), cursor.as_deref(), 100)
            .await?;
        for key in &page.keys {
            if let Ok(entry) = store.get(NS_ROOMS, key, None).await {
                if let Ok(r) = serde_json::from_slice::<Room>(&entry.value) {
                    rooms.push(r);
                }
            }
        }
        if page.cursor.is_none() {
            break;
        }
        cursor = page.cursor;
    }
    Ok(rooms)
}

/// List all rooms across all tenants (for startup hydration).
pub async fn list_all<S: Store>(store: &S) -> Result<Vec<(String, Room)>, anyhow::Error> {
    let mut results = Vec::new();
    let mut cursor = None;
    loop {
        let page = store.list(NS_ROOMS, None, cursor.as_deref(), 100).await?;
        for key in &page.keys {
            if let Ok(entry) = store.get(NS_ROOMS, key, None).await {
                let key_str = String::from_utf8_lossy(key);
                if let Some((tid, _)) = key_str.split_once('/') {
                    if let Ok(r) = serde_json::from_slice::<Room>(&entry.value) {
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
) -> Result<bool, anyhow::Error> {
    let key = room_key(tenant_id, id);
    let entry = match store.get(NS_ROOMS, &key, None).await {
        Ok(e) => e,
        Err(shroudb_store::StoreError::NotFound) => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    let mut room: Room = serde_json::from_slice(&entry.value)?;
    if let Some(n) = name {
        room.name = n.to_string();
    }
    if let Some(m) = meta {
        room.meta = Some(m.clone());
    }
    let value = serde_json::to_vec(&room)?;
    store.put(NS_ROOMS, &key, &value, None).await?;
    Ok(true)
}

pub async fn delete<S: Store>(store: &S, tenant_id: &str, id: &str) -> Result<bool, anyhow::Error> {
    let key = room_key(tenant_id, id);
    match store.delete(NS_ROOMS, &key).await {
        Ok(_) => Ok(true),
        Err(shroudb_store::StoreError::NotFound) => Ok(false),
        Err(e) => Err(e.into()),
    }
    // Note: members, messages, cursors cleanup should be handled by caller
}
