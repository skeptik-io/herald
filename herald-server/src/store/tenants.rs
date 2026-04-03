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
// Key format: "{token}" → value: "{tenant_id}"

pub async fn create_token<S: Store>(
    store: &S,
    token: &str,
    tenant_id: &str,
) -> Result<(), anyhow::Error> {
    store
        .put(NS_API_TOKENS, token.as_bytes(), tenant_id.as_bytes(), None)
        .await?;
    Ok(())
}

pub async fn validate_token<S: Store>(
    store: &S,
    token: &str,
) -> Result<Option<String>, anyhow::Error> {
    match store.get(NS_API_TOKENS, token.as_bytes(), None).await {
        Ok(entry) => Ok(Some(String::from_utf8(entry.value)?)),
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
            if entry.value != tenant_id.as_bytes() {
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
) -> Result<Vec<String>, anyhow::Error> {
    // Scan all tokens and filter by tenant_id
    let mut tokens = Vec::new();
    let mut cursor = None;
    loop {
        let page = store
            .list(NS_API_TOKENS, None, cursor.as_deref(), 100)
            .await?;
        for key in &page.keys {
            if let Ok(entry) = store.get(NS_API_TOKENS, key, None).await {
                if entry.value == tenant_id.as_bytes() {
                    tokens.push(String::from_utf8_lossy(key).to_string());
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
