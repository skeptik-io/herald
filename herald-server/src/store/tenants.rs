use shroudb_store::Store;

use super::{NS_API_TOKENS, NS_TENANTS, NS_TENANT_KEYS};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Tenant {
    pub id: String,
    pub name: String,
    pub key: String,
    pub secret: String,
    pub plan: String,
    pub config: serde_json::Value,
    pub created_at: i64,
}

/// Generate a (key, secret) credential pair for a new tenant.
///
/// Key: 24-char hex (12 bytes of randomness) — public identifier.
/// Secret: 64-char hex (32 bytes of randomness) — HMAC signing key.
pub fn generate_tenant_credentials() -> (String, String) {
    let u1 = uuid::Uuid::new_v4();
    let u2 = uuid::Uuid::new_v4();
    let key = hex::encode(u1.as_bytes())[..24].to_string();
    let secret = format!(
        "{}{}",
        hex::encode(u1.as_bytes()),
        hex::encode(u2.as_bytes())
    );
    (key, secret)
}

pub async fn insert<S: Store>(store: &S, tenant: &Tenant) -> Result<(), anyhow::Error> {
    let value = serde_json::to_vec(tenant)?;
    store
        .put(NS_TENANTS, tenant.id.as_bytes(), &value, None)
        .await?;
    insert_key_mapping(store, &tenant.key, &tenant.id).await?;
    Ok(())
}

