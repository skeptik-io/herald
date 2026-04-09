//! Cross-instance fanout via ShroudB store subscriptions.
//!
//! When clustering is enabled, a background task subscribes to the
//! `herald.events` namespace. On each new event write (from any instance),
//! the consumer checks whether the write originated locally. If not, it
//! reads the event from the shared store and fans it out to local subscribers.

use std::sync::Arc;

use herald_core::protocol::{EventNewPayload, ServerMessage};
use shroudb_store::{EventType, Store, Subscription, SubscriptionFilter};

use crate::store_backend::BackendSubscription;

use crate::state::AppState;
use crate::store;
use crate::ws::fanout::fanout_to_stream;

/// Spawn the backplane consumer task. Returns the `JoinHandle`.
///
/// This task subscribes to all Put events in the `herald.events` namespace
/// and re-fans out events that were written by other instances.
pub fn spawn(state: Arc<AppState>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            if let Err(e) = run(&state).await {
                tracing::error!("backplane subscription error: {e}, reconnecting in 2s");
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }
    })
}

async fn run(state: &Arc<AppState>) -> Result<(), anyhow::Error> {
    let filter = SubscriptionFilter {
        key: None, // all keys in the namespace
        events: vec![EventType::Put],
    };

    let mut sub: BackendSubscription = state
        .db
        .subscribe(store::NS_EVENTS, filter)
        .await
        .map_err(|e| anyhow::anyhow!("subscribe failed: {e}"))?;

    tracing::info!(
        instance = %state.instance_id,
        "backplane: subscribed to {} for cross-instance fanout",
        store::NS_EVENTS
    );

    while let Some(event) = sub.recv().await {
        // Only handle Put events (new/edited events)
        if event.event != EventType::Put {
            continue;
        }

        // Parse the key to extract tenant, stream, seq
        let Some((tenant_id, stream_id, _seq)) = store::events::parse_event_key(&event.key) else {
            continue; // ID index key or unparseable — skip
        };

        // Check if this write originated from this instance
        if state.consume_local_write(&event.key) {
            continue; // We already fanned out locally
        }

        // Remote event: read from shared store and fan out to local subscribers
        let herald_event = match store::events::get_by_raw_key(&*state.db, &event.key).await {
            Ok(Some(e)) => e,
            Ok(None) => {
                tracing::debug!(
                    tenant = %tenant_id,
                    stream = %stream_id,
                    "backplane: event already deleted before read"
                );
                continue;
            }
            Err(e) => {
                tracing::warn!(
                    tenant = %tenant_id,
                    stream = %stream_id,
                    "backplane: failed to read event: {e}"
                );
                continue;
            }
        };

        // Only fan out if we have local subscribers for this stream
        if state.streams.subscriber_count(&tenant_id, &stream_id) == 0 {
            continue;
        }

        let msg = ServerMessage::EventNew {
            payload: EventNewPayload {
                stream: herald_event.stream_id.clone(),
                id: herald_event.id.0.clone(),
                seq: herald_event.seq,
                sender: herald_event.sender.clone(),
                body: herald_event.body.clone(),
                meta: herald_event.meta.clone(),
                parent_id: herald_event.parent_id.clone(),
                sent_at: herald_event.sent_at,
            },
        };

        fanout_to_stream(
            state,
            &tenant_id,
            &stream_id,
            &msg,
            None,
            Some(&herald_event.sender),
        )
        .await;

        // Update cache channel for new subscribers
        state.streams.set_last_event(&tenant_id, &stream_id, msg);

        tracing::debug!(
            tenant = %tenant_id,
            stream = %stream_id,
            event_id = %herald_event.id.0,
            "backplane: re-fanned out remote event"
        );
    }

    // Subscription closed — will be retried by the outer loop
    Err(anyhow::anyhow!("subscription stream ended"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use shroudb_store::Store;

    #[tokio::test]
    async fn test_cross_store_subscription() {
        // Two EmbeddedStore instances sharing the same engine
        let dir = tempfile::tempdir().unwrap();
        let config = shroudb_storage::StorageEngineConfig {
            data_dir: dir.keep(),
            ..Default::default()
        };
        let engine = std::sync::Arc::new(
            shroudb_storage::StorageEngine::open(config, &shroudb_storage::EphemeralKey)
                .await
                .unwrap(),
        );

        let store_a = shroudb_storage::EmbeddedStore::new(engine.clone(), "a");
        let store_b = shroudb_storage::EmbeddedStore::new(engine.clone(), "b");

        // Create namespace on store_a
        store_a
            .namespace_create(
                crate::store::NS_EVENTS,
                shroudb_store::NamespaceConfig::default(),
            )
            .await
            .unwrap();

        // Subscribe on store_b
        let filter = SubscriptionFilter {
            key: None,
            events: vec![EventType::Put],
        };
        let mut sub: BackendSubscription = crate::store_backend::StoreBackend::Embedded(store_b)
            .subscribe(crate::store::NS_EVENTS, filter)
            .await
            .unwrap();

        // Write from store_a
        let key = b"t1/s1/00000000000000000001".to_vec();
        store_a
            .put(crate::store::NS_EVENTS, &key, b"test-value", None)
            .await
            .unwrap();

        // store_b's subscription should receive the event
        let event = tokio::time::timeout(std::time::Duration::from_secs(2), sub.recv())
            .await
            .expect("cross-store subscription should receive within 2s")
            .expect("should not be None");

        assert_eq!(event.key, key);
    }

    #[tokio::test]
    async fn test_subscription_receives_events() {
        let store = crate::store::test_store().await;

        // Subscribe to herald.events
        let filter = SubscriptionFilter {
            key: None,
            events: vec![EventType::Put],
        };
        let mut sub: BackendSubscription = store
            .subscribe(crate::store::NS_EVENTS, filter)
            .await
            .unwrap();

        // Write an event
        let key = b"t1/s1/00000000000000000001".to_vec();
        store
            .put(crate::store::NS_EVENTS, &key, b"test-value", None)
            .await
            .unwrap();

        // Should receive the subscription event
        let event = tokio::time::timeout(std::time::Duration::from_secs(2), sub.recv())
            .await
            .expect("should receive within 2s")
            .expect("should not be None");

        assert_eq!(event.key, key);
        assert_eq!(event.namespace, crate::store::NS_EVENTS);
    }
}
