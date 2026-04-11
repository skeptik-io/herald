//! Herald engine extension system.
//!
//! Engines add domain-specific logic (chat, presence, social, etc.) on top of
//! Herald's core event transport. Each engine can register HTTP routes, WebSocket
//! message handlers, connection lifecycle hooks, fanout filters, and background
//! timer tasks.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;

use crate::registry::connection::ConnId;
use crate::state::AppState;

/// Context passed to engine hooks during connection lifecycle events.
pub struct EngineConnContext {
    pub conn_id: ConnId,
    pub tenant_id: String,
    pub user_id: String,
}

/// Extension trait for Herald engines.
///
/// Engines register handlers, routes, and lifecycle hooks that compose onto the
/// core transport. Each engine is an isolated concern — it owns its own WAL
/// namespaces, background tasks, and state.
#[allow(unused_variables)]
pub trait HeraldEngine: Send + Sync {
    /// Human-readable name (for logging and config).
    fn name(&self) -> &'static str;

    /// Additional HTTP routes mounted under the tenant API (`/` prefix, behind
    /// tenant auth middleware).
    fn tenant_routes(&self, state: Arc<AppState>) -> Option<Router<Arc<AppState>>> {
        None
    }

    /// Additional HTTP routes mounted under the admin API (`/admin` prefix,
    /// behind admin auth middleware).
    fn admin_routes(&self, state: Arc<AppState>) -> Option<Router<Arc<AppState>>> {
        None
    }

    /// Called after a WebSocket connection is fully established and registered.
    fn on_connect(
        &self,
        state: &Arc<AppState>,
        ctx: &EngineConnContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(async {})
    }

    /// Called on every connection disconnect (before linger window).
    fn on_disconnect(
        &self,
        state: &Arc<AppState>,
        ctx: &EngineConnContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(async {})
    }

    /// Called when a user's last connection drops (after linger window expires
    /// with no reconnect).
    fn on_last_disconnect(
        &self,
        state: &Arc<AppState>,
        ctx: &EngineConnContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(async {})
    }

    /// Fanout filter — return `false` to suppress delivery of a message to a
    /// specific subscriber. Called per-subscriber during fanout. Default: allow
    /// all delivery.
    ///
    /// Parameters are owned strings to avoid lifetime constraints between the
    /// caller and the async future.
    fn fanout_filter(
        &self,
        state: Arc<AppState>,
        tenant_id: String,
        subscriber_id: String,
        sender_id: String,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + '_>> {
        let _ = (state, tenant_id, subscriber_id, sender_id);
        Box::pin(async { true })
    }

    /// WAL namespaces this engine needs initialized on startup.
    fn namespaces(&self) -> Vec<&'static str> {
        vec![]
    }

    /// Background timer tasks. Each entry is `(interval, task_fn)`.
    /// The task function receives the shared `AppState` and runs once per interval.
    /// Tasks are spawned after state hydration and cancelled on shutdown.
    fn timers(&self) -> Vec<TimerTask> {
        vec![]
    }
}

/// Async function type for timer tasks.
type TimerFn = Box<
    dyn Fn(Arc<AppState>) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>
        + Send
        + Sync,
>;

/// A background timer task registered by an engine.
pub struct TimerTask {
    /// Human-readable label for logging.
    pub label: &'static str,
    /// How often to run.
    pub interval: Duration,
    /// The async task body. Called repeatedly at `interval`.
    pub run: TimerFn,
}

/// Container for all registered engines. Stored in `AppState`.
pub struct EngineSet {
    engines: Vec<Arc<dyn HeraldEngine>>,
}

impl EngineSet {
    pub fn new(engines: Vec<Arc<dyn HeraldEngine>>) -> Self {
        Self { engines }
    }

    pub fn empty() -> Self {
        Self {
            engines: Vec::new(),
        }
    }

    /// Iterate over all registered engines.
    pub fn iter(&self) -> impl Iterator<Item = &Arc<dyn HeraldEngine>> {
        self.engines.iter()
    }

    /// Run all engine `on_connect` hooks.
    pub async fn on_connect(&self, state: &Arc<AppState>, ctx: &EngineConnContext) {
        for engine in &self.engines {
            engine.on_connect(state, ctx).await;
        }
    }

    /// Run all engine `on_disconnect` hooks.
    pub async fn on_disconnect(&self, state: &Arc<AppState>, ctx: &EngineConnContext) {
        for engine in &self.engines {
            engine.on_disconnect(state, ctx).await;
        }
    }

    /// Run all engine `on_last_disconnect` hooks.
    pub async fn on_last_disconnect(&self, state: &Arc<AppState>, ctx: &EngineConnContext) {
        for engine in &self.engines {
            engine.on_last_disconnect(state, ctx).await;
        }
    }

    /// Check all engine fanout filters. Returns `false` if any engine blocks
    /// delivery.
    pub async fn fanout_filter(
        &self,
        state: &Arc<AppState>,
        tenant_id: &str,
        subscriber_id: &str,
        sender_id: &str,
    ) -> bool {
        for engine in &self.engines {
            if !engine
                .fanout_filter(
                    state.clone(),
                    tenant_id.to_string(),
                    subscriber_id.to_string(),
                    sender_id.to_string(),
                )
                .await
            {
                return false;
            }
        }
        true
    }

    /// Collect all WAL namespaces from all engines.
    pub fn namespaces(&self) -> Vec<&'static str> {
        self.engines.iter().flat_map(|e| e.namespaces()).collect()
    }

    /// Collect all timer tasks from all engines.
    pub fn timers(&self) -> Vec<TimerTask> {
        self.engines.iter().flat_map(|e| e.timers()).collect()
    }

    /// Collect tenant routes from all engines.
    pub fn tenant_routes(&self, state: Arc<AppState>) -> Vec<Router<Arc<AppState>>> {
        self.engines
            .iter()
            .filter_map(|e| e.tenant_routes(state.clone()))
            .collect()
    }

    /// Collect admin routes from all engines.
    pub fn admin_routes(&self, state: Arc<AppState>) -> Vec<Router<Arc<AppState>>> {
        self.engines
            .iter()
            .filter_map(|e| e.admin_routes(state.clone()))
            .collect()
    }
}
