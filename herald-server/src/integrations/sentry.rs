use shroudb_sentry_client::SentryClient;

use super::{SentryError, SentryOps};

pub struct RemoteSentryOps {
    addr: String,
    auth_token: Option<String>,
}

impl RemoteSentryOps {
    pub async fn connect(addr: &str, auth_token: Option<&str>) -> Result<Self, SentryError> {
        let mut client = SentryClient::connect(addr)
            .await
            .map_err(|e| SentryError::Connection(format!("connect failed: {e}")))?;

        if let Some(token) = auth_token {
            client
                .auth(token)
                .await
                .map_err(|e| SentryError::Connection(format!("auth failed: {e}")))?;
        }

        client
            .health()
            .await
            .map_err(|e| SentryError::Connection(format!("health failed: {e}")))?;

        Ok(Self {
            addr: addr.to_string(),
            auth_token: auth_token.map(String::from),
        })
    }

    async fn fresh_client(&self) -> Result<SentryClient, SentryError> {
        let mut client = SentryClient::connect(&self.addr)
            .await
            .map_err(|e| SentryError::Connection(e.to_string()))?;
        if let Some(ref token) = self.auth_token {
            client
                .auth(token)
                .await
                .map_err(|e| SentryError::Connection(format!("auth failed: {e}")))?;
        }
        Ok(client)
    }
}

#[async_trait::async_trait]
impl SentryOps for RemoteSentryOps {
    async fn evaluate(
        &self,
        subject: &str,
        action: &str,
        resource: &str,
    ) -> Result<(), SentryError> {
        let mut client = self.fresh_client().await?;
        let request = serde_json::json!({
            "subject": subject,
            "action": action,
            "resource": resource,
        });
        let result = client
            .evaluate(&request.to_string())
            .await
            .map_err(|e| SentryError::Operation(e.to_string()))?;

        if result.decision == "permit" {
            Ok(())
        } else {
            Err(SentryError::Denied(format!(
                "policy={} decision={}",
                result.matched_policy.unwrap_or_default(),
                result.decision,
            )))
        }
    }
}
