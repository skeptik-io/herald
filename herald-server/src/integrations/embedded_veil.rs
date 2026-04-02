//! Embedded VeilOps — runs VeilEngine in-process, no TCP.

use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use shroudb_storage::EmbeddedStore;
use shroudb_veil_core::matching::MatchMode;
use shroudb_veil_engine::engine::{VeilConfig, VeilEngine};

use super::{SearchHit, VeilError, VeilOps};

pub struct EmbeddedVeilOps {
    engine: Arc<VeilEngine<EmbeddedStore>>,
}

impl EmbeddedVeilOps {
    pub async fn new(store: Arc<EmbeddedStore>) -> Result<Self, VeilError> {
        let config = VeilConfig::default();
        let engine = VeilEngine::new(store, config, None, None)
            .await
            .map_err(|e| VeilError::Operation(format!("engine init: {e}")))?;
        Ok(Self {
            engine: Arc::new(engine),
        })
    }
}

#[async_trait::async_trait]
impl VeilOps for EmbeddedVeilOps {
    async fn create_index(&self, name: &str) -> Result<(), VeilError> {
        match self.engine.index_create(name).await {
            Ok(_) => Ok(()),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("EXISTS") || msg.contains("exists") {
                    Ok(())
                } else {
                    Err(VeilError::Operation(msg))
                }
            }
        }
    }

    async fn put(&self, index: &str, entry_id: &str, plaintext: &str) -> Result<(), VeilError> {
        let b64 = B64.encode(plaintext.as_bytes());
        self.engine
            .put(index, entry_id, &b64, None)
            .await
            .map_err(|e| VeilError::Operation(e.to_string()))?;
        Ok(())
    }

    async fn delete(&self, index: &str, entry_id: &str) -> Result<(), VeilError> {
        self.engine
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
        let result = self
            .engine
            .search(index, query, MatchMode::Contains, None, limit)
            .await
            .map_err(|e| VeilError::Operation(e.to_string()))?;
        Ok(result
            .hits
            .into_iter()
            .map(|h| SearchHit {
                id: h.id,
                score: h.score,
            })
            .collect())
    }
}
