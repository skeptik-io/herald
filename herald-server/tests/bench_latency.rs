//! Latency benchmark: measures message pipeline throughput.
//!
//! Uses ShroudB storage engine (no Postgres needed).
//!
//! Run: cargo test --test bench_latency -- --nocapture --test-threads=1

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::Sha256;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

use herald_server::config::*;
use herald_server::state::{AppState, AppStateBuilder};
use herald_server::store;

const ADMIN_PASSWORD: &str = "bench-admin-password-long-enough";

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
    port: u16,
    key: String,
    secret: String,
    api_token: String,
    _handle: tokio::task::JoinHandle<()>,
}

impl BenchServer {
    async fn start() -> Self {
        let db = create_test_store().await;

        let config = HeraldConfig {
            server: ServerConfig {
                bind: "127.0.0.1:0".to_string(),
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
                password: Some(ADMIN_PASSWORD.to_string()),
                token_window_secs: 300,
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
            instance_id: uuid::Uuid::new_v4().to_string(),
        });

        state.hydrate_tenant_cache().await.unwrap();
        state.bootstrap_default_tenant().await.unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let app = herald_server::http::router(state.clone());

        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        // Create a tenant for the bench tests
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("http://127.0.0.1:{port}/admin/tenants"))
            .bearer_auth(ADMIN_PASSWORD)
            .json(&json!({"id": "bench", "name": "bench", "plan": "pro"}))
            .send()
            .await
            .unwrap();
        let body: serde_json::Value = resp.json().await.unwrap();
        let key = body["key"].as_str().unwrap().to_string();
        let secret = body["secret"].as_str().unwrap().to_string();

        let resp = client
            .post(format!(
                "http://127.0.0.1:{port}/admin/tenants/bench/tokens"
            ))
            .bearer_auth(ADMIN_PASSWORD)
            .send()
            .await
            .unwrap();
        let body: serde_json::Value = resp.json().await.unwrap();
        let api_token = body["token"].as_str().unwrap().to_string();

        BenchServer {
            state,
            port,
            key,
            secret,
            api_token,
            _handle: handle,
        }
    }

    async fn setup_stream(&self, stream_id: &str, members: &[&str]) {
        let client = reqwest::Client::new();
        let base = format!("http://127.0.0.1:{}", self.port);

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

fn mint_signed_token(secret: &str, user_id: &str, streams: &[&str]) -> String {
    type HmacSha256 = Hmac<Sha256>;
    let mut sorted_streams: Vec<&str> = streams.to_vec();
    sorted_streams.sort();
    let payload = format!("{}:{}:", user_id, sorted_streams.join(","));
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(payload.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn ws_connect_auth(
    port: u16,
    key: &str,
    secret: &str,
    user_id: &str,
    streams: &[&str],
) -> WsStream {
    let token = mint_signed_token(secret, user_id, streams);
    let streams_str = streams.join(",");
    let url = format!(
        "ws://127.0.0.1:{port}/ws?key={key}&token={token}&user_id={user_id}&streams={streams_str}"
    );
    let (ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    ws
}

async fn ws_wait_auth_ok(ws: &mut WsStream) {
    for _ in 0..20 {
        let timeout = tokio::time::timeout(Duration::from_secs(5), ws.next()).await;
        match timeout {
            Ok(Some(Ok(Message::Text(text)))) => {
                let msg: Value = serde_json::from_str(&text).unwrap();
                if msg["type"] == "auth_ok" {
                    return;
                }
            }
            other => panic!("expected text frame, got: {other:?}"),
        }
    }
    panic!("did not receive 'auth_ok' within 20 frames");
}

async fn ws_subscribe(ws: &mut WsStream, streams: &[&str]) {
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

    let mut ws = ws_connect_auth(
        server.port,
        &server.key,
        &server.secret,
        "sender",
        &["bench"],
    )
    .await;
    ws_wait_auth_ok(&mut ws).await;
    ws_subscribe(&mut ws, &["bench"]).await;

    let elapsed = bench_send(&mut ws, "bench", count).await;
    print_results("WAL STORAGE (ShroudB)", count, elapsed, &server.state);
}
