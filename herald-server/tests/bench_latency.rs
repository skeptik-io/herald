//! Latency benchmark: measures message pipeline with and without ShroudB.
//!
//! Uses ShroudB storage engine (no Postgres needed).
//! ShroudB Cipher + Veil binaries needed for encrypted benchmark (skips if not found).
//!
//! Run: cargo test --test bench_latency -- --nocapture --test-threads=1

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use jsonwebtoken::{EncodingKey, Header};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

use herald_core::auth::JwtClaims;
use herald_server::config::*;
use herald_server::integrations::cipher::RemoteCipherOps;
use herald_server::integrations::veil::RemoteVeilOps;
use herald_server::integrations::{CipherOps, VeilOps};
use herald_server::state::{AppState, AppStateBuilder};
use herald_server::store;

const JWT_SECRET: &str = "bench-secret";
const SUPER_TOKEN: &str = "bench-super";

async fn create_test_store() -> Arc<shroudb_storage::EmbeddedStore> {
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
    let store = Arc::new(shroudb_storage::EmbeddedStore::new(engine, "bench"));
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
    async fn start(cipher: Option<Arc<dyn CipherOps>>, veil: Option<Arc<dyn VeilOps>>) -> Self {
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
                path: "/tmp/bench".into(),
                message_ttl_days: 7,
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
        };

        let state = AppState::build(AppStateBuilder {
            config,
            db,
            cipher,
            veil,
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
        store::tenants::create_token(&*state.db, &api_token, "default")
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

    async fn setup_room(&self, room_id: &str, members: &[&str]) {
        let client = reqwest::Client::new();
        let base = format!("http://127.0.0.1:{}", self.http_port);

        let enc_mode = if self.state.cipher.is_some() {
            "server_encrypted"
        } else {
            "plaintext"
        };

        client
            .post(format!("{base}/rooms"))
            .bearer_auth(&self.api_token)
            .json(&json!({"id": room_id, "name": room_id, "encryption_mode": enc_mode}))
            .send()
            .await
            .unwrap();

        for m in members {
            client
                .post(format!("{base}/rooms/{room_id}/members"))
                .bearer_auth(&self.api_token)
                .json(&json!({"user_id": m}))
                .send()
                .await
                .unwrap();
        }
    }
}

fn mint_jwt(user_id: &str, rooms: &[&str]) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    jsonwebtoken::encode(
        &Header::default(),
        &JwtClaims {
            sub: user_id.to_string(),
            tenant: "default".to_string(),
            rooms: rooms.iter().map(|s| s.to_string()).collect(),
            exp: now + 3600,
            iat: now,
            iss: "test".to_string(),
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

async fn ws_auth_subscribe(ws: &mut WsStream, token: &str, rooms: &[&str]) {
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
        json!({"type": "subscribe", "payload": {"rooms": rooms}})
            .to_string()
            .into(),
    ))
    .await
    .unwrap();
    let mut got = 0;
    while got < rooms.len() {
        if let Some(Ok(Message::Text(text))) = ws.next().await {
            let msg: Value = serde_json::from_str(&text).unwrap();
            if msg["type"] == "subscribed" {
                got += 1;
            }
        }
    }
}

async fn bench_send(ws: &mut WsStream, room: &str, count: u64) -> Duration {
    let start = Instant::now();
    for i in 0..count {
        ws.send(Message::Text(
            json!({
                "type": "message.send",
                "ref": format!("b{i}"),
                "payload": {"room": room, "body": format!("bench msg {i}")}
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
            if msg["type"] == "message.ack" {
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
    let (p50, p95, p99) = state.metrics.message_total.percentiles();
    let avg = state.metrics.message_total.avg_ms();

    println!("\n=== {label} ===");
    println!(
        "  Messages: {count} in {:.2}s ({:.0} msg/s)",
        elapsed.as_secs_f64(),
        throughput
    );
    println!("  Total  — avg: {avg:.2}ms  p50: {p50:.2}ms  p95: {p95:.2}ms  p99: {p99:.2}ms");

    let encrypt = &state.metrics.message_encrypt;
    let store = &state.metrics.message_store;
    let index = &state.metrics.message_index;
    let fanout = &state.metrics.message_fanout;

    if encrypt.count() > 0 && encrypt.avg_ms() > 0.01 {
        println!(
            "  Encrypt — avg: {:.2}ms  p50: {:.2}ms  p95: {:.2}ms  p99: {:.2}ms",
            encrypt.avg_ms(),
            encrypt.percentiles().0,
            encrypt.percentiles().1,
            encrypt.percentiles().2,
        );
    }
    println!(
        "  Store   — avg: {:.2}ms  p50: {:.2}ms  p95: {:.2}ms  p99: {:.2}ms",
        store.avg_ms(),
        store.percentiles().0,
        store.percentiles().1,
        store.percentiles().2,
    );
    if index.count() > 0 && index.avg_ms() > 0.01 {
        println!(
            "  Index   — avg: {:.2}ms  p50: {:.2}ms  p95: {:.2}ms  p99: {:.2}ms",
            index.avg_ms(),
            index.percentiles().0,
            index.percentiles().1,
            index.percentiles().2,
        );
    }
    println!(
        "  Fanout  — avg: {:.2}ms  p50: {:.2}ms  p95: {:.2}ms  p99: {:.2}ms",
        fanout.avg_ms(),
        fanout.percentiles().0,
        fanout.percentiles().1,
        fanout.percentiles().2,
    );
}

// Engine harness for ShroudB binaries
struct EngineServer {
    child: Child,
    pub tcp_addr: String,
    _data_dir: tempfile::TempDir,
}

impl EngineServer {
    async fn start(name: &str) -> Option<Self> {
        let binary = find_binary(name)?;
        let tcp_port = free_port();
        let tcp_addr = format!("127.0.0.1:{tcp_port}");
        let data_dir = tempfile::tempdir().ok()?;

        let child = Command::new(&binary)
            .arg("--tcp-bind")
            .arg(&tcp_addr)
            .arg("--data-dir")
            .arg(data_dir.path())
            .arg("--log-level")
            .arg("warn")
            .env(
                "SHROUDB_MASTER_KEY",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            )
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;

        let mut server = Self {
            child,
            tcp_addr: tcp_addr.clone(),
            _data_dir: data_dir,
        };

        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            if tokio::time::Instant::now() > deadline {
                return None;
            }
            if let Some(status) = server.child.try_wait().ok().flatten() {
                eprintln!("{name} exited: {status}");
                return None;
            }
            if tokio::net::TcpStream::connect(&tcp_addr).await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Some(server)
    }
}

impl Drop for EngineServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn find_binary(name: &str) -> Option<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();
    [
        PathBuf::from(&home).join(format!(
            "dev/shroudb/shroudb-{name}/target/debug/shroudb-{name}"
        )),
        PathBuf::from(&home).join(format!(
            "dev/shroudb/shroudb-{name}/target/release/shroudb-{name}"
        )),
    ]
    .into_iter()
    .find(|p| p.exists())
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bench_plaintext_wal_latency() {
    let count = 500;
    let server = BenchServer::start(None, None).await;
    server.setup_room("bench", &["sender"]).await;

    let mut ws = ws_connect(server.ws_port).await;
    ws_auth_subscribe(&mut ws, &mint_jwt("sender", &["bench"]), &["bench"]).await;

    let elapsed = bench_send(&mut ws, "bench", count).await;
    print_results(
        "PLAINTEXT + WAL (no ShroudB, no Postgres)",
        count,
        elapsed,
        &server.state,
    );
}

#[tokio::test]
async fn bench_encrypted_wal_latency() {
    let cipher_server = match EngineServer::start("cipher").await {
        Some(s) => s,
        None => {
            eprintln!("SKIPPED: shroudb-cipher binary not found");
            return;
        }
    };
    let veil_server = match EngineServer::start("veil").await {
        Some(s) => s,
        None => {
            eprintln!("SKIPPED: shroudb-veil binary not found");
            return;
        }
    };

    let cipher = Arc::new(
        RemoteCipherOps::connect(&cipher_server.tcp_addr, None)
            .await
            .unwrap(),
    );
    let veil = Arc::new(
        RemoteVeilOps::connect(&veil_server.tcp_addr, None)
            .await
            .unwrap(),
    );

    let count = 500;
    let server = BenchServer::start(Some(cipher), Some(veil)).await;
    server.setup_room("bench", &["sender"]).await;

    let mut ws = ws_connect(server.ws_port).await;
    ws_auth_subscribe(&mut ws, &mint_jwt("sender", &["bench"]), &["bench"]).await;

    let elapsed = bench_send(&mut ws, "bench", count).await;
    print_results(
        "ENCRYPTED + WAL + REMOTE (Cipher + Veil over TCP)",
        count,
        elapsed,
        &server.state,
    );
}

#[tokio::test]
async fn bench_embedded_encrypted_latency() {
    use herald_server::integrations::embedded_cipher::EmbeddedCipherOps;
    use herald_server::integrations::embedded_veil::EmbeddedVeilOps;

    // Create a separate EmbeddedStore for the engines (shares same WAL dir pattern)
    let dir = tempfile::tempdir().unwrap();
    let engine_config = shroudb_storage::StorageEngineConfig {
        data_dir: dir.keep(),
        ..Default::default()
    };
    let key = shroudb_storage::EphemeralKey;
    let engine_storage = Arc::new(
        shroudb_storage::StorageEngine::open(engine_config, &key)
            .await
            .unwrap(),
    );
    let engine_store = Arc::new(shroudb_storage::EmbeddedStore::new(
        engine_storage,
        "embedded-engines",
    ));

    let cipher = Arc::new(EmbeddedCipherOps::new(engine_store.clone()).await.unwrap());
    let veil = Arc::new(EmbeddedVeilOps::new(engine_store).await.unwrap());

    let count = 500;
    let server = BenchServer::start(Some(cipher), Some(veil)).await;
    server.setup_room("bench", &["sender"]).await;

    let mut ws = ws_connect(server.ws_port).await;
    ws_auth_subscribe(&mut ws, &mint_jwt("sender", &["bench"]), &["bench"]).await;

    let elapsed = bench_send(&mut ws, "bench", count).await;
    print_results(
        "ENCRYPTED + WAL + EMBEDDED (Cipher + Veil in-process)",
        count,
        elapsed,
        &server.state,
    );
}
