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
    tracing_subscriber::fmt().with_env_filter(filter).init();

    // Pull secrets from Keep if HERALD_KEEP_ADDR is set
    config.load_secrets_from_keep().await?;

    if multi_tenant && config.auth.super_admin_token.is_none() {
        anyhow::bail!("auth.super_admin_token is required in multi-tenant mode");
    }

    // Open ShroudB storage engine
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
    let db = Arc::new(shroudb_storage::EmbeddedStore::new(
        storage.clone(),
        "herald",
    ));
    info!(data_dir = %data_dir.display(), "storage engine ready");

    // Initialize namespaces
    store::init_namespaces(&*db)
        .await
        .map_err(|e| anyhow::anyhow!("namespace init: {e}"))?;

    let ws_bind = config.server.ws_bind.clone();
    let http_bind = config.server.http_bind.clone();
    let shutdown_timeout = config.server.shutdown_timeout_secs;
    let tls_config = config.tls.clone();

    // Initialize ShroudB integrations
    let mut cipher: Option<Arc<dyn integrations::CipherOps>> = None;
    let mut veil: Option<Arc<dyn integrations::VeilOps>> = None;
    let mut sentry: Option<Arc<dyn integrations::SentryOps>> = None;
    let mut courier: Option<Arc<dyn integrations::CourierOps>> = None;
    let mut chronicle: Option<Arc<dyn integrations::ChronicleOps>> = None;

    if let Some(ref shroudb) = config.shroudb {
        use integrations::resilient::*;

        if let Some(ref addr) = shroudb.cipher_addr {
            match integrations::cipher::RemoteCipherOps::connect(
                addr,
                shroudb.cipher_token.as_deref(),
            )
            .await
            {
                Ok(c) => {
                    info!(addr = %addr, "connected to Cipher (with circuit breaker)");
                    cipher = Some(Arc::new(ResilientCipher::new(Arc::new(c))));
                }
                Err(e) => warn!(addr = %addr, error = %e, "failed to connect to Cipher"),
            }
        }
        if let Some(ref addr) = shroudb.veil_addr {
            match integrations::veil::RemoteVeilOps::connect(addr, shroudb.veil_token.as_deref())
                .await
            {
                Ok(v) => {
                    info!(addr = %addr, "connected to Veil (with circuit breaker)");
                    veil = Some(Arc::new(ResilientVeil::new(Arc::new(v))));
                }
                Err(e) => warn!(addr = %addr, error = %e, "failed to connect to Veil"),
            }
        }
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

    let state = AppState::build(AppStateBuilder {
        config,
        db,
        cipher,
        veil,
        sentry,
        courier,
        chronicle,
    });

    // Tenant setup
    if single_tenant {
        let tid = state.bootstrap_single_tenant().await?;
        info!(tenant = %tid, "single-tenant mode — default tenant bootstrapped");
    } else {
        state.hydrate_tenant_cache().await?;
        info!(
            tenants = state.tenant_cache.len(),
            "multi-tenant mode — tenant cache hydrated"
        );
    }

    // Hydrate rooms + members into memory
    let all_rooms = store::rooms::list_all(&*state.db).await?;
    for (tid, room) in &all_rooms {
        let latest_seq = store::messages::latest_seq(&*state.db, tid, room.id.as_str())
            .await
            .unwrap_or(0);
        state
            .rooms
            .create_room(tid, room.id.as_str(), latest_seq, room.encryption_mode);

        if let Ok(members) = store::members::list_by_room(&*state.db, tid, room.id.as_str()).await {
            for member in members {
                state
                    .rooms
                    .add_member(tid, room.id.as_str(), &member.user_id);
            }
        }
    }

    info!(rooms = all_rooms.len(), "hydrated room state");

    // TTL cleanup
    let cleanup_db = state.db.clone();
    let cleanup_handle = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            let now = herald_server::ws::connection::now_millis();
            match store::messages::delete_expired(&*cleanup_db, now).await {
                Ok(n) if n > 0 => info!(deleted = n, "TTL cleanup: pruned expired messages"),
                Err(e) => warn!("TTL cleanup error: {e}"),
                _ => {}
            }
        }
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
        cleanup_handle.abort();

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
        cleanup_handle.abort();

        let _ = tokio::time::timeout(Duration::from_secs(shutdown_timeout), async {
            let _ = ws_handle.await;
            let _ = http_handle.await;
        })
        .await;
    }

    info!("shutdown complete");
    Ok(())
}
