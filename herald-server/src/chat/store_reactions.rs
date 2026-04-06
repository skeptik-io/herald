use shroudb_store::Store;

use crate::store::NS_REACTIONS;

/// Key: "{tenant}/{stream}/{event_id}/{emoji}/{user_id}" -> "1"
fn reaction_key(
    tenant_id: &str,
    stream_id: &str,
    msg_id: &str,
    emoji: &str,
    user_id: &str,
) -> Vec<u8> {
    format!("{tenant_id}/{stream_id}/{msg_id}/{emoji}/{user_id}").into_bytes()
}

fn msg_reaction_prefix(tenant_id: &str, stream_id: &str, msg_id: &str) -> Vec<u8> {
    format!("{tenant_id}/{stream_id}/{msg_id}/").into_bytes()
}

pub async fn add<S: Store>(
    store: &S,
    tenant_id: &str,
    stream_id: &str,
    msg_id: &str,
    emoji: &str,
    user_id: &str,
) -> Result<(), anyhow::Error> {
    let key = reaction_key(tenant_id, stream_id, msg_id, emoji, user_id);
    store.put(NS_REACTIONS, &key, b"1", None).await?;
    Ok(())
}

pub async fn remove<S: Store>(
    store: &S,
    tenant_id: &str,
    stream_id: &str,
    msg_id: &str,
    emoji: &str,
    user_id: &str,
) -> Result<bool, anyhow::Error> {
    let key = reaction_key(tenant_id, stream_id, msg_id, emoji, user_id);
    match store.delete(NS_REACTIONS, &key).await {
        Ok(_) => Ok(true),
        Err(shroudb_store::StoreError::NotFound) => Ok(false),
        Err(e) => Err(e.into()),
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ReactionSummary {
    pub emoji: String,
    pub count: usize,
    pub users: Vec<String>,
}

pub async fn list_for_event<S: Store>(
    store: &S,
    tenant_id: &str,
    stream_id: &str,
    msg_id: &str,
) -> Result<Vec<ReactionSummary>, anyhow::Error> {
    let prefix = msg_reaction_prefix(tenant_id, stream_id, msg_id);
    let mut reactions: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    let mut cursor = None;
    loop {
        let page = store
            .list(NS_REACTIONS, Some(&prefix), cursor.as_deref(), 500)
            .await?;
        for key in &page.keys {
            let key_str = String::from_utf8_lossy(key);
            // key format: tenant/stream/event/emoji/user
            let parts: Vec<&str> = key_str.splitn(5, '/').collect();
            if parts.len() == 5 {
                reactions
                    .entry(parts[3].to_string())
                    .or_default()
                    .push(parts[4].to_string());
            }
        }
        if page.cursor.is_none() {
            break;
        }
        cursor = page.cursor;
    }
    Ok(reactions
        .into_iter()
        .map(|(emoji, users)| ReactionSummary {
            count: users.len(),
            emoji,
            users,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_reaction_add_remove() {
        let store = crate::store::test_store().await;
        add(&*store, "t1", "stream1", "msg1", "thumbsup", "alice")
            .await
            .unwrap();
        add(&*store, "t1", "stream1", "msg1", "thumbsup", "bob")
            .await
            .unwrap();
        add(&*store, "t1", "stream1", "msg1", "heart", "alice")
            .await
            .unwrap();

        let summaries = list_for_event(&*store, "t1", "stream1", "msg1")
            .await
            .unwrap();
        assert_eq!(summaries.len(), 2);

        let thumbsup = summaries.iter().find(|s| s.emoji == "thumbsup").unwrap();
        assert_eq!(thumbsup.count, 2);

        // Remove one
        let removed = remove(&*store, "t1", "stream1", "msg1", "thumbsup", "alice")
            .await
            .unwrap();
        assert!(removed);

        let summaries = list_for_event(&*store, "t1", "stream1", "msg1")
            .await
            .unwrap();
        let thumbsup = summaries.iter().find(|s| s.emoji == "thumbsup").unwrap();
        assert_eq!(thumbsup.count, 1);

        // Remove non-existent
        let removed = remove(&*store, "t1", "stream1", "msg1", "thumbsup", "charlie")
            .await
            .unwrap();
        assert!(!removed);
    }
}