pub async fn get<S: Store>(store: &S, id: &str) -> Result<Option<Tenant>, anyhow::Error> {
    match store.get(NS_TENANTS, id.as_bytes(), None).await {
        Ok(entry) => Ok(Some(serde_json::from_slice(&entry.value)?)),
        Err(shroudb_store::StoreError::NotFound) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub async fn list<S: Store>(store: &S) -> Result<Vec<Tenant>, anyhow::Error> {
    let mut tenants = Vec::new();
    let mut cursor = None;
    loop {
        let page = store.list(NS_TENANTS, None, cursor.as_deref(), 100).await?;
        for key in &page.keys {
            if let Ok(entry) = store.get(NS_TENANTS, key, None).await {
                if let Ok(t) = serde_json::from_slice::<Tenant>(&entry.value) {
                    tenants.push(t);
                }
            }
        }
        if page.cursor.is_none() {
            break;
        }
        cursor = page.cursor;
    }
    Ok(tenants)
}

pub async fn update<S: Store>(
    store: &S,
    id: &str,
    name: Option<&str>,
    plan: Option<&str>,
    config: Option<&serde_json::Value>,
) -> Result<bool, anyhow::Error> {
    let entry = match store.get(NS_TENANTS, id.as_bytes(), None).await {
        Ok(e) => e,
        Err(shroudb_store::StoreError::NotFound) => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    let mut tenant: Tenant = serde_json::from_slice(&entry.value)?;
    if let Some(n) = name {
        tenant.name = n.to_string();
    }
    if let Some(p) = plan {
        tenant.plan = p.to_string();
    }
    if let Some(c) = config {
        tenant.config = c.clone();
    }
    let value = serde_json::to_vec(&tenant)?;
    store.put(NS_TENANTS, id.as_bytes(), &value, None).await?;
    Ok(true)
}

/// Rotate a tenant's secret. Returns the new (key, secret) on success.
pub async fn rotate_secret<S: Store>(
    store: &S,
    id: &str,
) -> Result<Option<(String, String)>, anyhow::Error> {
    let entry = match store.get(NS_TENANTS, id.as_bytes(), None).await {
        Ok(e) => e,
        Err(shroudb_store::StoreError::NotFound) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let mut tenant: Tenant = serde_json::from_slice(&entry.value)?;
    let u1 = uuid::Uuid::new_v4();
    let u2 = uuid::Uuid::new_v4();
    tenant.secret = format!(
        "{}{}",
        hex::encode(u1.as_bytes()),
        hex::encode(u2.as_bytes())
    );
    let value = serde_json::to_vec(&tenant)?;
    store.put(NS_TENANTS, id.as_bytes(), &value, None).await?;
    Ok(Some((tenant.key.clone(), tenant.secret)))
}

pub async fn delete<S: Store>(
    store: &S,
    id: &str,
    key: Option<&str>,
) -> Result<bool, anyhow::Error> {
    match store.delete(NS_TENANTS, id.as_bytes()).await {
        Ok(_) => {
            if let Some(k) = key {
                let _ = delete_key_mapping(store, k).await;
            }
            Ok(true)
        }
        Err(shroudb_store::StoreError::NotFound) => Ok(false),
        Err(e) => Err(e.into()),
    }
}

// --- Key mappings (NS_TENANT_KEYS: key → tenant_id) ---

pub async fn insert_key_mapping<S: Store>(
    store: &S,
    key: &str,
    tenant_id: &str,
) -> Result<(), anyhow::Error> {
    store
        .put(NS_TENANT_KEYS, key.as_bytes(), tenant_id.as_bytes(), None)
        .await?;
    Ok(())
}

pub async fn lookup_key<S: Store>(store: &S, key: &str) -> Result<Option<String>, anyhow::Error> {
    match store.get(NS_TENANT_KEYS, key.as_bytes(), None).await {
        Ok(entry) => Ok(Some(String::from_utf8(entry.value)?)),
        Err(shroudb_store::StoreError::NotFound) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub async fn delete_key_mapping<S: Store>(store: &S, key: &str) -> Result<bool, anyhow::Error> {
    match store.delete(NS_TENANT_KEYS, key.as_bytes()).await {
        Ok(_) => Ok(true),
        Err(shroudb_store::StoreError::NotFound) => Ok(false),
        Err(e) => Err(e.into()),
    }
}

// --- API tokens ---
// Key format: "{token}" → value: JSON ApiToken (or legacy plain tenant_id)

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ApiToken {
    pub tenant_id: String,
    #[serde(default)]
    pub scope: Option<String>,
}

pub async fn create_token<S: Store>(
    store: &S,
    token: &str,
    tenant_id: &str,
    scope: Option<&str>,
) -> Result<(), anyhow::Error> {
    let api_token = ApiToken {
        tenant_id: tenant_id.to_string(),
        scope: scope.map(String::from),
    };
    let value = serde_json::to_vec(&api_token)?;
    store
        .put(NS_API_TOKENS, token.as_bytes(), &value, None)
        .await?;
    Ok(())
}

pub async fn validate_token<S: Store>(
    store: &S,
    token: &str,
) -> Result<Option<ApiToken>, anyhow::Error> {
    match store.get(NS_API_TOKENS, token.as_bytes(), None).await {
        Ok(entry) => {
            if let Ok(api_token) = serde_json::from_slice::<ApiToken>(&entry.value) {
                Ok(Some(api_token))
            } else {
                let tenant_id = String::from_utf8(entry.value)?;
                Ok(Some(ApiToken {
                    tenant_id,
                    scope: None,
                }))
            }
        }
        Err(shroudb_store::StoreError::NotFound) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub async fn delete_token<S: Store>(
    store: &S,
    token: &str,
    tenant_id: &str,
) -> Result<bool, anyhow::Error> {
    match store.get(NS_API_TOKENS, token.as_bytes(), None).await {
        Ok(entry) => {
            let owner = if let Ok(api_token) = serde_json::from_slice::<ApiToken>(&entry.value) {
                api_token.tenant_id
            } else {
                String::from_utf8(entry.value)?
            };
            if owner != tenant_id {
                return Ok(false);
            }
            store.delete(NS_API_TOKENS, token.as_bytes()).await?;
            Ok(true)
        }
        Err(shroudb_store::StoreError::NotFound) => Ok(false),
        Err(e) => Err(e.into()),
    }
}

pub async fn list_tokens<S: Store>(
    store: &S,
    tenant_id: &str,
) -> Result<Vec<serde_json::Value>, anyhow::Error> {
    let mut tokens = Vec::new();
    let mut cursor = None;
    loop {
        let page = store
            .list(NS_API_TOKENS, None, cursor.as_deref(), 100)
            .await?;
        for key in &page.keys {
            if let Ok(entry) = store.get(NS_API_TOKENS, key, None).await {
                let (tid, scope) =
                    if let Ok(api_token) = serde_json::from_slice::<ApiToken>(&entry.value) {
                        (api_token.tenant_id, api_token.scope)
                    } else {
                        (String::from_utf8_lossy(&entry.value).to_string(), None)
                    };
                if tid == tenant_id {
                    tokens.push(serde_json::json!({
                        "token": String::from_utf8_lossy(key),
                        "scope": scope,
                    }));
                }
            }
        }
        if page.cursor.is_none() {
            break;
        }
        cursor = page.cursor;
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tenant(id: &str, name: &str) -> Tenant {
        let (key, secret) = generate_tenant_credentials();
        Tenant {
            id: id.to_string(),
            name: name.to_string(),
            key,
            secret,
            plan: "free".to_string(),
            config: serde_json::json!({}),
            created_at: 1000,
        }
    }

    #[tokio::test]
    async fn test_tenant_crud() {
        let store = crate::store::test_store().await;
        let tenant = make_tenant("t1", "Test");
        insert(&*store, &tenant).await.unwrap();

        let got = get(&*store, "t1").await.unwrap().unwrap();
        assert_eq!(got.id, "t1");
        assert_eq!(got.name, "Test");
        assert_eq!(got.key, tenant.key);

        let mapped = lookup_key(&*store, &tenant.key).await.unwrap().unwrap();
        assert_eq!(mapped, "t1");

        assert!(get(&*store, "nope").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_tenant_list() {
        let store = crate::store::test_store().await;
        for i in 0..3 {
            let t = make_tenant(&format!("t{i}"), &format!("T{i}"));
            insert(&*store, &t).await.unwrap();
        }
        let all = list(&*store).await.unwrap();
        assert_eq!(all.len(), 3);
    }

    #[tokio::test]
    async fn test_tenant_update() {
        let store = crate::store::test_store().await;
        let t = make_tenant("t1", "Old");
        insert(&*store, &t).await.unwrap();

        assert!(update(&*store, "t1", Some("New"), None, None)
            .await
            .unwrap());
        let got = get(&*store, "t1").await.unwrap().unwrap();
        assert_eq!(got.name, "New");

        assert!(!update(&*store, "nope", Some("X"), None, None)
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn test_tenant_delete() {
        let store = crate::store::test_store().await;
        let t = make_tenant("t1", "T");
        let key = t.key.clone();
        insert(&*store, &t).await.unwrap();

        assert!(delete(&*store, "t1", Some(&key)).await.unwrap());
        assert!(get(&*store, "t1").await.unwrap().is_none());
        assert!(lookup_key(&*store, &key).await.unwrap().is_none());
        assert!(!delete(&*store, "t1", None).await.unwrap());
    }

    #[tokio::test]
    async fn test_rotate_secret() {
        let store = crate::store::test_store().await;
        let t = make_tenant("t1", "T");
        let old_secret = t.secret.clone();
        insert(&*store, &t).await.unwrap();

        let (key, new_secret) = rotate_secret(&*store, "t1").await.unwrap().unwrap();
        assert_eq!(key, t.key);
        assert_ne!(new_secret, old_secret);
        assert_eq!(new_secret.len(), 64);

        let got = get(&*store, "t1").await.unwrap().unwrap();
        assert_eq!(got.secret, new_secret);

        assert!(rotate_secret(&*store, "nope").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_generate_credentials() {
        let (key, secret) = generate_tenant_credentials();
        assert_eq!(key.len(), 24);
        assert_eq!(secret.len(), 64);
        hex::decode(&key).unwrap();
        hex::decode(&secret).unwrap();
        let (key2, secret2) = generate_tenant_credentials();
        assert_ne!(key, key2);
        assert_ne!(secret, secret2);
    }

    #[tokio::test]
    async fn test_api_tokens() {
        let store = crate::store::test_store().await;
        create_token(&*store, "tok1", "t1", None).await.unwrap();
        create_token(&*store, "tok2", "t1", Some("read-only"))
            .await
            .unwrap();
        create_token(&*store, "tok3", "t2", None).await.unwrap();

        let v = validate_token(&*store, "tok1").await.unwrap().unwrap();
        assert_eq!(v.tenant_id, "t1");
        assert!(v.scope.is_none());

        let v = validate_token(&*store, "tok2").await.unwrap().unwrap();
        assert_eq!(v.scope, Some("read-only".to_string()));

        assert!(validate_token(&*store, "missing").await.unwrap().is_none());

        let tokens = list_tokens(&*store, "t1").await.unwrap();
        assert_eq!(tokens.len(), 2);

        assert!(delete_token(&*store, "tok1", "t1").await.unwrap());
        assert!(!delete_token(&*store, "tok1", "t1").await.unwrap());
        assert!(!delete_token(&*store, "tok3", "t1").await.unwrap());
    }
}
