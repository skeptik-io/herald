use shroudb_chronicle_client::ChronicleClient;

use super::{AuditEvent, ChronicleError, ChronicleOps};

pub struct RemoteChronicleOps {
    addr: String,
    auth_token: Option<String>,
}

impl RemoteChronicleOps {
    pub async fn connect(addr: &str, auth_token: Option<&str>) -> Result<Self, ChronicleError> {
        let mut client = ChronicleClient::connect(addr)
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
        let mut client = ChronicleClient::connect(&self.addr)
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

        let mut client = self.fresh_client().await?;
        let mut chronicle_event = Event::new(
            Engine::ShrouDB, // Herald uses ShrouDB as the engine identifier
            event.operation,
            event.resource,
            if event.result == "success" {
                EventResult::Ok
            } else {
                EventResult::Error
            },
            event.actor,
        );
        chronicle_event.metadata = event.metadata;

        client
            .ingest(&chronicle_event)
            .await
            .map_err(|e| ChronicleError::Operation(e.to_string()))
    }
}
