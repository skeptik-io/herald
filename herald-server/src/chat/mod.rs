//! Chat extension module — conversational semantics layered on top of Herald's
//! core event transport.  Gated behind the `chat` Cargo feature (enabled by
//! default).  Disabling the feature strips typing indicators, read cursors,
//! reactions, user blocks, event edit/delete, and the related HTTP/WS handlers
//! from the server.

pub mod http_blocks;
pub mod http_events_ext;
pub mod http_presence;
pub mod store_blocks;
pub mod store_cursors;
pub mod store_reactions;
pub mod ws_handler;
