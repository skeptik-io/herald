//! Latency benchmark: measures message pipeline throughput.
//!
//! Uses ShroudB storage engine (no Postgres needed).
//!
//! Run: cargo test --test bench_latency -- --nocapture --test-threads=1

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use jsonwebtoken::{EncodingKey, Header};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

use herald_core::auth::JwtClaims;
use herald_server::config::*;
use herald_server::state::{AppState, AppStateBuilder};
use herald_server::store;

const JWT_SECRET: &str = "bench-secret";
const SUPER_TOKEN: &str = "bench-super";

async fn create_test_store() -> Arc<herald_server::store_backend::StoreBackend> {
    let dir = tempfile::tempdir().unwrap();
    let config = shroudb_storage::StorageEngineConfig {
        data_dir: dir.keep(),
        ..Default::default()
    };
    let key = shroudb_storage::EphemeralKey;
    let engine = Arc::new(
        shroudb_storage::StorageEngine::open(config, &key)
            .await
            .unwrap(),
    );
    let embedded = shroudb_storage::EmbeddedStore::new(engine, "bench");
    let store = Arc::new(herald_server::store_backend::StoreBackend::Embedded(
        embedded,
    ));
    store::init_namespaces(&*store).await.unwrap();
    store
}

struct BenchServer {
    state: Arc<AppState>,
    ws_port: u16,
    http_port: u16,
    api_token: String,
    _ws_handle: tokio::task::JoinHandle<()>,
    _http_handle: tokio::task::JoinHandle<()>,
}

impl BenchServer {
    async fn start() -> Self {
        let db = create_test_store().await;

        let config = HeraldConfig {
            server: ServerConfig {
                ws_bind: "127.0.0.1:0".to_string(),
                http_bind: "127.0.0.1:0".to_string(),
                log_level: "warn".to_string(),
                max_messages_per_sec: 100000,
                ..Default::default()
            },
            store: StoreConfig {
                mode: "embedded".to_string(),
                path: "/tmp/bench".into(),
                addr: None,
                event_ttl_days: 7,
            },
            auth: AuthConfig {
                jwt_secret: Some(JWT_SECRET.to_string()),
                jwt_issuer: None,
                super_admin_token: Some(SUPER_TOKEN.to_string()),
                api: ApiAuthConfig { tokens: vec![] },
            },
            presence: PresenceConfig {
                linger_secs: 0,
                manual_override_ttl_secs: 14400,
            },
            webhook: None,
            shroudb: None,
            tls: None,
            tenant_limits: Default::default(),
            cors: None,
            cluster: ClusterConfig::default(),
        };

        let state = AppState::build(AppStateBuilder {
            config,
            db,
            sentry: None,
            courier: None,
            chronicle: None,
        });

        state.bootstrap_single_tenant().await.unwrap();

        let ws_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let http_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let ws_port = ws_listener.local_addr().unwrap().port();
        let http_port = http_listener.local_addr().unwrap().port();

        let ws_state = state.clone();
        let ws_app = axum::Router::new()
            .route(
                "/",
                axum::routing::get(herald_server::ws::upgrade::ws_handler),
            )
            .with_state(ws_state);
        let http_app = herald_server::http::router(state.clone());

        let ws_handle = tokio::spawn(async move {
            axum::serve(ws_listener, ws_app).await.unwrap();
        });
        let http_handle = tokio::spawn(async move {
            axum::serve(http_listener, http_app).await.unwrap();
        });

        let api_token = uuid::Uuid::new_v4().to_string();
        store::tenants::create_token(&*state.db, &api_token, "default", None)
            .await
            .unwrap();

        tokio::time::sleep(Duration::from_millis(50)).await;

        BenchServer {
            state,
            ws_port,
            http_port,
            api_token,
            _ws_handle: ws_handle,
            _http_handle: http_handle,
        }
    }

    async fn setup_stream(&self, stream_id: &str, members: &[&str]) {
        let client = reqwest::Client::new();
        let base = format!("http://127.0.0.1:{}", self.http_port);

        client
            .post(format!("{base}/streams"))
            .bearer_auth(&self.api_token)
            .json(&json!({"id": stream_id, "name": stream_id}))
            .send()
            .await
            .unwrap();

        for m in members {
            client
                .post(format!("{base}/streams/{stream_id}/members"))
                .bearer_auth(&self.api_token)
                .json(&json!({"user_id": m}))
                .send()
                .await
                .unwrap();
        }
    }
}

