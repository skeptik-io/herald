//! Presence engine — manages manual presence overrides, watchlist
//! subscriptions, and presence broadcast.  Gated behind the `presence`
//! Cargo feature (enabled by default).

use std::sync::Arc;
use std::time::Duration;

use axum::routing::get;
use axum::Router;

use crate::engine::{EngineConnContext, HeraldEngine, TimerTask};
use crate::state::AppState;

pub mod http;
pub mod ws_handler;

pub struct PresenceEngine;

impl Default for PresenceEngine {
    fn default() -> Self {
        Self
    }
}

impl PresenceEngine {
    pub fn new() -> Self {
        Self
    }
}

impl HeraldEngine for PresenceEngine {
    fn name(&self) -> &'static str {
        "presence"
    }

    fn tenant_routes(&self, _state: Arc<AppState>) -> Option<Router<Arc<AppState>>> {
        let router = Router::new()
            .route("/streams/{id}/presence", get(http::stream_presence))
            .route(
                "/presence/{user_id}",
                get(http::user_presence).post(http::admin_set_presence),
            )
            .route("/presence", get(http::batch_presence));
        Some(router)
    }

    fn on_connect(
        &self,
        state: &Arc<AppState>,
        ctx: &EngineConnContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        let state = state.clone();
        let tenant_id = ctx.tenant_id.clone();
        let user_id = ctx.user_id.clone();
        Box::pin(async move {
            // Broadcast presence change to all streams the user is a member of
            crate::ws::connection::broadcast_presence_change(&state, &tenant_id, &user_id).await;

            // Notify users who have this user in their watchlist (only on first connection)
            if state
                .connections
                .user_connection_count(&tenant_id, &user_id)
                == 1
            {
                let resolved = state.presence.resolve(
                    &tenant_id,
                    &user_id,
                    &state.connections,
                    state.config.presence.manual_override_ttl_secs,
                );

                let watchers = state.connections.get_watchers(&tenant_id, &user_id);
                if !watchers.is_empty() {
                    if resolved.status == herald_core::presence::PresenceStatus::Online {
                        let msg = herald_core::protocol::ServerMessage::WatchlistOnline {
                            payload: herald_core::protocol::WatchlistPayload {
                                user_ids: vec![user_id.clone()],
                            },
                        };
                        for watcher_id in &watchers {
                            state.connections.send_to_user(&tenant_id, watcher_id, &msg);
                        }
                    } else {
                        let msg = herald_core::protocol::ServerMessage::PresenceChanged {
                            payload: herald_core::protocol::PresenceChangedPayload {
                                user_id: user_id.clone(),
                                presence: resolved.status,
                                until: resolved.until,
                            },
                        };
                        for watcher_id in &watchers {
                            state.connections.send_to_user(&tenant_id, watcher_id, &msg);
                        }
                    }
                }
            }
        })
    }

    fn on_last_disconnect(
        &self,
        state: &Arc<AppState>,
        ctx: &EngineConnContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        let state = state.clone();
        let tenant_id = ctx.tenant_id.clone();
        let user_id = ctx.user_id.clone();
        Box::pin(async move {
            ws_handler::on_last_disconnect(&state, &tenant_id, &user_id).await;
        })
    }

    fn namespaces(&self) -> Vec<&'static str> {
        vec!["herald.presence_overrides"]
    }

    fn timers(&self) -> Vec<TimerTask> {
        vec![TimerTask {
            label: "presence:override-expiry",
            interval: Duration::from_secs(30),
            run: Box::new(|state| {
                Box::pin(async move {
                    let changed = state.presence.sweep_expired();
                    for (tenant_id, user_id) in changed {
                        // Broadcast the new (connection-derived) presence
                        crate::ws::connection::broadcast_presence_change(
                            &state, &tenant_id, &user_id,
                        )
                        .await;

                        // Remove expired override from WAL
                        let _ = crate::store::presence_overrides::remove(
                            &*state.db, &tenant_id, &user_id,
                        )
                        .await;

                        // Notify watchlist watchers
                        let resolved = state.presence.resolve(
                            &tenant_id,
                            &user_id,
                            &state.connections,
                            state.config.presence.manual_override_ttl_secs,
                        );
                        let watchers = state.connections.get_watchers(&tenant_id, &user_id);
                        if !watchers.is_empty() {
                            let msg = herald_core::protocol::ServerMessage::PresenceChanged {
                                payload: herald_core::protocol::PresenceChangedPayload {
                                    user_id: user_id.clone(),
                                    presence: resolved.status,
                                    until: resolved.until,
                                },
                            };
                            for watcher_id in &watchers {
                                state.connections.send_to_user(&tenant_id, watcher_id, &msg);
                            }
                        }
                    }
                })
            }),
        }]
    }
}
