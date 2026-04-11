use shroudb_store::Store;

use super::NS_LAST_SEEN;
use crate::registry::presence::PersistedLastSeen;

/// Key: "{tenant_id}/{user_id}" → JSON-encoded PersistedLastSeen
fn last_seen_key(tenant_id: &str, user_id: &str) -> Vec<u8> {
    format!("{tenant_id}/{user_id}").into_bytes()
}

/// Save a last-seen timestamp to the WAL.
pub async fn save<S: Store>(store: &S, entry: &PersistedLastSeen) -> Result<(), anyhow::Error> {
    let key = last_seen_key(&entry.tenant_id, &entry.user_id);
    let value = serde_json::to_vec(entry)?;
    store.put(NS_LAST_SEEN, &key, &value, None).await?;
    Ok(())
}

/// Load all persisted last-seen entries from the WAL (for startup hydration).
pub async fn load_all<S: Store>(store: &S) -> Result<Vec<PersistedLastSeen>, anyhow::Error> {
    let mut entries = Vec::new();
    let mut cursor = None;
    loop {
        let page = store
            .list(NS_LAST_SEEN, None, cursor.as_deref(), 100)
            .await?;
        for key in &page.keys {
            if let Ok(entry) = store.get(NS_LAST_SEEN, key, None).await {
                if let Ok(ls) = serde_json::from_slice::<PersistedLastSeen>(&entry.value) {
                    entries.push(ls);
                }
            }
        }
        if page.cursor.is_none() {
            break;
        }
        cursor = page.cursor;
    }
    Ok(entries)
}
