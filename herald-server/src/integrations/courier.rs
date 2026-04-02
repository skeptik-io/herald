use shroudb_courier_client::CourierClient;

use super::{CourierError, CourierOps};

pub struct RemoteCourierOps {
    addr: String,
    auth_token: Option<String>,
    channel: String,
}

impl RemoteCourierOps {
    pub async fn connect(
        addr: &str,
        auth_token: Option<&str>,
        channel: &str,
    ) -> Result<Self, CourierError> {
        let mut client = CourierClient::connect(addr)
            .await
            .map_err(|e| CourierError::Connection(format!("connect failed: {e}")))?;

        if let Some(token) = auth_token {
            client
                .auth(token)
                .await
                .map_err(|e| CourierError::Connection(format!("auth failed: {e}")))?;
        }

        Ok(Self {
            addr: addr.to_string(),
            auth_token: auth_token.map(String::from),
            channel: channel.to_string(),
        })
    }

    async fn fresh_client(&self) -> Result<CourierClient, CourierError> {
        let mut client = CourierClient::connect(&self.addr)
            .await
            .map_err(|e| CourierError::Connection(e.to_string()))?;
        if let Some(ref token) = self.auth_token {
            client
                .auth(token)
                .await
                .map_err(|e| CourierError::Connection(format!("auth failed: {e}")))?;
        }
        Ok(client)
    }
}

#[async_trait::async_trait]
impl CourierOps for RemoteCourierOps {
    async fn notify(
        &self,
        _channel: &str,
        recipient: &str,
        subject: &str,
        body: &str,
    ) -> Result<(), CourierError> {
        let mut client = self.fresh_client().await?;
        let request = serde_json::json!({
            "channel": self.channel,
            "recipient": recipient,
            "subject": subject,
            "body": body,
        });
        client
            .deliver(&request.to_string())
            .await
            .map_err(|e| CourierError::Operation(e.to_string()))?;
        Ok(())
    }
}
