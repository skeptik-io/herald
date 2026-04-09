//! In-process ChronicleOps — runs ChronicleEngine in-process against any Store backend.
//!
//! Provides audit event ingestion and query capabilities. The engine handles
//! retention, rate limiting, and hash chaining internally.

use std::sync::Arc;

use shroudb_chronicle_core::event::{Engine, Event, EventResult};
use shroudb_chronicle_core::filter::EventFilter;
use shroudb_chronicle_engine::engine::{ChronicleConfig, ChronicleEngine};
use shroudb_store::Store;

use super::{
    AuditCountResult, AuditEvent, AuditEventView, AuditQuery, AuditQueryResult, ChronicleError,
    ChronicleOps,
};

pub struct InProcessChronicleOps<S: Store> {
    engine: Arc<ChronicleEngine<S>>,
}

impl<S: Store> InProcessChronicleOps<S> {
    pub async fn new(store: Arc<S>) -> Result<Self, ChronicleError> {
        Self::new_with_config(store, ChronicleConfig::default()).await
    }

    pub async fn new_with_config(
        store: Arc<S>,
        config: ChronicleConfig,
    ) -> Result<Self, ChronicleError> {
        let engine = ChronicleEngine::new(store, config)
            .await
            .map_err(|e| ChronicleError::Operation(format!("engine init: {e}")))?;
        Ok(Self {
            engine: Arc::new(engine),
        })
    }

    /// Total number of events stored.
    pub fn event_count(&self) -> usize {
        self.engine.event_count()
    }

    /// Apply retention policy (prune old events).
    pub async fn apply_retention(&self) {
        self.engine.apply_retention().await;
    }
}

fn audit_to_chronicle(event: AuditEvent) -> Event {
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
    chronicle_event.tenant_id = event.tenant_id;
    chronicle_event.diff = event.diff;
    chronicle_event.metadata = event.metadata;
    chronicle_event
}

fn chronicle_to_view(event: &Event) -> AuditEventView {
    AuditEventView {
        id: event.id.clone(),
        timestamp: event.timestamp,
        operation: event.operation.clone(),
        resource_type: event.resource_type.clone(),
        resource_id: event.resource_id.clone(),
        actor: event.actor.clone(),
        result: event.result.to_string(),
        tenant_id: event.tenant_id.clone(),
        diff: event.diff.clone(),
        metadata: event.metadata.clone(),
    }
}

fn query_to_filter(query: &AuditQuery) -> EventFilter {
    EventFilter {
        engine: Some(Engine::Custom("herald".to_string())),
        tenant_id: query.tenant_id.clone(),
        operation: query.operation.clone(),
        resource_type: query.resource_type.clone(),
        resource_id: query.resource_id.clone(),
        actor: query.actor.clone(),
        result: query
            .result
            .as_deref()
            .and_then(EventResult::from_str_loose),
        since: query.since,
        until: query.until,
        limit: query.limit,
        ..Default::default()
    }
}

#[async_trait::async_trait]
impl<S: Store> ChronicleOps for InProcessChronicleOps<S> {
    async fn ingest(&self, event: AuditEvent) -> Result<(), ChronicleError> {
        let chronicle_event = audit_to_chronicle(event);
        self.engine
            .ingest(chronicle_event)
            .await
            .map_err(|e| ChronicleError::Operation(e.to_string()))
    }

    async fn query(&self, filter: &AuditQuery) -> Option<AuditQueryResult> {
        let event_filter = query_to_filter(filter);
        let result = self.engine.query(&event_filter);
        Some(AuditQueryResult {
            matched: result.matched,
            events: result.events.iter().map(|e| chronicle_to_view(e)).collect(),
        })
    }

    async fn count(&self, filter: &AuditQuery) -> Option<AuditCountResult> {
        let event_filter = query_to_filter(filter);
        let result = self.engine.count(&event_filter);
        Some(AuditCountResult {
            count: result.count,
        })
    }
}
