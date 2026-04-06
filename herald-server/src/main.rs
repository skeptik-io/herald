use std::sync::Arc;
use std::time::Duration;

use axum::routing::get;
use axum::Router;
use tracing::{error, info, warn};

use herald_server::config::HeraldConfig;
use herald_server::integrations;
use herald_server::state::{AppState, AppStateBuilder};
use herald_server::store;
use herald_server::ws;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    let multi_tenant = args.contains(&"--multi-tenant".to_string());
    let single_tenant = args.contains(&"--single-tenant".to_string()) || !multi_tenant;

    let config_path = args
        .iter()
        .find(|a| !a.starts_with("--") && *a != &args[0])
        .cloned()
        .unwrap_or_else(|| "herald.toml".to_string());

    let mut config = HeraldConfig::load_or_env(&config_path)?;

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&config.server.log_level));
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(filter)
        .with_target(true)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .init();

    // Pull secrets from Keep if HERALD_KEEP_ADDR is set
    config.load_secrets_from_keep().await?;

    // Validate configuration before proceeding
    config.validate(!single_tenant)?;

    // Open ShroudB storage — embedded (local WAL) or remote (shared server)
    let db: Arc<herald_server::store_backend::StoreBackend> = match config.store.mode.as_str() {
        "remote" => {
            let addr = config.store.addr.as_deref().unwrap(); // validated above
            info!(addr = %addr, "connecting to remote ShroudB");
            let remote = shroudb_client::RemoteStore::connect(addr)
                .await
                .map_err(|e| anyhow::anyhow!("remote store connect failed: {e}"))?;
            info!(addr = %addr, "remote store ready");
            Arc::new(herald_server::store_backend::StoreBackend::Remote(remote))
        }
        _ => {
            let data_dir = &config.store.path;
            if let Some(parent) = data_dir.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let storage_config = shroudb_storage::StorageEngineConfig {
                data_dir: data_dir.to_path_buf(),
                ..Default::default()
            };
            let master_key = shroudb_storage::ChainedMasterKeySource::default_chain();
            let storage = Arc::new(
                shroudb_storage::StorageEngine::open(storage_config, &master_key)
                    .await
                    .map_err(|e| anyhow::anyhow!("storage engine failed: {e}"))?,
            );
            let embedded = shroudb_storage::EmbeddedStore::new(storage.clone(), "herald");
            info!(data_dir = %data_dir.display(), "embedded store ready");
            Arc::new(herald_server::store_backend::StoreBackend::Embedded(
                embedded,
            ))
        }
    };

    // Initialize namespaces
    store::init_namespaces(&*db)
        .await
        .map_err(|e| anyhow::anyhow!("namespace init: {e}"))?;

    let ws_bind = config.server.ws_bind.clone();
    let http_bind = config.server.http_bind.clone();
    let shutdown_timeout = config.server.shutdown_timeout_secs;
    let tls_config = config.tls.clone();

    // Initialize ShroudB integrations — Sentry for authorization.
    // Embedded Sentry is only available in embedded store mode (needs local WAL).
    let mut sentry: Option<Arc<dyn integrations::SentryOps>> =
        if let herald_server::store_backend::StoreBackend::Embedded(ref embedded) = *db {
            let engine = embedded.engine().clone();
            let sentry_store = Arc::new(shroudb_storage::EmbeddedStore::new(engine, "sentry"));
            match integrations::embedded_sentry::EmbeddedSentryOps::new(sentry_store).await {
                Ok(s) => {
                    info!("Sentry engine initialized (embedded)");
                    Some(Arc::new(s) as Arc<dyn integrations::SentryOps>)
                }
                Err(e) => {
                    warn!("embedded Sentry init failed: {e}");
                    None
                }
            }
        } else {
            info!("remote store mode — embedded Sentry unavailable, use [shroudb].sentry_addr");
            None
        };
    let mut courier: Option<Arc<dyn integrations::CourierOps>> = None;
    let mut chronicle: Option<Arc<dyn integrations::ChronicleOps>> = None;

    // Remote overrides — if [shroudb] config has addresses, use remote instead of embedded.
    if let Some(ref shroudb) = config.shroudb {
        use integrations::resilient::*;

        if let Some(ref addr) = shroudb.sentry_addr {
            match integrations::sentry::RemoteSentryOps::connect(
                addr,
                shroudb.sentry_token.as_deref(),
            )
            .await
            {
                Ok(s) => {
                    info!(addr = %addr, "connected to Sentry (fail-open circuit breaker)");
                    sentry = Some(Arc::new(ResilientSentry::new(Arc::new(s))));
                }
                Err(e) => warn!(addr = %addr, error = %e, "failed to connect to Sentry"),
            }
        }
        if let Some(ref addr) = shroudb.courier_addr {
            match integrations::courier::RemoteCourierOps::connect(
                addr,
                shroudb.courier_token.as_deref(),
                "default",
            )
            .await
            {
                Ok(c) => {
                    info!(addr = %addr, "connected to Courier (with circuit breaker)");
                    courier = Some(Arc::new(ResilientCourier::new(Arc::new(c))));
                }
                Err(e) => warn!(addr = %addr, error = %e, "failed to connect to Courier"),
            }
        }
        if let Some(ref addr) = shroudb.chronicle_addr {
            match integrations::chronicle::RemoteChronicleOps::connect(
                addr,
                shroudb.chronicle_token.as_deref(),
            )
            .await
            {
                Ok(c) => {
                    info!(addr = %addr, "connected to Chronicle (with circuit breaker)");
                    chronicle = Some(Arc::new(ResilientChronicle::new(Arc::new(c))));
                }
                Err(e) => warn!(addr = %addr, error = %e, "failed to connect to Chronicle"),
            }
        }
    }

    // Initialize Meterd metering client if enabled.
    let metering: Option<Arc<herald_server::metering::MeteringClient>> =
        config.metering.as_ref().map(|mc| {
            info!(
                base_url = %mc.base_url,
                flush_interval = mc.flush_interval_secs,
                "Meterd metering enabled"
            );
            Arc::new(herald_server::metering::MeteringClient::new(mc.clone()))
        });

    let metering_flush_interval = config
        .metering
        .as_ref()
        .map(|mc| mc.flush_interval_secs)
        .unwrap_or(10);

    let state = AppState::build(AppStateBuilder {
        config,
        db,
        sentry,
        courier,
        chronicle,
        metering: metering.clone(),
    });

    // Always hydrate all tenants from Store
    state.hydrate_tenant_cache().await?;

    // In single-tenant mode, ensure a default tenant exists
    if single_tenant {
        let tid = state.bootstrap_single_tenant().await?;
        info!(
            tenant = %tid,
            tenants = state.tenant_cache.len(),
            "single-tenant mode — default tenant bootstrapped"
        );
    } else {
        if state.config.auth.super_admin_token.is_none() {
            anyhow::bail!("auth.super_admin_token is required in multi-tenant mode");
        }
        info!(tenants = state.tenant_cache.len(), "multi-tenant mode");
    }

    // Hydrate streams + members into memory
    let all_streams = store::streams::list_all(&*state.db).await?;
    for (tid, stream) in &all_streams {
        let latest_seq = store::events::latest_seq(&*state.db, tid, stream.id.as_str())
            .await
            .unwrap_or(0);
        state
            .streams
            .create_stream(tid, stream.id.as_str(), latest_seq, stream.public);
        if stream.archived {
            state.streams.set_archived(tid, stream.id.as_str(), true);
        }

        if let Ok(members) =
            store::members::list_by_stream(&*state.db, tid, stream.id.as_str()).await
        {
            for member in members {
                state
                    .streams
                    .add_member(tid, stream.id.as_str(), &member.user_id);
            }
        }
    }

    info!(streams = all_streams.len(), "hydrated stream state");

    // Stats snapshot collector — every 60 seconds
    let stats_state = state.clone();
    let stats_handle = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            let connections = stats_state.connections.total_connections() as u64;
            let events = stats_state
                .metrics
                .events_published
                .load(std::sync::atomic::Ordering::Relaxed);
            let streams = stats_state.streams.stream_count() as u64;
            let auth_failures = stats_state
                .metrics
                .ws_auth_failures
                .load(std::sync::atomic::Ordering::Relaxed);
            stats_state
                .event_bus
                .record_snapshot(connections, events, streams, auth_failures);

            // Per-tenant snapshots
            for entry in stats_state.tenant_cache.iter() {
                let tid = entry.key();
                let t_connections = stats_state.connections.tenant_connection_count(tid) as u64;
                let t_events = stats_state.tenant_events_published(tid);
                let t_webhooks = stats_state.tenant_webhooks_sent(tid);
                let t_streams = stats_state.streams.tenant_stream_count(tid) as u64;
                stats_state.event_bus.record_tenant_snapshot(
                    tid,
                    t_connections,
                    t_events,
                    t_webhooks,
                    t_streams,
                );

                // Report peak concurrent connections to Meterd
                if t_connections > 0 {
                    if let Some(ref metering) = stats_state.metering {
                        metering.track("peak_connections", tid, t_connections as f64, None);
                    }
                }
            }
        }
    });

    // Typing TTL expiry — every 5 seconds (chat feature only)
    #[cfg(feature = "chat")]
    let typing_handle = {
        let typing_state = state.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                let expired = typing_state.typing.expire();
                for (tenant_id, stream_id, user_id) in expired {
                    let msg = herald_core::protocol::ServerMessage::Typing {
                        payload: herald_core::protocol::TypingPayload {
                            stream: stream_id.clone(),
                            user_id: user_id.clone(),
                            active: false,
                        },
                    };
                    herald_server::ws::fanout::fanout_to_stream(
                        &typing_state,
                        &tenant_id,
                        &stream_id,
                        &msg,
                        None,
                        Some(&user_id),
                    )
                    .await;
                }
            }
        })
    };

    // TTL cleanup
    let cleanup_db = state.db.clone();
    let cleanup_handle = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            let now = herald_server::ws::connection::now_millis();
            match store::events::delete_expired(&*cleanup_db, now).await {
                Ok(n) if n > 0 => info!(deleted = n, "TTL cleanup: pruned expired events"),
                Err(e) => warn!("TTL cleanup error: {e}"),
                _ => {}
            }
        }
    });

    // Metering flush task
    let metering_handle = metering.map(|mc| {
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(metering_flush_interval)).await;
                if let Err(e) = mc.flush().await {
                    warn!("metering flush error: {e}");
                }
            }
        })
    });

    // Shutdown signal
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    let ws_state = state.clone();
    let ws_app = Router::new()
        .route("/", get(ws::upgrade::ws_handler))
        .with_state(ws_state);
    let http_app = herald_server::http::router(state.clone());

    let use_tls = tls_config.is_some();
    if use_tls {
        info!(ws = %ws_bind, http = %http_bind, "starting Herald (TLS, multi-tenant)");
    } else {
        info!(ws = %ws_bind, http = %http_bind, "starting Herald (multi-tenant)");
    }

    let make_shutdown = |rx: tokio::sync::watch::Receiver<bool>| async move {
        let mut rx = rx;
        while !*rx.borrow() {
            if rx.changed().await.is_err() {
                break;
            }
        }
    };

    if let Some(ref tls) = tls_config {
        let rustls_config =
            axum_server::tls_rustls::RustlsConfig::from_pem_file(&tls.cert_path, &tls.key_path)
                .await?;

        let ws_tls = rustls_config.clone();
        let ws_addr: std::net::SocketAddr = ws_bind.parse()?;
        let ws_handle = tokio::spawn(async move {
            if let Err(e) = axum_server::bind_rustls(ws_addr, ws_tls)
                .serve(ws_app.into_make_service())
                .await
            {
                error!("WS server error: {e}");
            }
        });

        let http_tls = rustls_config;
        let http_addr: std::net::SocketAddr = http_bind.parse()?;
        let http_handle = tokio::spawn(async move {
            if let Err(e) = axum_server::bind_rustls(http_addr, http_tls)
                .serve(http_app.into_make_service())
                .await
            {
                error!("HTTP server error: {e}");
            }
        });

        tokio::signal::ctrl_c().await?;
        info!("shutdown signal received, draining...");
        let _ = shutdown_tx.send(true);

        // Notify all WebSocket clients of impending shutdown
        state
            .connections
            .broadcast_all(&herald_core::protocol::ServerMessage::error(
                None,
                herald_core::error::ErrorCode::Internal,
                "server shutting down",
            ));

        // Grace period for background tasks to finish their current iteration
        tokio::time::sleep(Duration::from_millis(500)).await;
        cleanup_handle.abort();
        stats_handle.abort();
        if let Some(ref h) = metering_handle {
            h.abort();
        }
        #[cfg(feature = "chat")]
        typing_handle.abort();

        let _ = tokio::time::timeout(Duration::from_secs(shutdown_timeout), async {
            let _ = ws_handle.await;
            let _ = http_handle.await;
        })
        .await;
    } else {
        let ws_listener = tokio::net::TcpListener::bind(&ws_bind).await?;
        let http_listener = tokio::net::TcpListener::bind(&http_bind).await?;

        let ws_shutdown = shutdown_rx.clone();
        let ws_handle = tokio::spawn(async move {
            if let Err(e) = axum::serve(ws_listener, ws_app)
                .with_graceful_shutdown(make_shutdown(ws_shutdown))
                .await
            {
                error!("WS server error: {e}");
            }
        });

        let http_shutdown = shutdown_rx.clone();
        let http_handle = tokio::spawn(async move {
            if let Err(e) = axum::serve(http_listener, http_app)
                .with_graceful_shutdown(make_shutdown(http_shutdown))
                .await
            {
                error!("HTTP server error: {e}");
            }
        });

        tokio::signal::ctrl_c().await?;
        info!("shutdown signal received, draining...");
        let _ = shutdown_tx.send(true);

        // Notify all WebSocket clients of impending shutdown
        state
            .connections
            .broadcast_all(&herald_core::protocol::ServerMessage::error(
                None,
                herald_core::error::ErrorCode::Internal,
                "server shutting down",
            ));

        // Grace period for background tasks to finish their current iteration
        tokio::time::sleep(Duration::from_millis(500)).await;
        cleanup_handle.abort();
        stats_handle.abort();
        if let Some(ref h) = metering_handle {
            h.abort();
        }
        #[cfg(feature = "chat")]
        typing_handle.abort();

        let _ = tokio::time::timeout(Duration::from_secs(shutdown_timeout), async {
            let _ = ws_handle.await;
            let _ = http_handle.await;
        })
        .await;
    }

    info!("shutdown complete");
    Ok(())
}
