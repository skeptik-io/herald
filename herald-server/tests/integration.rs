//! Multi-tenant integration tests.
//!
//! Uses ShroudB storage engine (in-memory via EphemeralKey) — no Postgres needed.

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use jsonwebtoken::{EncodingKey, Header};
use reqwest::StatusCode;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

use herald_core::auth::JwtClaims;
use herald_server::config::{
    ApiAuthConfig, AuthConfig, HeraldConfig, PresenceConfig, ServerConfig, StoreConfig,
};
use herald_server::state::{AppState, AppStateBuilder};
use herald_server::store;

const SUPER_ADMIN_TOKEN: &str = "test-super-admin";
const TENANT_A_SECRET: &str = "tenant-a-secret";
const TENANT_B_SECRET: &str = "tenant-b-secret";

// ---------------------------------------------------------------------------
// Test harness
// ---------------------------------------------------------------------------

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
    let store = Arc::new(shroudb_storage::EmbeddedStore::new(engine, "test"));
    store::init_namespaces(&*store).await.unwrap();
    store
}

struct TestServer {
    #[allow(dead_code)]
    state: Arc<AppState>,
    ws_port: u16,
    http_port: u16,
    _ws_handle: tokio::task::JoinHandle<()>,
    _http_handle: tokio::task::JoinHandle<()>,
}

impl TestServer {
    async fn start() -> Self {
        let db = create_test_store().await;

        let config = HeraldConfig {
            server: ServerConfig {
                ws_bind: "127.0.0.1:0".to_string(),
                http_bind: "127.0.0.1:0".to_string(),
                log_level: "warn".to_string(),
                ..Default::default()
            },
            store: StoreConfig {
                path: "/tmp/herald-test".into(),
                message_ttl_days: 7,
            },
            auth: AuthConfig {
                jwt_secret: Some(TENANT_A_SECRET.to_string()),
                jwt_issuer: None,
                super_admin_token: Some(SUPER_ADMIN_TOKEN.to_string()),
                api: ApiAuthConfig { tokens: vec![] },
            },
            presence: PresenceConfig {
                linger_secs: 0,
                manual_override_ttl_secs: 14400,
            },
            webhook: None,
            shroudb: None,
            tls: None,
        };

        let state = AppState::build(AppStateBuilder {
            config,
            db,
            cipher: None,
            veil: None,
            sentry: None,
            courier: None,
            chronicle: None,
        });

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

        tokio::time::sleep(Duration::from_millis(50)).await;

        TestServer {
            state,
            ws_port,
            http_port,
            _ws_handle: ws_handle,
            _http_handle: http_handle,
        }
    }

