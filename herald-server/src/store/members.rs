use herald_core::member::{Member, Role};
use shroudb_store::Store;

use super::NS_MEMBERS;

/// Key format: "{tenant_id}/{room_id}/{user_id}"
fn member_key(tenant_id: &str, room_id: &str, user_id: &str) -> Vec<u8> {
    format!("{tenant_id}/{room_id}/{user_id}").into_bytes()
}

fn room_prefix(tenant_id: &str, room_id: &str) -> Vec<u8> {
    format!("{tenant_id}/{room_id}/").into_bytes()
}

pub async fn insert<S: Store>(
    store: &S,
    tenant_id: &str,
    member: &Member,
) -> Result<(), anyhow::Error> {
    let key = member_key(tenant_id, &member.room_id, &member.user_id);
    let value = serde_json::to_vec(member)?;
    store.put(NS_MEMBERS, &key, &value, None).await?;
    Ok(())
}

pub async fn get<S: Store>(
    store: &S,
    tenant_id: &str,
    room_id: &str,
    user_id: &str,
) -> Result<Option<Member>, anyhow::Error> {
    let key = member_key(tenant_id, room_id, user_id);
    match store.get(NS_MEMBERS, &key, None).await {
        Ok(entry) => Ok(Some(serde_json::from_slice(&entry.value)?)),
        Err(shroudb_store::StoreError::NotFound) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub async fn list_by_room<S: Store>(
    store: &S,
    tenant_id: &str,
    room_id: &str,
) -> Result<Vec<Member>, anyhow::Error> {
    let prefix = room_prefix(tenant_id, room_id);
    let mut members = Vec::new();
    let mut cursor = None;
    loop {
        let page = store
            .list(NS_MEMBERS, Some(&prefix), cursor.as_deref(), 100)
            .await?;
        for key in &page.keys {
            if let Ok(entry) = store.get(NS_MEMBERS, key, None).await {
                if let Ok(m) = serde_json::from_slice::<Member>(&entry.value) {
                    members.push(m);
                }
            }
        }
        if page.cursor.is_none() {
            break;
        }
        cursor = page.cursor;
    }
    Ok(members)
}

pub async fn delete<S: Store>(
    store: &S,
    tenant_id: &str,
    room_id: &str,
    user_id: &str,
) -> Result<bool, anyhow::Error> {
    let key = member_key(tenant_id, room_id, user_id);
    match store.delete(NS_MEMBERS, &key).await {
        Ok(_) => Ok(true),
        Err(shroudb_store::StoreError::NotFound) => Ok(false),
        Err(e) => Err(e.into()),
    }
}

pub async fn update_role<S: Store>(
    store: &S,
    tenant_id: &str,
    room_id: &str,
    user_id: &str,
    role: Role,
) -> Result<bool, anyhow::Error> {
    let key = member_key(tenant_id, room_id, user_id);
    let entry = match store.get(NS_MEMBERS, &key, None).await {
        Ok(e) => e,
        Err(shroudb_store::StoreError::NotFound) => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    let mut member: Member = serde_json::from_slice(&entry.value)?;
    member.role = role;
    let value = serde_json::to_vec(&member)?;
    store.put(NS_MEMBERS, &key, &value, None).await?;
    Ok(true)
}
