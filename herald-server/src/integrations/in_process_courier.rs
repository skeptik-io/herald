//! In-process CourierOps — runs CourierEngine in-process against any Store backend.
//!
//! Herald uses Courier for offline member notifications. The in-process engine
//! seeds a "default" webhook channel and delivers via the registered adapter.
//! In clustered mode, delivery dedup is handled by an atomic claim in the store
//! before dispatching — only the instance that wins the claim delivers.

use std::sync::Arc;

use shroudb_courier_core::{Channel, ChannelType, DeliveryRequest, DeliveryStatus, WebhookConfig};
use shroudb_courier_engine::engine::{CourierEngine, PolicyMode};
use shroudb_store::Store;

use super::{CourierError, CourierOps};

pub struct InProcessCourierOps<S: Store> {
    engine: Arc<CourierEngine<S>>,
    /// Store handle for claim-based delivery dedup in clustered mode.
    store: Arc<S>,
    /// Instance ID for claim-based dedup.
    instance_id: String,
}

const DEDUP_NAMESPACE: &str = "courier.dedup";

impl<S: Store> InProcessCourierOps<S> {
    pub async fn new(store: Arc<S>, instance_id: String) -> Result<Self, CourierError> {
        let engine = CourierEngine::new_with_policy_mode(
            store.clone(),
            None, // no decryptor — Herald sends plaintext notifications
            None, // no policy evaluator — Herald handles its own authorization
            None, // no Chronicle — Herald has its own audit layer
            PolicyMode::Open,
        )
        .await
        .map_err(|e| CourierError::Operation(format!("engine init: {e}")))?;

        // Seed a default webhook channel for Herald notifications.
        engine
            .seed_channel(Channel {
                name: "default".to_string(),
                channel_type: ChannelType::Webhook,
                smtp: None,
                webhook: Some(WebhookConfig {
                    default_method: None,
                    default_headers: None,
                    timeout_secs: None,
                }),
                enabled: true,
                created_at: 0,
                default_recipient: None,
            })
            .await
            .map_err(|e| CourierError::Operation(format!("seed channel: {e}")))?;

        // Create dedup namespace for clustered delivery claims.
        store
            .namespace_create(DEDUP_NAMESPACE, Default::default())
            .await
            .or_else(|e| match e {
                shroudb_store::StoreError::NamespaceExists(_) => Ok(()),
                other => Err(CourierError::Operation(format!("dedup ns: {other}"))),
            })?;

        Ok(Self {
            engine: Arc::new(engine),
            store,
            instance_id,
        })
    }

    /// Try to claim a delivery slot via atomic store operation.
    /// Returns true if this instance won the claim (should deliver).
    async fn try_claim(&self, key: &str) -> bool {
        // Use put-if-absent semantics: first writer wins.
        // Key format: "dedup:{recipient}:{subject_hash}" with short TTL.
        let claim_value = self.instance_id.as_bytes();
        match self
            .store
            .put(DEDUP_NAMESPACE, key.as_bytes(), claim_value, None)
            .await
        {
            Ok(_) => true,
            Err(_) => {
                // Key already exists — another instance claimed it.
                // Check if it was us (idempotent retry).
                match self.store.get(DEDUP_NAMESPACE, key.as_bytes(), None).await {
                    Ok(entry) => entry.value == claim_value,
                    Err(_) => false,
                }
            }
        }
    }
}

#[async_trait::async_trait]
impl<S: Store> CourierOps for InProcessCourierOps<S> {
    async fn notify(
        &self,
        _channel: &str,
        recipient: &str,
        subject: &str,
        body: &str,
    ) -> Result<(), CourierError> {
        // Clustered dedup: only one instance delivers each notification.
        let dedup_key = format!(
            "{}:{}",
            recipient,
            &format!("{:x}", md5_hash(subject, body))
        );
        if !self.try_claim(&dedup_key).await {
            tracing::debug!(recipient, "delivery claimed by another instance, skipping");
            return Ok(());
        }

        let request = DeliveryRequest {
            channel: "default".to_string(),
            recipient: recipient.to_string(),
            subject: Some(subject.to_string()),
            body: Some(body.to_string()),
            body_encrypted: None,
            content_type: None,
        };

        match self.engine.deliver(request).await {
            Ok(receipt) if receipt.status == DeliveryStatus::Delivered => Ok(()),
            Ok(receipt) => Err(CourierError::Operation(format!(
                "delivery failed: {:?}",
                receipt.error
            ))),
            Err(e) => Err(CourierError::Operation(e.to_string())),
        }
    }
}

/// Simple hash for dedup keys. Not cryptographic — just for uniqueness.
fn md5_hash(subject: &str, body: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    subject.hash(&mut hasher);
    body.hash(&mut hasher);
    hasher.finish()
}
