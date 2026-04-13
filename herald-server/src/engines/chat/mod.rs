use std::sync::Arc;
use std::time::Duration;

use axum::routing::{delete, get, post};
use axum::Router;

use crate::engine::{EngineConnContext, HeraldEngine, TimerTask};
use crate::state::AppState;

pub mod http_blocks;
pub mod ws_handler;

pub struct ChatEngine;

impl Default for ChatEngine {
    fn default() -> Self {
        Self
    }
}

impl ChatEngine {
    pub fn new() -> Self {
        Self
    }
}

impl HeraldEngine for ChatEngine {
    fn name(&self) -> &'static str {
        "chat"
    }

    fn tenant_routes(&self, _state: Arc<AppState>) -> Option<Router<Arc<AppState>>> {
        let router = Router::new()
            .route("/blocks", post(http_blocks::block_user))
            .route("/blocks", delete(http_blocks::unblock_user))
            .route("/blocks/{user_id}", get(http_blocks::list_blocked));
        Some(router)
    }

    fn on_disconnect(
        &self,
        state: &Arc<AppState>,
        ctx: &EngineConnContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        let state = state.clone();
        let tenant_id = ctx.tenant_id.clone();
        let user_id = ctx.user_id.clone();
        Box::pin(async move {
            // Typing cleanup — broadcast stop for any streams this user was typing in
            let typing_streams = state.typing.remove_user(&tenant_id, &user_id);
            for stream_id in typing_streams {
                let msg = herald_core::protocol::ServerMessage::Typing {
                    payload: herald_core::protocol::TypingPayload {
                        stream: stream_id.clone(),
                        user_id: user_id.clone(),
                        active: false,
                    },
                };
                crate::ws::fanout::fanout_to_stream(
                    &state,
                    &tenant_id,
                    &stream_id,
                    &msg,
                    None,
                    Some(&user_id),
                )
                .await;
            }
        })
    }

    fn fanout_filter(
        &self,
        state: Arc<AppState>,
        tenant_id: String,
        subscriber_id: String,
        sender_id: String,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + '_>> {
        Box::pin(async move {
            match crate::store::blocks::is_blocked(
                &*state.db,
                &tenant_id,
                &subscriber_id,
                &sender_id,
            )
            .await
            {
                Ok(true) => false, // blocked — suppress delivery
                Err(e) => {
                    tracing::warn!(
                        tenant = %tenant_id,
                        blocker = %subscriber_id,
                        blocked = %sender_id,
                        "block check failed during fanout: {e}"
                    );
                    true // fail-open
                }
                _ => true,
            }
        })
    }

    fn namespaces(&self) -> Vec<&'static str> {
        vec!["herald.blocks"]
    }

    fn timers(&self) -> Vec<TimerTask> {
        vec![TimerTask {
            label: "chat:typing-ttl",
            interval: Duration::from_secs(5),
            run: Box::new(|state| {
                Box::pin(async move {
                    let expired = state.typing.expire();
                    for (tenant_id, stream_id, user_id) in expired {
                        let msg = herald_core::protocol::ServerMessage::Typing {
                            payload: herald_core::protocol::TypingPayload {
                                stream: stream_id.clone(),
                                user_id: user_id.clone(),
                                active: false,
                            },
                        };
                        crate::ws::fanout::fanout_to_stream(
                            &state,
                            &tenant_id,
                            &stream_id,
                            &msg,
                            None,
                            Some(&user_id),
                        )
                        .await;
                    }
                })
            }),
        }]
    }
}
