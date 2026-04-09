use shroudb_chronicle_client::ChronicleClient;

use super::{AuditEvent, ChronicleError, ChronicleOps};

pub struct RemoteChronicleOps {
    addr: String,
    auth_token: Option<String>,
}

impl RemoteChronicleOps {
    pub async fn connect(addr: &str, auth_token: Option<&str>) -> Result<Self, ChronicleError> {
        let client = ChronicleClient::connect(addr)
            .await
            .map_err(|e| ChronicleError::Connection(format!("connect failed: {e}")))?;

        if let Some(token) = auth_token {
            client
                .auth(token)
                .await
                .map_err(|e| ChronicleError::Connection(format!("auth failed: {e}")))?;
        }

        client
            .health()
            .await
            .map_err(|e| ChronicleError::Connection(format!("health failed: {e}")))?;

        Ok(Self {
            addr: addr.to_string(),
            auth_token: auth_token.map(String::from),
        })
    }

    async fn fresh_client(&self) -> Result<ChronicleClient, ChronicleError> {
        let client = ChronicleClient::connect(&self.addr)
            .await
            .map_err(|e| ChronicleError::Connection(e.to_string()))?;
        if let Some(ref token) = self.auth_token {
            client
                .auth(token)
                .await
                .map_err(|e| ChronicleError::Connection(format!("auth failed: {e}")))?;
        }
        Ok(client)
    }
}

#[async_trait::async_trait]
impl ChronicleOps for RemoteChronicleOps {
    async fn ingest(&self, event: AuditEvent) -> Result<(), ChronicleError> {
        use shroudb_chronicle_core::event::{Engine, Event, EventResult};

        let client = self.fresh_client().await?;
        let mut chronicle_event = Event::new(
            Engine::Custom("herald".to_string()),
            event.operation,
            event.resource_type,
            event.resource_id,
            if event.result == "success" {
                EventResult::Ok
            } else {
                EventResult::Error
            },
            event.actor,
        );
        if let Some(tid) = event.tenant_id {
            chronicle_event.tenant_id = Some(tid);
        }
        chronicle_event.diff = event.diff;
        chronicle_event.metadata = event.metadata;

        client
            .ingest(&chronicle_event)
            .await
            .map_err(|e| ChronicleError::Operation(e.to_string()))
    }
}