    fn http_url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{}", self.http_port, path)
    }

    fn http_client(&self) -> reqwest::Client {
        reqwest::Client::new()
    }

    async fn ws_connect(
        &self,
    ) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>
    {
        let url = format!("ws://127.0.0.1:{}/", self.ws_port);
        let (ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        ws
    }

    async fn create_tenant(&self, id: &str, jwt_secret: &str) -> String {
        let resp = self
            .http_client()
            .post(self.http_url("/admin/tenants"))
            .bearer_auth(SUPER_ADMIN_TOKEN)
            .json(&json!({"id": id, "name": id, "jwt_secret": jwt_secret}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED, "create tenant {id}");

        let resp = self
            .http_client()
            .post(self.http_url(&format!("/admin/tenants/{id}/tokens")))
            .bearer_auth(SUPER_ADMIN_TOKEN)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body: Value = resp.json().await.unwrap();
        body["token"].as_str().unwrap().to_string()
    }

    async fn create_room(&self, api_token: &str, room_id: &str) {
        let resp = self
            .http_client()
            .post(self.http_url("/rooms"))
            .bearer_auth(api_token)
            .json(&json!({"id": room_id, "name": room_id}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED, "create room {room_id}");
    }

    async fn add_member(&self, api_token: &str, room_id: &str, user_id: &str) {
        let resp = self
            .http_client()
            .post(self.http_url(&format!("/rooms/{room_id}/members")))
            .bearer_auth(api_token)
            .json(&json!({"user_id": user_id}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED, "add member {user_id}");
    }
}

fn mint_jwt(user_id: &str, tenant: &str, rooms: &[&str], secret: &str) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    jsonwebtoken::encode(
        &Header::default(),
        &JwtClaims {
            sub: user_id.to_string(),
            tenant: tenant.to_string(),
            rooms: rooms.iter().map(|s| s.to_string()).collect(),
            exp: now + 3600,
            iat: now,
            iss: "test".to_string(),
        },
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .unwrap()
}

async fn ws_send(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    msg: Value,
) {
    ws.send(Message::Text(msg.to_string().into()))
        .await
        .unwrap();
}

async fn ws_recv_type(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    expected_type: &str,
) -> Value {
    for _ in 0..20 {
        let timeout = tokio::time::timeout(Duration::from_secs(5), ws.next()).await;
        match timeout {
            Ok(Some(Ok(Message::Text(text)))) => {
                let msg: Value = serde_json::from_str(&text).unwrap();
                if msg["type"] == expected_type {
                    return msg;
                }
            }
            other => panic!("expected text frame, got: {other:?}"),
        }
    }
    panic!("did not receive '{expected_type}' within 20 frames");
}

async fn ws_auth(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    token: &str,
) -> Value {
    ws_send(ws, json!({"type": "auth", "payload": {"token": token}})).await;
    ws_recv_type(ws, "auth_ok").await
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_tenant_creation_and_room_isolation() {
    let server = TestServer::start().await;

    let token_a = server.create_tenant("acme", TENANT_A_SECRET).await;
    let token_b = server.create_tenant("beta", TENANT_B_SECRET).await;

    server.create_room(&token_a, "chat").await;

    // Tenant B can't see tenant A's room
    let resp = server
        .http_client()
        .get(server.http_url("/rooms/chat"))
        .bearer_auth(&token_b)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // Tenant B can create same-named room — independent namespace
    server.create_room(&token_b, "chat").await;
}

#[tokio::test]
async fn test_ws_tenant_isolation() {
    let server = TestServer::start().await;

    let token_a = server.create_tenant("ws-a", TENANT_A_SECRET).await;
    let token_b = server.create_tenant("ws-b", TENANT_B_SECRET).await;

    server.create_room(&token_a, "room").await;
    server.add_member(&token_a, "room", "alice").await;
    server.create_room(&token_b, "room").await;
    server.add_member(&token_b, "room", "bob").await;

    let mut ws_a = server.ws_connect().await;
    ws_auth(
        &mut ws_a,
        &mint_jwt("alice", "ws-a", &["room"], TENANT_A_SECRET),
    )
    .await;
    ws_send(
        &mut ws_a,
        json!({"type": "subscribe", "payload": {"rooms": ["room"]}}),
    )
    .await;
    ws_recv_type(&mut ws_a, "subscribed").await;

    let mut ws_b = server.ws_connect().await;
    ws_auth(
        &mut ws_b,
        &mint_jwt("bob", "ws-b", &["room"], TENANT_B_SECRET),
    )
    .await;
    ws_send(
        &mut ws_b,
        json!({"type": "subscribe", "payload": {"rooms": ["room"]}}),
    )
    .await;
    ws_recv_type(&mut ws_b, "subscribed").await;

    // Alice sends — seq=1 in tenant A
    ws_send(
        &mut ws_a,
        json!({"type": "message.send", "ref": "m1", "payload": {"room": "room", "body": "from A"}}),
    )
    .await;
    let ack_a = ws_recv_type(&mut ws_a, "message.ack").await;
    assert_eq!(ack_a["payload"]["seq"], 1);

    // Bob sends — seq=1 in tenant B (independent)
    ws_send(
        &mut ws_b,
        json!({"type": "message.send", "ref": "m2", "payload": {"room": "room", "body": "from B"}}),
    )
    .await;
    let ack_b = ws_recv_type(&mut ws_b, "message.ack").await;
    assert_eq!(ack_b["payload"]["seq"], 1);
}

#[tokio::test]
async fn test_invalid_tenant_rejected() {
    let server = TestServer::start().await;
    server.create_tenant("real", TENANT_A_SECRET).await;

    let bad_jwt = mint_jwt("alice", "fake", &["chat"], TENANT_A_SECRET);
    let mut ws = server.ws_connect().await;
    ws_send(
        &mut ws,
        json!({"type": "auth", "payload": {"token": bad_jwt}}),
    )
    .await;

    let result = tokio::time::timeout(Duration::from_secs(2), ws.next()).await;
    match result {
        Ok(Some(Ok(Message::Text(text)))) => {
            let msg: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(msg["type"], "auth_error");
        }
        Ok(Some(Ok(Message::Close(_)))) | Ok(Some(Err(_))) | Ok(None) => {}
        Err(_) => panic!("timeout"),
        other => panic!("unexpected: {other:?}"),
    }
}

#[tokio::test]
async fn test_admin_requires_super_token() {
    let server = TestServer::start().await;

    let resp = server
        .http_client()
        .post(server.http_url("/admin/tenants"))
        .json(&json!({"id": "x", "name": "x", "jwt_secret": "x"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_subscribe_send_fanout() {
    let server = TestServer::start().await;
    let token = server.create_tenant("fanout", TENANT_A_SECRET).await;

    server.create_room(&token, "general").await;
    server.add_member(&token, "general", "alice").await;
    server.add_member(&token, "general", "bob").await;

    let mut ws_a = server.ws_connect().await;
    ws_auth(
        &mut ws_a,
        &mint_jwt("alice", "fanout", &["general"], TENANT_A_SECRET),
    )
    .await;
    ws_send(
        &mut ws_a,
        json!({"type": "subscribe", "payload": {"rooms": ["general"]}}),
    )
    .await;
    ws_recv_type(&mut ws_a, "subscribed").await;

    let mut ws_b = server.ws_connect().await;
    ws_auth(
        &mut ws_b,
        &mint_jwt("bob", "fanout", &["general"], TENANT_A_SECRET),
    )
    .await;
    ws_send(
        &mut ws_b,
        json!({"type": "subscribe", "payload": {"rooms": ["general"]}}),
    )
    .await;
    ws_recv_type(&mut ws_b, "subscribed").await;

    ws_send(
        &mut ws_a,
        json!({
            "type": "message.send",
            "ref": "m1",
            "payload": {"room": "general", "body": "hello!"}
        }),
    )
    .await;

    ws_recv_type(&mut ws_a, "message.ack").await;
    let msg_a = ws_recv_type(&mut ws_a, "message.new").await;
    assert_eq!(msg_a["payload"]["body"], "hello!");

    let msg_b = ws_recv_type(&mut ws_b, "message.new").await;
    assert_eq!(msg_b["payload"]["body"], "hello!");
}

#[tokio::test]
async fn test_health_and_metrics() {
    let server = TestServer::start().await;

    let resp = server
        .http_client()
        .get(server.http_url("/health"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = server
        .http_client()
        .get(server.http_url("/metrics"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.text().await.unwrap();
    assert!(body.contains("herald_connections_total"));
    assert!(body.contains("herald_message_total_seconds"));
}
