use base64::{engine::general_purpose::STANDARD as B64, Engine};
use shroudb_cipher_client::CipherClient;

use super::{CipherError, CipherOps};

/// CipherOps backed by a remote ShroudB Cipher server over TCP.
///
/// Creates a fresh connection per call (same pattern as Sigil's RemoteCipherOps).
/// This avoids serializing concurrent operations through a single connection.
pub struct RemoteCipherOps {
    addr: String,
    auth_token: Option<String>,
}

impl RemoteCipherOps {
    /// Connect to verify the address is reachable, then store params for per-call connections.
    pub async fn connect(addr: &str, auth_token: Option<&str>) -> Result<Self, CipherError> {
        let mut client = CipherClient::connect(addr)
            .await
            .map_err(|e| CipherError::Connection(format!("initial connect failed: {e}")))?;

        if let Some(token) = auth_token {
            client
                .auth(token)
                .await
                .map_err(|e| CipherError::Connection(format!("auth failed: {e}")))?;
        }

        client
            .health()
            .await
            .map_err(|e| CipherError::Connection(format!("health check failed: {e}")))?;

        Ok(Self {
            addr: addr.to_string(),
            auth_token: auth_token.map(String::from),
        })
    }

    async fn fresh_client(&self) -> Result<CipherClient, CipherError> {
        let mut client = CipherClient::connect(&self.addr)
            .await
            .map_err(|e| CipherError::Connection(e.to_string()))?;

        if let Some(ref token) = self.auth_token {
            client
                .auth(token)
                .await
                .map_err(|e| CipherError::Connection(format!("auth failed: {e}")))?;
        }

        Ok(client)
    }
}

#[async_trait::async_trait]
impl CipherOps for RemoteCipherOps {
    async fn create_keyring(&self, name: &str) -> Result<(), CipherError> {
        let mut client = self.fresh_client().await?;
        match client
            .keyring_create(name, "aes-256-gcm", None, None, false)
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => {
                let msg = e.to_string();
                // Idempotent: keyring already exists is not an error
                if msg.contains("EXISTS") || msg.contains("already exists") {
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
        let mut client = self.fresh_client().await?;
        let b64 = B64.encode(plaintext.as_bytes());
        let result = client
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
        let mut client = self.fresh_client().await?;
        let result = client
            .decrypt(keyring, ciphertext, context)
            .await
            .map_err(|e| CipherError::Operation(e.to_string()))?;
        let bytes = B64
            .decode(&result.plaintext)
            .map_err(|e| CipherError::Operation(format!("base64 decode failed: {e}")))?;
        String::from_utf8(bytes)
            .map_err(|e| CipherError::Operation(format!("utf8 decode failed: {e}")))
    }
}