fn mint_jwt(user_id: &str, streams: &[&str]) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    jsonwebtoken::encode(
        &Header::default(),
        &JwtClaims {
            sub: user_id.to_string(),
            tenant: "default".to_string(),
            streams: streams.iter().map(|s| s.to_string()).collect(),
            exp: now + 3600,
            iat: now,
            iss: "test".to_string(),
            watchlist: vec![],
        },
        &EncodingKey::from_secret(JWT_SECRET.as_bytes()),
    )
    .unwrap()
}

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn ws_connect(port: u16) -> WsStream {
    let (ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/"))
        .await
        .unwrap();
    ws
}

async fn ws_auth_subscribe(ws: &mut WsStream, token: &str, streams: &[&str]) {
    ws.send(Message::Text(
        json!({"type": "auth", "payload": {"token": token}})
            .to_string()
            .into(),
    ))
    .await
    .unwrap();
    loop {
        if let Some(Ok(Message::Text(text))) = ws.next().await {
            let msg: Value = serde_json::from_str(&text).unwrap();
            if msg["type"] == "auth_ok" {
                break;
            }
        }
    }
    ws.send(Message::Text(
        json!({"type": "subscribe", "payload": {"streams": streams}})
            .to_string()
            .into(),
    ))
    .await
    .unwrap();
    let mut got = 0;
    while got < streams.len() {
        if let Some(Ok(Message::Text(text))) = ws.next().await {
            let msg: Value = serde_json::from_str(&text).unwrap();
            if msg["type"] == "subscribed" {
                got += 1;
            }
        }
    }
}

async fn bench_send(ws: &mut WsStream, stream: &str, count: u64) -> Duration {
    let start = Instant::now();
    for i in 0..count {
        ws.send(Message::Text(
            json!({
                "type": "event.publish",
                "ref": format!("b{i}"),
                "payload": {"stream": stream, "body": format!("bench msg {i}")}
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    }

    let mut acks = 0u64;
    let deadline = Instant::now() + Duration::from_secs(30);
    while acks < count && Instant::now() < deadline {
        if let Ok(Some(Ok(Message::Text(text)))) =
            tokio::time::timeout(Duration::from_secs(5), ws.next()).await
        {
            let msg: Value = serde_json::from_str(&text).unwrap();
            if msg["type"] == "event.ack" {
                acks += 1;
            }
        } else {
            break;
        }
    }

    assert_eq!(acks, count, "expected {count} acks, got {acks}");
    start.elapsed()
}

fn print_results(label: &str, count: u64, elapsed: Duration, state: &AppState) {
    let throughput = count as f64 / elapsed.as_secs_f64();
    let (p50, p95, p99) = state.metrics.event_total.percentiles();
    let avg = state.metrics.event_total.avg_ms();

    println!("\n=== {label} ===");
    println!(
        "  Events: {count} in {:.2}s ({:.0} evt/s)",
        elapsed.as_secs_f64(),
        throughput
    );
    println!("  Total  — avg: {avg:.2}ms  p50: {p50:.2}ms  p95: {p95:.2}ms  p99: {p99:.2}ms");

    let store = &state.metrics.event_store;
    let fanout = &state.metrics.event_fanout;

    println!(
        "  Store   — avg: {:.2}ms  p50: {:.2}ms  p95: {:.2}ms  p99: {:.2}ms",
        store.avg_ms(),
        store.percentiles().0,
        store.percentiles().1,
        store.percentiles().2,
    );
    println!(
        "  Fanout  — avg: {:.2}ms  p50: {:.2}ms  p95: {:.2}ms  p99: {:.2}ms",
        fanout.avg_ms(),
        fanout.percentiles().0,
        fanout.percentiles().1,
        fanout.percentiles().2,
    );
}

// ---------------------------------------------------------------------------
// Benchmark
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bench_wal_latency() {
    let count = 500;
    let server = BenchServer::start().await;
    server.setup_stream("bench", &["sender"]).await;

    let mut ws = ws_connect(server.ws_port).await;
    ws_auth_subscribe(&mut ws, &mint_jwt("sender", &["bench"]), &["bench"]).await;

    let elapsed = bench_send(&mut ws, "bench", count).await;
    print_results("WAL STORAGE (ShroudB)", count, elapsed, &server.state);
}
