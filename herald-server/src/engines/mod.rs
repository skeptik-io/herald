#[cfg(feature = "chat")]
pub mod chat;
#[cfg(feature = "presence")]
pub mod presence;

use std::sync::Arc;

use crate::engine::{EngineSet, HeraldEngine};

/// Build an `EngineSet` with all engines enabled by the current feature set.
#[allow(clippy::vec_init_then_push, unused_mut)]
pub fn default_engines() -> EngineSet {
    let mut engines: Vec<Arc<dyn HeraldEngine>> = Vec::new();
    #[cfg(feature = "chat")]
    engines.push(Arc::new(chat::ChatEngine::new()));
    #[cfg(feature = "presence")]
    engines.push(Arc::new(presence::PresenceEngine::new()));
    EngineSet::new(engines)
}
