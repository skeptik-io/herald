use shroudb_store::Store;

use super::NS_PRESENCE_OVERRIDES;
use crate::registry::presence::PersistedOverride;

/// Key: "{tenant_id}/{user_id}" → JSON-encoded PersistedOverride
fn override_key(tenant_id: &str, user_id: &str) -> Vec<u8> {
    format!("{tenant_id}/{user_id}").into_bytes()
}

/// Save a presence override to the WAL.
pub async fn save<S: Store>(store: &S, po: &PersistedOverride) -> Result<(), anyhow::Error> {
    let key = override_key(&po.tenant_id, &po.user_id);
    let value = serde_json::to_vec(po)?;
    store.put(NS_PRESENCE_OVERRIDES, &key, &value, None).await?;
    Ok(())
}

/// Remove a presence override from the WAL.
pub async fn remove<S: Store>(
    store: &S,
    tenant_id: &str,
    user_id: &str,
) -> Result<(), anyhow::Error> {
    let key = override_key(tenant_id, user_id);
    match store.delete(NS_PRESENCE_OVERRIDES, &key).await {
        Ok(_) | Err(shroudb_store::StoreError::NotFound) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Load all persisted presence overrides from the WAL (for startup hydration).
pub async fn load_all<S: Store>(store: &S) -> Result<Vec<PersistedOverride>, anyhow::Error> {
    let mut overrides = Vec::new();
    let mut cursor = None;
    loop {
        let page = store
            .list(NS_PRESENCE_OVERRIDES, None, cursor.as_deref(), 100)
            .await?;
        for key in &page.keys {
            if let Ok(entry) = store.get(NS_PRESENCE_OVERRIDES, key, None).await {
                if let Ok(po) = serde_json::from_slice::<PersistedOverride>(&entry.value) {
                    overrides.push(po);
                }
            }
        }
        if page.cursor.is_none() {
            break;
        }
        cursor = page.cursor;
    }
    Ok(overrides)
}
