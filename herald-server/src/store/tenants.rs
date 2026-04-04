use shroudb_store::Store;

use super::{NS_API_TOKENS, NS_TENANTS};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Tenant {
    pub id: String,
    pub name: String,
    pub jwt_secret: String,
    pub jwt_issuer: Option<String>,
    pub plan: String,
    pub config: serde_json::Value,
    pub created_at: i64,
}

pub async fn insert<S: Store>(store: &S, tenant: &Tenant) -> Result<(), anyhow::Error> {
    let value = serde_json::to_vec(tenant)?;
    store
        .put(NS_TENANTS, tenant.id.as_bytes(), &value, None)
        .await?;
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

pub async fn delete<S: Store>(store: &S, id: &str) -> Result<bool, anyhow::Error> {
    match store.delete(NS_TENANTS, id.as_bytes()).await {
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
    pub scope: Option<String>, // "read-only", "stream:chat", etc. None = full access
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
            // Try JSON format first (new), fall back to plain tenant_id (legacy)
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
    // Verify token belongs to this tenant
    match store.get(NS_API_TOKENS, token.as_bytes(), None).await {
        Ok(entry) => {
            // Check tenant ownership — handle both JSON and legacy formats
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
    // Scan all tokens and filter by tenant_id
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

    #[tokio::test]
    async fn test_tenant_crud() {
        let store = crate::store::test_store().await;
        let tenant = Tenant {
            id: "t1".to_string(),
            name: "Test".to_string(),
            jwt_secret: "secret".to_string(),
            jwt_issuer: None,
            plan: "free".to_string(),
            config: serde_json::json!({}),
            created_at: 1000,
        };
        insert(&*store, &tenant).await.unwrap();

        let got = get(&*store, "t1").await.unwrap().unwrap();
        assert_eq!(got.id, "t1");
        assert_eq!(got.name, "Test");

        assert!(get(&*store, "nope").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_tenant_list() {
        let store = crate::store::test_store().await;
        for i in 0..3 {
            let t = Tenant {
                id: format!("t{i}"),
                name: format!("T{i}"),
                jwt_secret: "s".into(),
                jwt_issuer: None,
                plan: "free".into(),
                config: serde_json::json!({}),
                created_at: 1000,
            };
            insert(&*store, &t).await.unwrap();
        }
        let all = list(&*store).await.unwrap();
        assert_eq!(all.len(), 3);
    }

    #[tokio::test]
    async fn test_tenant_update() {
        let store = crate::store::test_store().await;
        let t = Tenant {
            id: "t1".into(),
            name: "Old".into(),
            jwt_secret: "s".into(),
            jwt_issuer: None,
            plan: "free".into(),
            config: serde_json::json!({}),
            created_at: 1000,
        };
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
        let t = Tenant {
            id: "t1".into(),
            name: "T".into(),
            jwt_secret: "s".into(),
            jwt_issuer: None,
            plan: "free".into(),
            config: serde_json::json!({}),
            created_at: 1000,
        };
        insert(&*store, &t).await.unwrap();

        assert!(delete(&*store, "t1").await.unwrap());
        assert!(get(&*store, "t1").await.unwrap().is_none());
        assert!(!delete(&*store, "t1").await.unwrap());
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
