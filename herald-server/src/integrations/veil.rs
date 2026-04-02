use base64::{engine::general_purpose::STANDARD as B64, Engine};
use shroudb_veil_client::VeilClient;

use super::{SearchHit, VeilError, VeilOps};

/// VeilOps backed by a remote ShroudB Veil server over TCP.
pub struct RemoteVeilOps {
    addr: String,
    auth_token: Option<String>,
}

impl RemoteVeilOps {
    pub async fn connect(addr: &str, auth_token: Option<&str>) -> Result<Self, VeilError> {
        let mut client = VeilClient::connect(addr)
            .await
            .map_err(|e| VeilError::Connection(format!("initial connect failed: {e}")))?;

        if let Some(token) = auth_token {
            client
                .auth(token)
                .await
                .map_err(|e| VeilError::Connection(format!("auth failed: {e}")))?;
        }

        client
            .health()
            .await
            .map_err(|e| VeilError::Connection(format!("health check failed: {e}")))?;

        Ok(Self {
            addr: addr.to_string(),
            auth_token: auth_token.map(String::from),
        })
    }

    async fn fresh_client(&self) -> Result<VeilClient, VeilError> {
        let mut client = VeilClient::connect(&self.addr)
            .await
            .map_err(|e| VeilError::Connection(e.to_string()))?;

        if let Some(ref token) = self.auth_token {
            client
                .auth(token)
                .await
                .map_err(|e| VeilError::Connection(format!("auth failed: {e}")))?;
        }

        Ok(client)
    }
}

#[async_trait::async_trait]
impl VeilOps for RemoteVeilOps {
    async fn create_index(&self, name: &str) -> Result<(), VeilError> {
        let mut client = self.fresh_client().await?;
        match client.index_create(name).await {
            Ok(_) => Ok(()),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("EXISTS") || msg.contains("already exists") {
                    Ok(())
                } else {
                    Err(VeilError::Operation(msg))
                }
            }
        }
    }

    async fn put(&self, index: &str, entry_id: &str, plaintext: &str) -> Result<(), VeilError> {
        let mut client = self.fresh_client().await?;
        let b64 = B64.encode(plaintext.as_bytes());
        client
            .put(index, entry_id, &b64, None)
            .await
            .map_err(|e| VeilError::Operation(e.to_string()))?;
        Ok(())
    }

    async fn delete(&self, index: &str, entry_id: &str) -> Result<(), VeilError> {
        let mut client = self.fresh_client().await?;
        client
            .delete(index, entry_id)
            .await
            .map_err(|e| VeilError::Operation(e.to_string()))?;
        Ok(())
    }

    async fn search(
        &self,
        index: &str,
        query: &str,
        limit: Option<usize>,
    ) -> Result<Vec<SearchHit>, VeilError> {
        let mut client = self.fresh_client().await?;
        let result = client
            .search(index, query, Some("contains"), None, limit)
            .await
            .map_err(|e| VeilError::Operation(e.to_string()))?;
        Ok(result
            .results
            .into_iter()
            .map(|h| SearchHit {
                id: h.id,
                score: h.score,
            })
            .collect())
    }
}
