//! Embedded CipherOps — runs CipherEngine in-process, no TCP.

use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use shroudb_cipher_engine::engine::{CipherConfig, CipherEngine};
use shroudb_storage::EmbeddedStore;

use super::{CipherError, CipherOps};

pub struct EmbeddedCipherOps {
    engine: Arc<CipherEngine<EmbeddedStore>>,
}

impl EmbeddedCipherOps {
    pub async fn new(store: Arc<EmbeddedStore>) -> Result<Self, CipherError> {
        let config = CipherConfig::default();
        let engine = CipherEngine::new(store, config, None, None)
            .await
            .map_err(|e| CipherError::Operation(format!("engine init: {e}")))?;
        Ok(Self {
            engine: Arc::new(engine),
        })
    }
}

#[async_trait::async_trait]
impl CipherOps for EmbeddedCipherOps {
    async fn create_keyring(&self, name: &str) -> Result<(), CipherError> {
        let algo: shroudb_cipher_core::keyring::KeyringAlgorithm = "aes-256-gcm"
            .parse()
            .map_err(|e: String| CipherError::Operation(e))?;
        match self
            .engine
            .keyring_create(name, algo, None, None, false, None)
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("EXISTS") || msg.contains("exists") {
                    Ok(())
                } else {
                    Err(CipherError::Operation(msg))
                }
            }
        }
    }

    async fn encrypt(
        &self,
        keyring: &str,
        plaintext: &str,
        context: Option<&str>,
    ) -> Result<String, CipherError> {
        let b64 = B64.encode(plaintext.as_bytes());
        let result = self
            .engine
            .encrypt(keyring, &b64, context, None, false)
            .await
            .map_err(|e| CipherError::Operation(e.to_string()))?;
        Ok(result.ciphertext)
    }

    async fn decrypt(
        &self,
        keyring: &str,
        ciphertext: &str,
        context: Option<&str>,
    ) -> Result<String, CipherError> {
        let result = self
            .engine
            .decrypt(keyring, ciphertext, context)
            .await
            .map_err(|e| CipherError::Operation(e.to_string()))?;
        let plaintext_b64 = std::str::from_utf8(result.plaintext.as_bytes())
            .map_err(|e| CipherError::Operation(format!("plaintext not utf8: {e}")))?;
        let bytes = B64
            .decode(plaintext_b64)
            .map_err(|e| CipherError::Operation(format!("base64 decode: {e}")))?;
        String::from_utf8(bytes).map_err(|e| CipherError::Operation(format!("utf8: {e}")))
    }
}
