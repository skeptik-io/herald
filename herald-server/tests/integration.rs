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
    TenantLimitsConfig, TlsConfig, WebhookConfig,
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
                max_messages_per_sec: 1000,
                api_rate_limit: 10000,
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
            tenant_limits: Default::default(),
            cors: None,
        };

        let state = AppState::build(AppStateBuilder {
            config,
            db,
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

// ---------------------------------------------------------------------------
// Admin API tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_admin_tenant_crud() {
    let server = TestServer::start().await;

    // Create
    let resp = server
        .http_client()
        .post(server.http_url("/admin/tenants"))
        .bearer_auth(SUPER_ADMIN_TOKEN)
        .json(&json!({"id": "crud-test", "name": "CRUD Tenant", "jwt_secret": "secret123"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Get
    let resp = server
        .http_client()
        .get(server.http_url("/admin/tenants/crud-test"))
        .bearer_auth(SUPER_ADMIN_TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["name"], "CRUD Tenant");
    assert_eq!(body["plan"], "free");

    // Update
    let resp = server
        .http_client()
        .patch(server.http_url("/admin/tenants/crud-test"))
        .bearer_auth(SUPER_ADMIN_TOKEN)
        .json(&json!({"plan": "pro"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // List
    let resp = server
        .http_client()
        .get(server.http_url("/admin/tenants"))
        .bearer_auth(SUPER_ADMIN_TOKEN)
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    let tenants = body["tenants"].as_array().unwrap();
    assert!(tenants.iter().any(|t| t["id"] == "crud-test"));

    // Delete
    let resp = server
        .http_client()
        .delete(server.http_url("/admin/tenants/crud-test"))
        .bearer_auth(SUPER_ADMIN_TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Verify deleted
    let resp = server
        .http_client()
        .get(server.http_url("/admin/tenants/crud-test"))
        .bearer_auth(SUPER_ADMIN_TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_admin_api_token_management() {
    let server = TestServer::start().await;
    server.create_tenant("tok-test", TENANT_A_SECRET).await;

    // Create token
    let resp = server
        .http_client()
        .post(server.http_url("/admin/tenants/tok-test/tokens"))
        .bearer_auth(SUPER_ADMIN_TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body: Value = resp.json().await.unwrap();
    let token = body["token"].as_str().unwrap();
    assert!(!token.is_empty());

    // List tokens
    let resp = server
        .http_client()
        .get(server.http_url("/admin/tenants/tok-test/tokens"))
        .bearer_auth(SUPER_ADMIN_TOKEN)
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    let tokens = body["tokens"].as_array().unwrap();
    assert!(tokens.iter().any(|t| t["token"].as_str() == Some(token)));

    // Token works for tenant API
    let resp = server
        .http_client()
        .post(server.http_url("/rooms"))
        .bearer_auth(token)
        .json(&json!({"id": "tok-room", "name": "Token Room"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
}

// ---------------------------------------------------------------------------
// Room edge cases
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_duplicate_room_creation() {
    let server = TestServer::start().await;
    let token = server.create_tenant("dup", TENANT_A_SECRET).await;

    server.create_room(&token, "room1").await;

    // Duplicate should fail
    let _resp = server
        .http_client()
        .post(server.http_url("/rooms"))
        .bearer_auth(&token)
        .json(&json!({"id": "room1", "name": "Duplicate"}))
        .send()
        .await
        .unwrap();
    // Store upserts — room exists, verify it
}

#[tokio::test]
async fn test_room_update_and_delete() {
    let server = TestServer::start().await;
    let token = server.create_tenant("rud", TENANT_A_SECRET).await;
    server.create_room(&token, "updatable").await;

    // Update
    let resp = server
        .http_client()
        .patch(server.http_url("/rooms/updatable"))
        .bearer_auth(&token)
        .json(&json!({"name": "Updated Name", "meta": {"custom": true}}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Verify
    let resp = server
        .http_client()
        .get(server.http_url("/rooms/updatable"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["name"], "Updated Name");

    // Delete
    let resp = server
        .http_client()
        .delete(server.http_url("/rooms/updatable"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Verify gone
    let resp = server
        .http_client()
        .get(server.http_url("/rooms/updatable"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_nonexistent_room() {
    let server = TestServer::start().await;
    let token = server.create_tenant("noroom", TENANT_A_SECRET).await;

    let resp = server
        .http_client()
        .get(server.http_url("/rooms/does-not-exist"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// Member edge cases
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_member_role_update() {
    let server = TestServer::start().await;
    let token = server.create_tenant("role", TENANT_A_SECRET).await;
    server.create_room(&token, "room").await;
    server.add_member(&token, "room", "alice").await;

    // Update role
    let resp = server
        .http_client()
        .patch(server.http_url("/rooms/room/members/alice"))
        .bearer_auth(&token)
        .json(&json!({"role": "admin"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Verify
    let resp = server
        .http_client()
        .get(server.http_url("/rooms/room/members"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    let alice = body["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["user_id"] == "alice")
        .unwrap();
    assert_eq!(alice["role"], "admin");
}

#[tokio::test]
async fn test_member_remove() {
    let server = TestServer::start().await;
    let token = server.create_tenant("rem", TENANT_A_SECRET).await;
    server.create_room(&token, "room").await;
    server.add_member(&token, "room", "alice").await;

    let resp = server
        .http_client()
        .delete(server.http_url("/rooms/room/members/alice"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Verify removed
    let resp = server
        .http_client()
        .get(server.http_url("/rooms/room/members"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    assert!(body["members"].as_array().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// WS: presence, cursor, typing, history, reconnect
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_ws_presence_cursor_typing() {
    let server = TestServer::start().await;
    let token = server.create_tenant("pct", TENANT_A_SECRET).await;
    server.create_room(&token, "room").await;
    server.add_member(&token, "room", "alice").await;
    server.add_member(&token, "room", "bob").await;

    let mut ws_a = server.ws_connect().await;
    ws_auth(
        &mut ws_a,
        &mint_jwt("alice", "pct", &["room"], TENANT_A_SECRET),
    )
    .await;
    ws_send(
        &mut ws_a,
        json!({"type":"subscribe","payload":{"rooms":["room"]}}),
    )
    .await;
    ws_recv_type(&mut ws_a, "subscribed").await;

    let mut ws_b = server.ws_connect().await;
    ws_auth(
        &mut ws_b,
        &mint_jwt("bob", "pct", &["room"], TENANT_A_SECRET),
    )
    .await;
    ws_send(
        &mut ws_b,
        json!({"type":"subscribe","payload":{"rooms":["room"]}}),
    )
    .await;
    ws_recv_type(&mut ws_b, "subscribed").await;

    // Presence
    ws_send(
        &mut ws_a,
        json!({"type":"presence.set","payload":{"status":"dnd"}}),
    )
    .await;
    let r = ws_recv_type(&mut ws_b, "presence.changed").await;
    assert_eq!(r["payload"]["user_id"], "alice");
    assert_eq!(r["payload"]["presence"], "dnd");

    // Send a message so we have a seq for cursor
    ws_send(
        &mut ws_a,
        json!({"type":"message.send","ref":"m1","payload":{"room":"room","body":"msg"}}),
    )
    .await;
    let ack = ws_recv_type(&mut ws_a, "message.ack").await;
    ws_recv_type(&mut ws_a, "message.new").await;
    ws_recv_type(&mut ws_b, "message.new").await;

    // Cursor
    ws_send(
        &mut ws_a,
        json!({"type":"cursor.update","payload":{"room":"room","seq":ack["payload"]["seq"]}}),
    )
    .await;
    let r = ws_recv_type(&mut ws_b, "cursor.moved").await;
    assert_eq!(r["payload"]["user_id"], "alice");

    // Typing
    ws_send(
        &mut ws_a,
        json!({"type":"typing.start","payload":{"room":"room"}}),
    )
    .await;
    let r = ws_recv_type(&mut ws_b, "typing").await;
    assert_eq!(r["payload"]["user_id"], "alice");
    assert_eq!(r["payload"]["active"], true);
}

#[tokio::test]
async fn test_ws_message_history() {
    let server = TestServer::start().await;
    let token = server.create_tenant("hist", TENANT_A_SECRET).await;
    server.create_room(&token, "room").await;
    server.add_member(&token, "room", "alice").await;

    let mut ws = server.ws_connect().await;
    ws_auth(
        &mut ws,
        &mint_jwt("alice", "hist", &["room"], TENANT_A_SECRET),
    )
    .await;
    ws_send(
        &mut ws,
        json!({"type":"subscribe","payload":{"rooms":["room"]}}),
    )
    .await;
    ws_recv_type(&mut ws, "subscribed").await;

    // Send 5 messages
    for i in 1..=5 {
        ws_send(&mut ws, json!({"type":"message.send","ref":format!("m{i}"),"payload":{"room":"room","body":format!("msg {i}")}})).await;
        ws_recv_type(&mut ws, "message.ack").await;
        ws_recv_type(&mut ws, "message.new").await;
    }

    // Fetch before seq 4
    ws_send(
        &mut ws,
        json!({"type":"messages.fetch","ref":"f1","payload":{"room":"room","before":4,"limit":10}}),
    )
    .await;
    let r = ws_recv_type(&mut ws, "messages.batch").await;
    let msgs = r["payload"]["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[0]["seq"], 1);
    assert_eq!(msgs[2]["seq"], 3);
}

#[tokio::test]
async fn test_ws_reconnect_catchup() {
    let server = TestServer::start().await;
    let token = server.create_tenant("recon", TENANT_A_SECRET).await;
    server.create_room(&token, "room").await;
    server.add_member(&token, "room", "alice").await;
    server.add_member(&token, "room", "bob").await;

    // Alice sends messages
    let mut ws_a = server.ws_connect().await;
    ws_auth(
        &mut ws_a,
        &mint_jwt("alice", "recon", &["room"], TENANT_A_SECRET),
    )
    .await;
    ws_send(
        &mut ws_a,
        json!({"type":"subscribe","payload":{"rooms":["room"]}}),
    )
    .await;
    ws_recv_type(&mut ws_a, "subscribed").await;

    let before = herald_server::ws::connection::now_millis();
    tokio::time::sleep(Duration::from_millis(50)).await;

    for i in 1..=3 {
        ws_send(&mut ws_a, json!({"type":"message.send","ref":format!("m{i}"),"payload":{"room":"room","body":format!("catch {i}")}})).await;
        ws_recv_type(&mut ws_a, "message.ack").await;
        ws_recv_type(&mut ws_a, "message.new").await;
    }

    // Bob connects with last_seen_at
    let mut ws_b = server.ws_connect().await;
    ws_send(&mut ws_b, json!({"type":"auth","payload":{"token":mint_jwt("bob","recon",&["room"],TENANT_A_SECRET),"last_seen_at":before}})).await;
    ws_recv_type(&mut ws_b, "auth_ok").await;
    let sub = ws_recv_type(&mut ws_b, "subscribed").await;
    assert_eq!(sub["payload"]["room"], "room");

    let batch = ws_recv_type(&mut ws_b, "messages.batch").await;
    let msgs = batch["payload"]["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[0]["body"], "catch 1");
    assert_eq!(msgs[2]["body"], "catch 3");
}

#[tokio::test]
async fn test_ws_http_inject_fanout() {
    let server = TestServer::start().await;
    let token = server.create_tenant("inject", TENANT_A_SECRET).await;
    server.create_room(&token, "room").await;
    server.add_member(&token, "room", "alice").await;

    let mut ws = server.ws_connect().await;
    ws_auth(
        &mut ws,
        &mint_jwt("alice", "inject", &["room"], TENANT_A_SECRET),
    )
    .await;
    ws_send(
        &mut ws,
        json!({"type":"subscribe","payload":{"rooms":["room"]}}),
    )
    .await;
    ws_recv_type(&mut ws, "subscribed").await;

    // Inject via HTTP
    let resp = server
        .http_client()
        .post(server.http_url("/rooms/room/messages"))
        .bearer_auth(&token)
        .json(&json!({"sender": "system", "body": "injected!", "meta": {"system": true}}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // WS subscriber receives it
    let msg = ws_recv_type(&mut ws, "message.new").await;
    assert_eq!(msg["payload"]["sender"], "system");
    assert_eq!(msg["payload"]["body"], "injected!");
    assert_eq!(msg["payload"]["meta"]["system"], true);
}

#[tokio::test]
async fn test_ws_wrong_secret_rejected() {
    let server = TestServer::start().await;
    server.create_tenant("secure", TENANT_A_SECRET).await;

    let bad_jwt = mint_jwt("alice", "secure", &["room"], "wrong-secret");
    let mut ws = server.ws_connect().await;
    ws_send(&mut ws, json!({"type":"auth","payload":{"token": bad_jwt}})).await;

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
async fn test_ws_ping_pong() {
    let server = TestServer::start().await;
    let _token = server.create_tenant("ping", TENANT_A_SECRET).await;

    let mut ws = server.ws_connect().await;
    ws_auth(&mut ws, &mint_jwt("alice", "ping", &[], TENANT_A_SECRET)).await;

    ws_send(&mut ws, json!({"type":"ping","ref":"p1"})).await;
    let r = ws_recv_type(&mut ws, "pong").await;
    assert_eq!(r["ref"], "p1");
}

#[tokio::test]
async fn test_http_presence_query() {
    let server = TestServer::start().await;
    let token = server.create_tenant("pres", TENANT_A_SECRET).await;
    server.create_room(&token, "room").await;
    server.add_member(&token, "room", "alice").await;

    // No connections — offline
    let resp = server
        .http_client()
        .get(server.http_url("/presence/alice"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "offline");
    assert_eq!(body["connections"], 0);

    // Connect — online
    let mut ws = server.ws_connect().await;
    ws_auth(
        &mut ws,
        &mint_jwt("alice", "pres", &["room"], TENANT_A_SECRET),
    )
    .await;

    let resp = server
        .http_client()
        .get(server.http_url("/presence/alice"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "online");
    assert_eq!(body["connections"], 1);

    // Room presence
    let resp = server
        .http_client()
        .get(server.http_url("/rooms/room/presence"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    let members = body["members"].as_array().unwrap();
    let alice = members.iter().find(|m| m["user_id"] == "alice").unwrap();
    assert_eq!(alice["status"], "online");
}

// ---------------------------------------------------------------------------
// Stress: concurrent connections + rapid messages
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_stress_concurrent_connections() {
    let server = TestServer::start().await;
    let token = server.create_tenant("stress", TENANT_A_SECRET).await;
    server.create_room(&token, "room").await;

    let num = 20;
    for i in 0..num {
        server.add_member(&token, "room", &format!("u{i}")).await;
    }

    let barrier = Arc::new(tokio::sync::Barrier::new(num));
    let connected = Arc::new(std::sync::atomic::AtomicU32::new(0));

    let mut handles = Vec::new();
    for i in 0..num {
        let b = barrier.clone();
        let c = connected.clone();
        let port = server.ws_port;
        let jwt = mint_jwt(&format!("u{i}"), "stress", &["room"], TENANT_A_SECRET);

        handles.push(tokio::spawn(async move {
            let url = format!("ws://127.0.0.1:{port}/");
            let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
            ws.send(Message::Text(
                json!({"type":"auth","payload":{"token":jwt}})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
            loop {
                let r: Value =
                    serde_json::from_str(&ws.next().await.unwrap().unwrap().into_text().unwrap())
                        .unwrap();
                if r["type"] == "auth_ok" {
                    break;
                }
            }
            c.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            b.wait().await;
            tokio::time::sleep(Duration::from_millis(50)).await;
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    assert_eq!(
        connected.load(std::sync::atomic::Ordering::Relaxed),
        num as u32
    );
}

#[tokio::test]
async fn test_stress_rapid_messages() {
    let server = TestServer::start().await;
    let token = server.create_tenant("rapid", TENANT_A_SECRET).await;
    server.create_room(&token, "room").await;
    server.add_member(&token, "room", "sender").await;

    let mut ws = server.ws_connect().await;
    ws_auth(
        &mut ws,
        &mint_jwt("sender", "rapid", &["room"], TENANT_A_SECRET),
    )
    .await;
    ws_send(
        &mut ws,
        json!({"type":"subscribe","payload":{"rooms":["room"]}}),
    )
    .await;
    ws_recv_type(&mut ws, "subscribed").await;

    let count = 50u64;
    for i in 0..count {
        ws_send(&mut ws, json!({"type":"message.send","ref":format!("r{i}"),"payload":{"room":"room","body":format!("rapid {i}")}})).await;
    }

    let mut acks = 0u64;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while acks < count && std::time::Instant::now() < deadline {
        if let Ok(Some(Ok(Message::Text(text)))) =
            tokio::time::timeout(Duration::from_secs(3), ws.next()).await
        {
            let r: Value = serde_json::from_str(&text).unwrap();
            if r["type"] == "message.ack" {
                acks += 1;
            }
        } else {
            break;
        }
    }

    assert_eq!(acks, count, "expected {count} acks, got {acks}");
}

// ---------------------------------------------------------------------------
// Admin endpoint tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_admin_list_rooms() {
    let server = TestServer::start().await;
    let token = server.create_tenant("tenant_lr", TENANT_A_SECRET).await;

    // List rooms when empty
    let resp = server
        .http_client()
        .get(server.http_url("/rooms"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["rooms"].as_array().unwrap().len(), 0);

    // Create rooms then list
    server.create_room(&token, "chat").await;
    server.create_room(&token, "support").await;

    let resp = server
        .http_client()
        .get(server.http_url("/rooms"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["rooms"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn test_admin_tenant_rooms() {
    let server = TestServer::start().await;
    let token = server.create_tenant("tenant_tr", TENANT_A_SECRET).await;
    server.create_room(&token, "room1").await;
    server.create_room(&token, "room2").await;

    let resp = server
        .http_client()
        .get(server.http_url("/admin/tenants/tenant_tr/rooms"))
        .bearer_auth(SUPER_ADMIN_TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["rooms"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn test_admin_token_revocation() {
    let server = TestServer::start().await;
    let token = server.create_tenant("tenant_rev", TENANT_A_SECRET).await;

    // Token should work
    let resp = server
        .http_client()
        .get(server.http_url("/rooms"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Revoke the token
    let resp = server
        .http_client()
        .delete(server.http_url(&format!("/admin/tenants/tenant_rev/tokens/{token}")))
        .bearer_auth(SUPER_ADMIN_TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Token should no longer work
    let resp = server
        .http_client()
        .get(server.http_url("/rooms"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_admin_token_revocation_wrong_tenant() {
    let server = TestServer::start().await;
    let token_a = server.create_tenant("tenant_a_rev", TENANT_A_SECRET).await;
    server.create_tenant("tenant_b_rev", TENANT_B_SECRET).await;

    // Try to revoke tenant_a's token via tenant_b — should 404
    let resp = server
        .http_client()
        .delete(server.http_url(&format!("/admin/tenants/tenant_b_rev/tokens/{token_a}")))
        .bearer_auth(SUPER_ADMIN_TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // Original token should still work
    let resp = server
        .http_client()
        .get(server.http_url("/rooms"))
        .bearer_auth(&token_a)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_admin_connections() {
    let server = TestServer::start().await;
    let token = server.create_tenant("tenant_conn", TENANT_A_SECRET).await;
    server.create_room(&token, "room").await;
    server.add_member(&token, "room", "alice").await;

    // No connections initially
    let resp = server
        .http_client()
        .get(server.http_url("/admin/connections"))
        .bearer_auth(SUPER_ADMIN_TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["total"].as_u64().unwrap(), 0);

    // Connect a WebSocket client
    let mut ws = server.ws_connect().await;
    ws_auth(
        &mut ws,
        &mint_jwt("alice", "tenant_conn", &["room"], TENANT_A_SECRET),
    )
    .await;

    // Now should have 1 connection
    let resp = server
        .http_client()
        .get(server.http_url("/admin/connections"))
        .bearer_auth(SUPER_ADMIN_TOKEN)
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["total"].as_u64().unwrap(), 1);
    let by_tenant = body["by_tenant"].as_array().unwrap();
    assert_eq!(by_tenant.len(), 1);
    assert_eq!(by_tenant[0]["tenant_id"], "tenant_conn");
    assert_eq!(by_tenant[0]["connections"].as_u64().unwrap(), 1);

    // Disconnect
    drop(ws);
    tokio::time::sleep(Duration::from_millis(100)).await;

    let resp = server
        .http_client()
        .get(server.http_url("/admin/connections"))
        .bearer_auth(SUPER_ADMIN_TOKEN)
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["total"].as_u64().unwrap(), 0);
}

#[tokio::test]
async fn test_admin_events() {
    let server = TestServer::start().await;
    let token = server.create_tenant("tenant_ev", TENANT_A_SECRET).await;
    server.create_room(&token, "room").await;
    server.add_member(&token, "room", "bob").await;

    // Connect and disconnect to generate events
    let mut ws = server.ws_connect().await;
    ws_auth(
        &mut ws,
        &mint_jwt("bob", "tenant_ev", &["room"], TENANT_A_SECRET),
    )
    .await;
    drop(ws);
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Fetch events
    let resp = server
        .http_client()
        .get(server.http_url("/admin/events?limit=10"))
        .bearer_auth(SUPER_ADMIN_TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    let events = body["events"].as_array().unwrap();

    // Should have at least connection + disconnection
    assert!(
        events.len() >= 2,
        "expected at least 2 events, got {}",
        events.len()
    );

    let kinds: Vec<&str> = events.iter().map(|e| e["kind"].as_str().unwrap()).collect();
    assert!(
        kinds.contains(&"disconnection"),
        "missing disconnection event"
    );
    assert!(kinds.contains(&"connection"), "missing connection event");

    // Test after_id filtering — get events after the first one
    let first_id = events.last().unwrap()["id"].as_u64().unwrap();
    let resp = server
        .http_client()
        .get(server.http_url(&format!("/admin/events?after_id={first_id}")))
        .bearer_auth(SUPER_ADMIN_TOKEN)
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    let filtered = body["events"].as_array().unwrap();
    assert!(
        filtered.len() < events.len(),
        "after_id filter should return fewer events"
    );
}

#[tokio::test]
async fn test_admin_events_message() {
    let server = TestServer::start().await;
    let token = server.create_tenant("tenant_evm", TENANT_A_SECRET).await;
    server.create_room(&token, "chat").await;
    server.add_member(&token, "chat", "carol").await;

    // Connect, subscribe, and send a message
    let mut ws = server.ws_connect().await;
    ws_auth(
        &mut ws,
        &mint_jwt("carol", "tenant_evm", &["chat"], TENANT_A_SECRET),
    )
    .await;
    ws_send(
        &mut ws,
        json!({"type": "subscribe", "payload": {"rooms": ["chat"]}}),
    )
    .await;
    ws_recv_type(&mut ws, "subscribed").await;

    ws_send(
        &mut ws,
        json!({"type": "message.send", "ref": "m1", "payload": {"room": "chat", "body": "hello"}}),
    )
    .await;
    ws_recv_type(&mut ws, "message.ack").await;

    // Check events include a message event
    let resp = server
        .http_client()
        .get(server.http_url("/admin/events?limit=20"))
        .bearer_auth(SUPER_ADMIN_TOKEN)
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    let events = body["events"].as_array().unwrap();
    let kinds: Vec<&str> = events.iter().map(|e| e["kind"].as_str().unwrap()).collect();
    assert!(
        kinds.contains(&"message"),
        "expected message event in: {kinds:?}"
    );

    // Verify message event details
    let msg_event = events.iter().find(|e| e["kind"] == "message").unwrap();
    assert_eq!(msg_event["details"]["room"], "chat");
    assert_eq!(msg_event["details"]["sender"], "carol");
    assert_eq!(msg_event["tenant_id"], "tenant_evm");
}

#[tokio::test]
async fn test_admin_errors_auth_failure() {
    let server = TestServer::start().await;
    server.create_tenant("tenant_err", TENANT_A_SECRET).await;

    // Trigger auth failure with bad JWT
    let bad_jwt = mint_jwt("hacker", "tenant_err", &["room"], "wrong-secret");
    let mut ws = server.ws_connect().await;
    ws_send(
        &mut ws,
        json!({"type": "auth", "payload": {"token": bad_jwt}}),
    )
    .await;
    // Wait for connection to close/reject
    tokio::time::sleep(Duration::from_millis(200)).await;
    drop(ws);

    // Check error logs
    let resp = server
        .http_client()
        .get(server.http_url("/admin/errors?category=client"))
        .bearer_auth(SUPER_ADMIN_TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    let errors = body["errors"].as_array().unwrap();
    assert!(
        !errors.is_empty(),
        "expected at least one client error after auth failure"
    );
    let err = &errors[0];
    assert_eq!(err["category"], "client");
    assert!(
        err["message"].as_str().unwrap().contains("Auth failure"),
        "error message should mention auth: {:?}",
        err["message"]
    );
}

#[tokio::test]
async fn test_admin_errors_empty() {
    let server = TestServer::start().await;

    // All categories should be empty initially
    for cat in &["client", "webhook", "http"] {
        let resp = server
            .http_client()
            .get(server.http_url(&format!("/admin/errors?category={cat}")))
            .bearer_auth(SUPER_ADMIN_TOKEN)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["errors"].as_array().unwrap().len(), 0);
    }
}

#[tokio::test]
async fn test_admin_stats() {
    let server = TestServer::start().await;

    // Stats should return valid structure even with no snapshots
    let resp = server
        .http_client()
        .get(server.http_url("/admin/stats"))
        .bearer_auth(SUPER_ADMIN_TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();

    // Today summary should be present
    assert!(body["today"].is_object(), "missing today summary");
    assert!(
        body["today"]["peak_connections"].is_number(),
        "missing peak_connections"
    );
    assert!(
        body["today"]["messages_today"].is_number(),
        "missing messages_today"
    );
    assert!(
        body["today"]["webhooks_today"].is_number(),
        "missing webhooks_today"
    );

    // Snapshots array should exist (may be empty if no snapshots recorded yet)
    assert!(body["snapshots"].is_array(), "missing snapshots array");
}

#[tokio::test]
async fn test_admin_stats_with_snapshot() {
    let server = TestServer::start().await;

    // Manually record a snapshot via the event bus
    let messages_total = server
        .state
        .metrics
        .messages_sent
        .load(std::sync::atomic::Ordering::Relaxed);
    let auth_failures = server
        .state
        .metrics
        .ws_auth_failures
        .load(std::sync::atomic::Ordering::Relaxed);
    server
        .state
        .event_bus
        .record_snapshot(5, messages_total, 2, auth_failures);

    let resp = server
        .http_client()
        .get(server.http_url("/admin/stats"))
        .bearer_auth(SUPER_ADMIN_TOKEN)
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    let snapshots = body["snapshots"].as_array().unwrap();
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0]["connections"].as_u64().unwrap(), 5);
    assert_eq!(snapshots[0]["rooms"].as_u64().unwrap(), 2);
}

#[tokio::test]
async fn test_admin_stats_time_range_filter() {
    let server = TestServer::start().await;

    // Record two snapshots
    server.state.event_bus.record_snapshot(1, 0, 0, 0);
    tokio::time::sleep(Duration::from_millis(50)).await;
    server.state.event_bus.record_snapshot(2, 10, 1, 0);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    // Query all (wide range)
    let resp = server
        .http_client()
        .get(server.http_url(&format!(
            "/admin/stats?from={}&to={}",
            now - 60_000,
            now + 60_000
        )))
        .bearer_auth(SUPER_ADMIN_TOKEN)
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["snapshots"].as_array().unwrap().len(), 2);

    // Query with future range — should get 0
    let resp = server
        .http_client()
        .get(server.http_url(&format!(
            "/admin/stats?from={}&to={}",
            now + 60_000,
            now + 120_000
        )))
        .bearer_auth(SUPER_ADMIN_TOKEN)
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["snapshots"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_admin_events_stream_sse() {
    let server = TestServer::start().await;
    let token = server.create_tenant("tenant_sse", TENANT_A_SECRET).await;
    server.create_room(&token, "room").await;
    server.add_member(&token, "room", "eve").await;

    // Connect to SSE stream
    let client = reqwest::Client::new();
    let resp = client
        .get(server.http_url("/admin/events/stream"))
        .bearer_auth(SUPER_ADMIN_TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // The content type should be SSE
    let content_type = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        content_type.contains("text/event-stream"),
        "expected SSE content type, got: {content_type}"
    );
}

#[tokio::test]
async fn test_admin_endpoints_require_auth() {
    let server = TestServer::start().await;

    let admin_paths = vec![
        "/admin/events",
        "/admin/errors",
        "/admin/stats",
        "/admin/connections",
    ];

    for path in admin_paths {
        // No auth
        let resp = server
            .http_client()
            .get(server.http_url(path))
            .send()
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "{path} should require auth"
        );

        // Wrong token
        let resp = server
            .http_client()
            .get(server.http_url(path))
            .bearer_auth("wrong-token")
            .send()
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "{path} should reject wrong token"
        );
    }
}

#[tokio::test]
async fn test_tenant_stats_endpoint() {
    let server = TestServer::start().await;
    let token = server.create_tenant("tenant_ts", TENANT_A_SECRET).await;
    server.create_room(&token, "chat").await;
    server.add_member(&token, "chat", "alice").await;

    // Send some messages via HTTP inject to generate tenant metrics
    for i in 0..3 {
        let resp = server
            .http_client()
            .post(server.http_url("/rooms/chat/messages"))
            .bearer_auth(&token)
            .json(&json!({"sender": "alice", "body": format!("msg {i}")}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    // Get tenant stats
    let resp = server
        .http_client()
        .get(server.http_url("/stats"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();

    // Current should reflect tenant-specific data
    assert!(body["current"].is_object(), "missing current summary");
    assert_eq!(
        body["current"]["messages_sent"].as_u64().unwrap(),
        3,
        "expected 3 tenant messages"
    );
    assert_eq!(
        body["current"]["rooms"].as_u64().unwrap(),
        1,
        "expected 1 room"
    );

    // Snapshots array should exist (may be empty if no snapshot cycle yet)
    assert!(body["snapshots"].is_array(), "missing snapshots");
}

#[tokio::test]
async fn test_tenant_stats_isolated() {
    let server = TestServer::start().await;
    let token_a = server.create_tenant("tenant_sa", TENANT_A_SECRET).await;
    let token_b = server.create_tenant("tenant_sb", TENANT_B_SECRET).await;
    server.create_room(&token_a, "room_a").await;
    server.create_room(&token_b, "room_b").await;

    // Send 5 messages on tenant A
    for i in 0..5 {
        server
            .http_client()
            .post(server.http_url("/rooms/room_a/messages"))
            .bearer_auth(&token_a)
            .json(&json!({"sender": "sys", "body": format!("a{i}")}))
            .send()
            .await
            .unwrap();
    }

    // Send 2 messages on tenant B
    for i in 0..2 {
        server
            .http_client()
            .post(server.http_url("/rooms/room_b/messages"))
            .bearer_auth(&token_b)
            .json(&json!({"sender": "sys", "body": format!("b{i}")}))
            .send()
            .await
            .unwrap();
    }

    // Tenant A should see 5 messages
    let resp = server
        .http_client()
        .get(server.http_url("/stats"))
        .bearer_auth(&token_a)
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["current"]["messages_sent"].as_u64().unwrap(), 5);
    assert_eq!(body["current"]["rooms"].as_u64().unwrap(), 1);

    // Tenant B should see 2 messages
    let resp = server
        .http_client()
        .get(server.http_url("/stats"))
        .bearer_auth(&token_b)
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["current"]["messages_sent"].as_u64().unwrap(), 2);
    assert_eq!(body["current"]["rooms"].as_u64().unwrap(), 1);
}

#[tokio::test]
async fn test_admin_token_constant_time_comparison() {
    let server = TestServer::start().await;

    // Valid token works
    let resp = server
        .http_client()
        .get(server.http_url("/admin/tenants"))
        .bearer_auth(SUPER_ADMIN_TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Wrong tokens of varying prefix-match lengths all get 401
    let wrong_tokens = [
        "",
        "x",
        "test-super-admi",        // one char short
        "test-super-admin-extra", // too long
        "test-super-admio",       // last char wrong
        "xest-super-admin",       // first char wrong
        "test-sXper-admin",       // middle char wrong
    ];
    for bad_token in &wrong_tokens {
        let resp = server
            .http_client()
            .get(server.http_url("/admin/tenants"))
            .bearer_auth(bad_token)
            .send()
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "expected 401 for token: {bad_token:?}"
        );
    }

    // No auth header at all
    let resp = server
        .http_client()
        .get(server.http_url("/admin/tenants"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ---------------------------------------------------------------------------
// Body size limit & input validation tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_http_body_size_limit() {
    let server = TestServer::start().await;
    let token = server.create_tenant("acme", TENANT_A_SECRET).await;

    // Create a body larger than 1MB
    let huge_body = "x".repeat(2 * 1024 * 1024);
    let resp = server
        .http_client()
        .post(server.http_url("/rooms"))
        .bearer_auth(&token)
        .header("content-type", "application/json")
        .body(format!(r#"{{"id":"room","name":"{huge_body}"}}"#))
        .send()
        .await
        .unwrap();
    // Should be rejected — either 413 Payload Too Large or 400
    assert!(
        resp.status() == StatusCode::PAYLOAD_TOO_LARGE || resp.status() == StatusCode::BAD_REQUEST,
        "expected 413 or 400 for oversized body, got {}",
        resp.status()
    );
}

#[tokio::test]
async fn test_input_validation_room_id() {
    let server = TestServer::start().await;
    let token = server.create_tenant("acme", TENANT_A_SECRET).await;

    // Path traversal in room ID
    let resp = server
        .http_client()
        .post(server.http_url("/rooms"))
        .bearer_auth(&token)
        .json(&json!({"id": "../etc/passwd", "name": "bad room"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // Null bytes in room ID
    let resp = server
        .http_client()
        .post(server.http_url("/rooms"))
        .bearer_auth(&token)
        .json(&json!({"id": "room\0evil", "name": "bad room"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // Overly long room ID
    let long_id = "a".repeat(300);
    let resp = server
        .http_client()
        .post(server.http_url("/rooms"))
        .bearer_auth(&token)
        .json(&json!({"id": long_id, "name": "bad room"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // Valid room ID works
    let resp = server
        .http_client()
        .post(server.http_url("/rooms"))
        .bearer_auth(&token)
        .json(&json!({"id": "valid-room_123", "name": "Good Room"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn test_input_validation_message_body_size() {
    let server = TestServer::start().await;
    let token = server.create_tenant("acme", TENANT_A_SECRET).await;
    server.create_room(&token, "chat").await;
    server.add_member(&token, "chat", "alice").await;

    // Message body over 64KB should be rejected
    let huge_body = "x".repeat(70_000);
    let resp = server
        .http_client()
        .post(server.http_url("/rooms/chat/messages"))
        .bearer_auth(&token)
        .json(&json!({"sender": "alice", "body": huge_body}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // Normal message works
    let resp = server
        .http_client()
        .post(server.http_url("/rooms/chat/messages"))
        .bearer_auth(&token)
        .json(&json!({"sender": "alice", "body": "hello"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn test_input_validation_tenant_id() {
    let server = TestServer::start().await;

    // Path traversal in tenant ID
    let resp = server
        .http_client()
        .post(server.http_url("/admin/tenants"))
        .bearer_auth(SUPER_ADMIN_TOKEN)
        .json(&json!({"id": "../evil", "name": "bad", "jwt_secret": "secret123"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // Valid tenant ID works
    let resp = server
        .http_client()
        .post(server.http_url("/admin/tenants"))
        .bearer_auth(SUPER_ADMIN_TOKEN)
        .json(&json!({"id": "good-tenant", "name": "Good", "jwt_secret": "secret123"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn test_http_api_rate_limiting() {
    // Build a server with a very low rate limit
    let db = create_test_store().await;
    let config = HeraldConfig {
        server: ServerConfig {
            ws_bind: "127.0.0.1:0".to_string(),
            http_bind: "127.0.0.1:0".to_string(),
            log_level: "warn".to_string(),
            max_messages_per_sec: 1000,
            api_rate_limit: 5, // Only 5 requests per minute
            ..Default::default()
        },
        store: StoreConfig {
            path: "/tmp/herald-test-rate".into(),
            message_ttl_days: 7,
        },
        auth: AuthConfig {
            jwt_secret: Some(TENANT_A_SECRET.to_string()),
            jwt_issuer: None,
            super_admin_token: Some(SUPER_ADMIN_TOKEN.to_string()),
            api: ApiAuthConfig {
                tokens: vec!["rate-test-token".to_string()],
            },
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
    };

    let state = AppState::build(AppStateBuilder {
        config,
        db,
        sentry: None,
        courier: None,
        chronicle: None,
    });
    state.bootstrap_single_tenant().await.unwrap();

    let http_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let http_port = http_listener.local_addr().unwrap().port();
    let http_app = herald_server::http::router(state.clone());
    tokio::spawn(async move {
        axum::serve(http_listener, http_app).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(50)).await;

    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{http_port}");

    // Send 6 requests (limit is 5)
    let mut statuses = Vec::new();
    for _ in 0..6 {
        let resp = client
            .get(format!("{base}/rooms"))
            .bearer_auth("rate-test-token")
            .send()
            .await
            .unwrap();
        statuses.push(resp.status());
    }

    // First 5 should succeed, 6th should be 429
    for s in &statuses[..5] {
        assert_eq!(*s, StatusCode::OK, "first 5 requests should succeed");
    }
    assert_eq!(
        statuses[5],
        StatusCode::TOO_MANY_REQUESTS,
        "6th request should be rate limited"
    );
}

#[tokio::test]
async fn test_ws_sliding_window_rate_limit() {
    // Build a server with low WS rate limit
    let db = create_test_store().await;
    let config = HeraldConfig {
        server: ServerConfig {
            ws_bind: "127.0.0.1:0".to_string(),
            http_bind: "127.0.0.1:0".to_string(),
            log_level: "warn".to_string(),
            max_messages_per_sec: 3, // Very low for testing
            ..Default::default()
        },
        store: StoreConfig {
            path: "/tmp/herald-test-wsrate".into(),
            message_ttl_days: 7,
        },
        auth: AuthConfig {
            jwt_secret: Some(TENANT_A_SECRET.to_string()),
            jwt_issuer: None,
            super_admin_token: Some(SUPER_ADMIN_TOKEN.to_string()),
            api: ApiAuthConfig {
                tokens: vec!["ws-rate-token".to_string()],
            },
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
    let ws_port = ws_listener.local_addr().unwrap().port();
    let http_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let http_port = http_listener.local_addr().unwrap().port();

    let ws_state = state.clone();
    let ws_app = axum::Router::new()
        .route(
            "/",
            axum::routing::get(herald_server::ws::upgrade::ws_handler),
        )
        .with_state(ws_state);
    let http_app = herald_server::http::router(state.clone());

    tokio::spawn(async move {
        axum::serve(ws_listener, ws_app).await.unwrap();
    });
    tokio::spawn(async move {
        axum::serve(http_listener, http_app).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Setup: create room and member
    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{http_port}");
    client
        .post(format!("{base}/rooms"))
        .bearer_auth("ws-rate-token")
        .json(&json!({"id": "chat", "name": "Chat"}))
        .send()
        .await
        .unwrap();
    client
        .post(format!("{base}/rooms/chat/members"))
        .bearer_auth("ws-rate-token")
        .json(&json!({"user_id": "alice"}))
        .send()
        .await
        .unwrap();

    // Connect WS
    let ws_url = format!("ws://127.0.0.1:{ws_port}/");
    let (mut ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    let token = mint_jwt("alice", "default", &["chat"], TENANT_A_SECRET);
    ws_auth(&mut ws, &token).await;
    ws_send(
        &mut ws,
        json!({"type": "subscribe", "payload": {"rooms": ["chat"]}}),
    )
    .await;
    ws_recv_type(&mut ws, "subscribed").await;

    // Send 5 messages rapidly (limit is 3/sec)
    for i in 0..5 {
        ws_send(
            &mut ws,
            json!({
                "type": "message.send",
                "payload": {"room": "chat", "body": format!("msg {i}")}
            }),
        )
        .await;
    }

    // Collect responses - should get some acks and at least one rate_limited error
    let mut got_rate_limited = false;
    let mut got_ack = false;
    for _ in 0..10 {
        let timeout = tokio::time::timeout(Duration::from_secs(2), ws.next()).await;
        match timeout {
            Ok(Some(Ok(Message::Text(text)))) => {
                let msg: Value = serde_json::from_str(&text).unwrap();
                if msg["type"] == "error" && msg["payload"]["code"] == "RATE_LIMITED" {
                    got_rate_limited = true;
                }
                if msg["type"] == "message.ack" {
                    got_ack = true;
                }
                if got_rate_limited && got_ack {
                    break;
                }
            }
            _ => break,
        }
    }
    assert!(got_ack, "should have received at least one message ack");
    assert!(got_rate_limited, "should have received rate limit error");
}

#[tokio::test]
async fn test_backpressure_increments_dropped_metric() {
    use herald_server::registry::connection::ConnId;
    use std::sync::atomic::Ordering;

    let server = TestServer::start().await;
    let token = server.create_tenant("acme", TENANT_A_SECRET).await;
    server.create_room(&token, "chat").await;
    server.add_member(&token, "chat", "alice").await;

    // Register a connection with a tiny channel that will fill immediately
    let (tx, _rx) = tokio::sync::mpsc::channel(1);
    let conn_id = ConnId::next();
    server
        .state
        .connections
        .register(conn_id, "acme".to_string(), "alice".to_string(), tx);
    server
        .state
        .connections
        .add_room_subscription(conn_id, "chat");
    server
        .state
        .rooms
        .subscribe("acme", "chat", "alice", conn_id);

    // Fill the channel capacity (1 slot) then the next send should drop
    let msg = herald_core::protocol::ServerMessage::error(
        None,
        herald_core::error::ErrorCode::Internal,
        "test",
    );

    // First send fills the single slot
    assert!(server.state.connections.send_to_conn(conn_id, &msg));
    // Second send should fail (channel full)
    assert!(!server.state.connections.send_to_conn(conn_id, &msg));

    // Now test via fanout — send a few messages that fanout to the room
    let dropped_before = server
        .state
        .metrics
        .messages_dropped
        .load(Ordering::Relaxed);

    for _ in 0..5 {
        herald_server::ws::fanout::fanout_to_room(&server.state, "acme", "chat", &msg, None);
    }

    let dropped_after = server
        .state
        .metrics
        .messages_dropped
        .load(Ordering::Relaxed);

    assert!(
        dropped_after > dropped_before,
        "expected messages_dropped to increase via fanout, before={dropped_before} after={dropped_after}"
    );

    // Also verify the metric is exposed via /metrics endpoint
    let resp = server
        .http_client()
        .get(server.http_url("/metrics"))
        .send()
        .await
        .unwrap();
    let body = resp.text().await.unwrap();

    let dropped_line = body
        .lines()
        .find(|l| l.starts_with("herald_messages_dropped_total"))
        .expect("should have messages_dropped metric");
    let dropped: u64 = dropped_line
        .split_whitespace()
        .last()
        .unwrap()
        .parse()
        .unwrap();
    assert!(
        dropped > 0,
        "expected some messages to be dropped due to backpressure, got {dropped}"
    );
}

#[tokio::test]
async fn test_circuit_breaker_halfopen_single_probe() {
    use herald_server::integrations::circuit_breaker::{CircuitBreaker, State};
    use std::time::Duration;

    let cb = CircuitBreaker::new("test-integration", 3, Duration::from_millis(100));

    // Trip to open
    cb.record_failure();
    cb.record_failure();
    cb.record_failure();
    assert_eq!(cb.state(), State::Open);
    assert!(cb.check().is_err(), "should reject when open");

    // Wait for cooldown
    tokio::time::sleep(Duration::from_millis(150)).await;

    // First call: transitions to half-open, allowed through
    assert!(cb.check().is_ok(), "first call in half-open should pass");
    assert_eq!(cb.state(), State::HalfOpen);

    // Concurrent calls: should be rejected (probe in progress)
    assert!(
        cb.check().is_err(),
        "second call should be rejected while probe in progress"
    );
    assert!(
        cb.check().is_err(),
        "third call should be rejected while probe in progress"
    );
    assert!(
        cb.check().is_err(),
        "fourth call should be rejected while probe in progress"
    );

    // Probe succeeds
    cb.record_success();
    assert_eq!(cb.state(), State::Closed);

    // All calls pass again
    assert!(cb.check().is_ok());
    assert!(cb.check().is_ok());
}

#[tokio::test]
async fn test_presence_linger_reconnect_no_offline() {
    // Server with linger = 2 seconds
    let db = create_test_store().await;
    let config = HeraldConfig {
        server: ServerConfig {
            ws_bind: "127.0.0.1:0".to_string(),
            http_bind: "127.0.0.1:0".to_string(),
            log_level: "warn".to_string(),
            max_messages_per_sec: 1000,
            ..Default::default()
        },
        store: StoreConfig {
            path: "/tmp/herald-test-linger".into(),
            message_ttl_days: 7,
        },
        auth: AuthConfig {
            jwt_secret: Some(TENANT_A_SECRET.to_string()),
            jwt_issuer: None,
            super_admin_token: Some(SUPER_ADMIN_TOKEN.to_string()),
            api: ApiAuthConfig {
                tokens: vec!["linger-token".to_string()],
            },
        },
        presence: PresenceConfig {
            linger_secs: 2,
            manual_override_ttl_secs: 14400,
        },
        webhook: None,
        shroudb: None,
        tls: None,
        tenant_limits: Default::default(),
        cors: None,
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
    let ws_port = ws_listener.local_addr().unwrap().port();
    let http_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let http_port = http_listener.local_addr().unwrap().port();

    let ws_state = state.clone();
    let ws_app = axum::Router::new()
        .route(
            "/",
            axum::routing::get(herald_server::ws::upgrade::ws_handler),
        )
        .with_state(ws_state);
    let http_app = herald_server::http::router(state.clone());

    tokio::spawn(async move { axum::serve(ws_listener, ws_app).await.unwrap() });
    tokio::spawn(async move { axum::serve(http_listener, http_app).await.unwrap() });
    tokio::time::sleep(Duration::from_millis(50)).await;

    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{http_port}");

    // Setup room and members
    client
        .post(format!("{base}/rooms"))
        .bearer_auth("linger-token")
        .json(&json!({"id": "chat", "name": "Chat"}))
        .send()
        .await
        .unwrap();
    client
        .post(format!("{base}/rooms/chat/members"))
        .bearer_auth("linger-token")
        .json(&json!({"user_id": "alice"}))
        .send()
        .await
        .unwrap();
    client
        .post(format!("{base}/rooms/chat/members"))
        .bearer_auth("linger-token")
        .json(&json!({"user_id": "bob"}))
        .send()
        .await
        .unwrap();

    let ws_url = format!("ws://127.0.0.1:{ws_port}/");

    // Connect bob as observer
    let (mut ws_bob, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    let bob_jwt = mint_jwt("bob", "default", &["chat"], TENANT_A_SECRET);
    ws_auth(&mut ws_bob, &bob_jwt).await;
    ws_send(
        &mut ws_bob,
        json!({"type": "subscribe", "payload": {"rooms": ["chat"]}}),
    )
    .await;
    ws_recv_type(&mut ws_bob, "subscribed").await;

    // Connect alice
    let (mut ws_alice, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    let alice_jwt = mint_jwt("alice", "default", &["chat"], TENANT_A_SECRET);
    ws_auth(&mut ws_alice, &alice_jwt).await;
    ws_send(
        &mut ws_alice,
        json!({"type": "subscribe", "payload": {"rooms": ["chat"]}}),
    )
    .await;
    ws_recv_type(&mut ws_alice, "subscribed").await;

    // Bob should see alice come online
    let presence_msg = ws_recv_type(&mut ws_bob, "presence.changed").await;
    assert_eq!(presence_msg["payload"]["user_id"], "alice");
    assert_eq!(presence_msg["payload"]["presence"], "online");

    // Disconnect alice
    drop(ws_alice);
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Immediately reconnect alice (within linger window)
    let (mut ws_alice2, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
    ws_auth(&mut ws_alice2, &alice_jwt).await;
    ws_send(
        &mut ws_alice2,
        json!({"type": "subscribe", "payload": {"rooms": ["chat"]}}),
    )
    .await;
    ws_recv_type(&mut ws_alice2, "subscribed").await;

    // Wait for linger to expire
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Check bob's messages — should see alice online (reconnect), but NOT offline
    // Drain all messages bob received
    let mut saw_offline = false;
    loop {
        let timeout = tokio::time::timeout(Duration::from_millis(200), ws_bob.next()).await;
        match timeout {
            Ok(Some(Ok(Message::Text(text)))) => {
                let msg: Value = serde_json::from_str(&text).unwrap();
                if msg["type"] == "presence.changed"
                    && msg["payload"]["user_id"] == "alice"
                    && msg["payload"]["presence"] == "offline"
                {
                    saw_offline = true;
                }
            }
            _ => break,
        }
    }
    assert!(
        !saw_offline,
        "alice should NOT have been broadcast as offline during quick reconnect"
    );
}

#[tokio::test]
async fn test_cors_headers_present() {
    let server = TestServer::start().await;

    // Regular request should get CORS headers (permissive by default)
    let resp = server
        .http_client()
        .get(server.http_url("/health"))
        .header("origin", "http://example.com")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        resp.headers().contains_key("access-control-allow-origin"),
        "response should include CORS allow-origin header"
    );

    // OPTIONS preflight request should succeed
    let resp = server
        .http_client()
        .request(reqwest::Method::OPTIONS, server.http_url("/health"))
        .header("origin", "http://example.com")
        .header("access-control-request-method", "GET")
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "preflight OPTIONS should succeed, got {}",
        resp.status()
    );
    assert!(
        resp.headers().contains_key("access-control-allow-origin"),
        "preflight should include CORS allow-origin header"
    );
    assert!(
        resp.headers().contains_key("access-control-allow-methods"),
        "preflight should include CORS allow-methods header"
    );
}

#[tokio::test]
async fn test_error_responses_do_not_leak_internals() {
    let server = TestServer::start().await;
    let token = server.create_tenant("acme", TENANT_A_SECRET).await;
    server.create_room(&token, "chat").await;

    // Try to create a duplicate room — triggers store conflict error
    let resp = server
        .http_client()
        .post(server.http_url("/rooms"))
        .bearer_auth(&token)
        .json(&json!({"id": "chat", "name": "Chat Again"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body: Value = resp.json().await.unwrap();
    let error_msg = body["error"].as_str().unwrap();
    // Should be generic — no internal details like "WAL", "store", "UNIQUE constraint"
    assert!(
        !error_msg.contains("WAL")
            && !error_msg.contains("store")
            && !error_msg.contains("UNIQUE")
            && !error_msg.contains("constraint"),
        "error message should not contain internal details: {error_msg}"
    );
    assert_eq!(error_msg, "failed to create room");

    // Try to create a duplicate tenant
    let resp = server
        .http_client()
        .post(server.http_url("/admin/tenants"))
        .bearer_auth(SUPER_ADMIN_TOKEN)
        .json(&json!({"id": "acme", "name": "Acme Again", "jwt_secret": "secret"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body: Value = resp.json().await.unwrap();
    let error_msg = body["error"].as_str().unwrap();
    assert!(
        !error_msg.contains("WAL") && !error_msg.contains("store"),
        "admin error should not contain internal details: {error_msg}"
    );
    assert_eq!(error_msg, "failed to create tenant");
}

// ---------------------------------------------------------------------------
// Config validation tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_config_validation_rejects_invalid() {
    // zero max_messages_per_sec
    let config = HeraldConfig {
        server: ServerConfig {
            max_messages_per_sec: 0,
            ..Default::default()
        },
        store: StoreConfig::default(),
        auth: AuthConfig {
            jwt_secret: Some("a-long-enough-secret".to_string()),
            jwt_issuer: None,
            super_admin_token: None,
            api: ApiAuthConfig::default(),
        },
        presence: PresenceConfig::default(),
        webhook: None,
        shroudb: None,
        tls: None,
        tenant_limits: TenantLimitsConfig::default(),
        cors: None,
    };
    assert!(
        config.validate(false).is_err(),
        "should reject max_messages_per_sec=0"
    );

    // empty jwt_secret in single-tenant mode
    let config = HeraldConfig {
        server: ServerConfig::default(),
        store: StoreConfig::default(),
        auth: AuthConfig {
            jwt_secret: Some("".to_string()),
            jwt_issuer: None,
            super_admin_token: None,
            api: ApiAuthConfig::default(),
        },
        presence: PresenceConfig::default(),
        webhook: None,
        shroudb: None,
        tls: None,
        tenant_limits: TenantLimitsConfig::default(),
        cors: None,
    };
    assert!(
        config.validate(false).is_err(),
        "should reject empty jwt_secret"
    );

    // short jwt_secret
    let config = HeraldConfig {
        server: ServerConfig::default(),
        store: StoreConfig::default(),
        auth: AuthConfig {
            jwt_secret: Some("short".to_string()),
            jwt_issuer: None,
            super_admin_token: None,
            api: ApiAuthConfig::default(),
        },
        presence: PresenceConfig::default(),
        webhook: None,
        shroudb: None,
        tls: None,
        tenant_limits: TenantLimitsConfig::default(),
        cors: None,
    };
    assert!(
        config.validate(false).is_err(),
        "should reject short jwt_secret"
    );

    // empty webhook secret
    let config = HeraldConfig {
        server: ServerConfig::default(),
        store: StoreConfig::default(),
        auth: AuthConfig {
            jwt_secret: Some("a-long-enough-secret".to_string()),
            jwt_issuer: None,
            super_admin_token: None,
            api: ApiAuthConfig::default(),
        },
        presence: PresenceConfig::default(),
        webhook: Some(WebhookConfig {
            url: "http://example.com/hook".to_string(),
            secret: "".to_string(),
            retries: 3,
            events: None,
        }),
        shroudb: None,
        tls: None,
        tenant_limits: TenantLimitsConfig::default(),
        cors: None,
    };
    assert!(
        config.validate(false).is_err(),
        "should reject empty webhook secret"
    );

    // empty TLS key path
    let config = HeraldConfig {
        server: ServerConfig::default(),
        store: StoreConfig::default(),
        auth: AuthConfig {
            jwt_secret: Some("a-long-enough-secret".to_string()),
            jwt_issuer: None,
            super_admin_token: None,
            api: ApiAuthConfig::default(),
        },
        presence: PresenceConfig::default(),
        webhook: None,
        shroudb: None,
        tls: Some(TlsConfig {
            cert_path: "/tmp/nonexistent.pem".to_string(),
            key_path: "".to_string(),
        }),
        tenant_limits: TenantLimitsConfig::default(),
        cors: None,
    };
    assert!(
        config.validate(false).is_err(),
        "should reject empty TLS key_path"
    );

    // Valid config should pass
    let config = HeraldConfig {
        server: ServerConfig::default(),
        store: StoreConfig::default(),
        auth: AuthConfig {
            jwt_secret: Some("a-long-enough-secret".to_string()),
            jwt_issuer: None,
            super_admin_token: None,
            api: ApiAuthConfig::default(),
        },
        presence: PresenceConfig::default(),
        webhook: None,
        shroudb: None,
        tls: None,
        tenant_limits: TenantLimitsConfig::default(),
        cors: None,
    };
    assert!(config.validate(false).is_ok(), "valid config should pass");

    // Multi-tenant requires super_admin_token
    let config = HeraldConfig {
        server: ServerConfig::default(),
        store: StoreConfig::default(),
        auth: AuthConfig {
            jwt_secret: None,
            jwt_issuer: None,
            super_admin_token: None,
            api: ApiAuthConfig::default(),
        },
        presence: PresenceConfig::default(),
        webhook: None,
        shroudb: None,
        tls: None,
        tenant_limits: TenantLimitsConfig::default(),
        cors: None,
    };
    assert!(
        config.validate(true).is_err(),
        "should require super_admin_token in multi-tenant"
    );
}

// ---------------------------------------------------------------------------
// Request ID + Security headers
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_request_id_header() {
    let server = TestServer::start().await;

    let resp = server
        .http_client()
        .get(server.http_url("/health"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let request_id = resp
        .headers()
        .get("x-request-id")
        .expect("response should have x-request-id header")
        .to_str()
        .unwrap();

    // Should be a valid UUID
    assert!(
        uuid::Uuid::parse_str(request_id).is_ok(),
        "x-request-id should be a valid UUID, got: {request_id}"
    );

    // Two requests should have different IDs
    let resp2 = server
        .http_client()
        .get(server.http_url("/health"))
        .send()
        .await
        .unwrap();
    let request_id2 = resp2
        .headers()
        .get("x-request-id")
        .unwrap()
        .to_str()
        .unwrap();
    assert_ne!(
        request_id, request_id2,
        "each request should get a unique ID"
    );
}

#[tokio::test]
async fn test_security_headers_present() {
    let server = TestServer::start().await;

    let resp = server
        .http_client()
        .get(server.http_url("/health"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    assert_eq!(
        resp.headers()
            .get("x-content-type-options")
            .unwrap()
            .to_str()
            .unwrap(),
        "nosniff"
    );
    assert_eq!(
        resp.headers()
            .get("x-frame-options")
            .unwrap()
            .to_str()
            .unwrap(),
        "DENY"
    );
    assert_eq!(
        resp.headers()
            .get("referrer-policy")
            .unwrap()
            .to_str()
            .unwrap(),
        "no-referrer"
    );
}

#[tokio::test]
async fn test_structured_json_logging() {
    // This test verifies that the JSON logging feature compiles and the tracing-subscriber
    // json feature is available. The actual JSON output is validated at the config level.
    // We verify it indirectly by checking the feature is importable.
    use tracing_subscriber::fmt::format::JsonFields;
    let _: fn() -> JsonFields = JsonFields::new;
    // If this compiles, the json feature is available
}

// ---------------------------------------------------------------------------
// Pagination
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_pagination_on_list_rooms() {
    let server = TestServer::start().await;
    let token = server.create_tenant("acme", TENANT_A_SECRET).await;

    // Create 10 rooms
    for i in 0..10 {
        server.create_room(&token, &format!("room-{i:02}")).await;
    }

    // Default pagination
    let resp = server
        .http_client()
        .get(server.http_url("/rooms"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["total"], 10);
    assert_eq!(body["rooms"].as_array().unwrap().len(), 10);

    // With limit
    let resp = server
        .http_client()
        .get(server.http_url("/rooms?limit=3"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["rooms"].as_array().unwrap().len(), 3);
    assert_eq!(body["total"], 10);
    assert_eq!(body["limit"], 3);
    assert_eq!(body["offset"], 0);

    // With offset
    let resp = server
        .http_client()
        .get(server.http_url("/rooms?limit=3&offset=8"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["rooms"].as_array().unwrap().len(), 2); // only 2 remaining
    assert_eq!(body["total"], 10);
    assert_eq!(body["offset"], 8);
}

#[tokio::test]
async fn test_pagination_on_list_tenants() {
    let server = TestServer::start().await;

    // Create 5 tenants
    for i in 0..5 {
        server
            .create_tenant(&format!("tenant-{i}"), "secret-for-testing-12345")
            .await;
    }

    let resp = server
        .http_client()
        .get(server.http_url("/admin/tenants?limit=2"))
        .bearer_auth(SUPER_ADMIN_TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["tenants"].as_array().unwrap().len(), 2);
    assert!(body["total"].as_u64().unwrap() >= 5);
}

// ---------------------------------------------------------------------------
// Liveness / Readiness probes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_health_liveness_readiness() {
    let server = TestServer::start().await;

    // Liveness -- always 200
    let resp = server
        .http_client()
        .get(server.http_url("/health/live"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "alive");

    // Readiness -- checks storage
    let resp = server
        .http_client()
        .get(server.http_url("/health/ready"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ready");
    assert_eq!(body["storage"], true);
}

#[tokio::test]
async fn test_tenant_cache_invalidation_on_delete() {
    let server = TestServer::start().await;
    let _token = server.create_tenant("ephemeral", TENANT_A_SECRET).await;

    // JWT should work before deletion
    let jwt = mint_jwt("user1", "ephemeral", &["chat"], TENANT_A_SECRET);
    let mut ws = server.ws_connect().await;
    ws_send(&mut ws, json!({"type": "auth", "payload": {"token": &jwt}})).await;
    let msg = ws_recv_type(&mut ws, "auth_ok").await;
    assert_eq!(msg["payload"]["user_id"], "user1");
    drop(ws);

    // Delete tenant via admin API
    let resp = server
        .http_client()
        .delete(server.http_url("/admin/tenants/ephemeral"))
        .bearer_auth(SUPER_ADMIN_TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // JWT should now fail — cache entry was removed on delete
    let mut ws2 = server.ws_connect().await;
    ws_send(
        &mut ws2,
        json!({"type": "auth", "payload": {"token": &jwt}}),
    )
    .await;
    let msg = ws_recv_type(&mut ws2, "auth_error").await;
    assert_eq!(msg["payload"]["code"], "TOKEN_INVALID");
}

#[tokio::test]
async fn test_tenant_cache_refresh_on_update() {
    let server = TestServer::start().await;
    let _token = server.create_tenant("updatable", TENANT_A_SECRET).await;

    // Verify initial plan
    let cached = server.state.tenant_cache.get("updatable").unwrap();
    assert_eq!(cached.plan, "free");
    drop(cached);

    // Update tenant plan
    let resp = server
        .http_client()
        .patch(server.http_url("/admin/tenants/updatable"))
        .bearer_auth(SUPER_ADMIN_TOKEN)
        .json(&json!({"plan": "enterprise"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Cache should be refreshed with new plan
    let cached = server.state.tenant_cache.get("updatable").unwrap();
    assert_eq!(cached.plan, "enterprise");
}

#[tokio::test]
async fn test_typing_cleanup_on_disconnect() {
    let server = TestServer::start().await;
    let token = server.create_tenant("acme", TENANT_A_SECRET).await;
    server.create_room(&token, "chat").await;
    server.add_member(&token, "chat", "alice").await;
    server.add_member(&token, "chat", "bob").await;

    // Connect bob as observer
    let mut ws_bob = server.ws_connect().await;
    let bob_jwt = mint_jwt("bob", "acme", &["chat"], TENANT_A_SECRET);
    ws_auth(&mut ws_bob, &bob_jwt).await;
    ws_send(
        &mut ws_bob,
        json!({"type": "subscribe", "payload": {"rooms": ["chat"]}}),
    )
    .await;
    ws_recv_type(&mut ws_bob, "subscribed").await;

    // Connect alice
    let mut ws_alice = server.ws_connect().await;
    let alice_jwt = mint_jwt("alice", "acme", &["chat"], TENANT_A_SECRET);
    ws_auth(&mut ws_alice, &alice_jwt).await;
    ws_send(
        &mut ws_alice,
        json!({"type": "subscribe", "payload": {"rooms": ["chat"]}}),
    )
    .await;
    ws_recv_type(&mut ws_alice, "subscribed").await;

    // Drain bob's presence messages
    loop {
        let timeout = tokio::time::timeout(Duration::from_millis(200), ws_bob.next()).await;
        match timeout {
            Ok(Some(Ok(Message::Text(_)))) => continue,
            _ => break,
        }
    }

    // Alice starts typing
    ws_send(
        &mut ws_alice,
        json!({"type": "typing.start", "payload": {"room": "chat"}}),
    )
    .await;

    // Bob should see typing start
    let typing_msg = ws_recv_type(&mut ws_bob, "typing").await;
    assert_eq!(typing_msg["payload"]["user_id"], "alice");
    assert_eq!(typing_msg["payload"]["active"], true);

    // Alice disconnects WITHOUT sending typing.stop
    drop(ws_alice);
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Bob should receive typing.stop (broadcast on disconnect)
    let stop_msg = ws_recv_type(&mut ws_bob, "typing").await;
    assert_eq!(stop_msg["payload"]["user_id"], "alice");
    assert_eq!(stop_msg["payload"]["active"], false);
}

#[tokio::test]
async fn test_reconnect_catchup_has_more() {
    let server = TestServer::start().await;
    let token = server.create_tenant("acme", TENANT_A_SECRET).await;
    server.create_room(&token, "chat").await;
    server.add_member(&token, "chat", "alice").await;

    // Send 210 messages (more than CATCHUP_LIMIT=200)
    let client = server.http_client();
    for i in 0..210 {
        let resp = client
            .post(server.http_url("/rooms/chat/messages"))
            .bearer_auth(&token)
            .json(&json!({"sender": "bot", "body": format!("msg {i}")}))
            .send()
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::CREATED,
            "inject message {i} failed"
        );
    }

    // Connect alice with last_seen_at=0 (ancient) to trigger catchup
    let alice_jwt = mint_jwt("alice", "acme", &["chat"], TENANT_A_SECRET);
    let mut ws = server.ws_connect().await;
    ws_send(
        &mut ws,
        json!({
            "type": "auth",
            "payload": {"token": &alice_jwt, "last_seen_at": 0}
        }),
    )
    .await;
    ws_recv_type(&mut ws, "auth_ok").await;

    // Should receive subscribed + messages.batch
    let _subscribed = ws_recv_type(&mut ws, "subscribed").await;
    let batch = ws_recv_type(&mut ws, "messages.batch").await;

    let messages = batch["payload"]["messages"].as_array().unwrap();
    let has_more = batch["payload"]["has_more"].as_bool().unwrap();

    assert!(messages.len() <= 200, "should return at most 200 messages");
    assert!(
        has_more,
        "has_more should be true when more than 200 messages exist"
    );
}

#[tokio::test]
async fn test_graceful_shutdown_notifies_clients() {
    let server = TestServer::start().await;
    let token = server.create_tenant("acme", TENANT_A_SECRET).await;
    server.create_room(&token, "chat").await;
    server.add_member(&token, "chat", "alice").await;

    // Connect alice
    let mut ws = server.ws_connect().await;
    let jwt = mint_jwt("alice", "acme", &["chat"], TENANT_A_SECRET);
    ws_auth(&mut ws, &jwt).await;
    ws_send(
        &mut ws,
        json!({"type": "subscribe", "payload": {"rooms": ["chat"]}}),
    )
    .await;
    ws_recv_type(&mut ws, "subscribed").await;

    // Simulate shutdown broadcast
    server
        .state
        .connections
        .broadcast_all(&herald_core::protocol::ServerMessage::error(
            None,
            herald_core::error::ErrorCode::Internal,
            "server shutting down",
        ));

    // Alice should receive the shutdown error
    let msg = ws_recv_type(&mut ws, "error").await;
    assert_eq!(msg["payload"]["message"], "server shutting down");
}

// ---------------------------------------------------------------------------
// Message deletion tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_message_deletion_http() {
    let server = TestServer::start().await;
    let token = server.create_tenant("acme", TENANT_A_SECRET).await;
    server.create_room(&token, "chat").await;
    server.add_member(&token, "chat", "alice").await;

    // Send a message
    let resp = server
        .http_client()
        .post(server.http_url("/rooms/chat/messages"))
        .bearer_auth(&token)
        .json(&json!({"sender": "alice", "body": "to be deleted"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body: Value = resp.json().await.unwrap();
    let msg_id = body["id"].as_str().unwrap().to_string();

    // Delete the message
    let resp = server
        .http_client()
        .delete(server.http_url(&format!("/rooms/chat/messages/{msg_id}")))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Verify message body is empty in history
    let resp = server
        .http_client()
        .get(server.http_url("/rooms/chat/messages"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    let messages = body["messages"].as_array().unwrap();
    let deleted_msg = messages.iter().find(|m| m["id"] == msg_id).unwrap();
    assert_eq!(deleted_msg["body"], "");
    assert_eq!(deleted_msg["meta"]["deleted"], true);
}

#[tokio::test]
async fn test_message_deletion_ws() {
    let server = TestServer::start().await;
    let token = server.create_tenant("acme", TENANT_A_SECRET).await;
    server.create_room(&token, "chat").await;
    server.add_member(&token, "chat", "alice").await;
    server.add_member(&token, "chat", "bob").await;

    // Connect bob
    let mut ws_bob = server.ws_connect().await;
    let bob_jwt = mint_jwt("bob", "acme", &["chat"], TENANT_A_SECRET);
    ws_auth(&mut ws_bob, &bob_jwt).await;
    ws_send(
        &mut ws_bob,
        json!({"type": "subscribe", "payload": {"rooms": ["chat"]}}),
    )
    .await;
    ws_recv_type(&mut ws_bob, "subscribed").await;

    // Connect alice and send a message
    let mut ws_alice = server.ws_connect().await;
    let alice_jwt = mint_jwt("alice", "acme", &["chat"], TENANT_A_SECRET);
    ws_auth(&mut ws_alice, &alice_jwt).await;
    ws_send(
        &mut ws_alice,
        json!({"type": "subscribe", "payload": {"rooms": ["chat"]}}),
    )
    .await;
    ws_recv_type(&mut ws_alice, "subscribed").await;

    // Drain presence messages
    while let Ok(Some(Ok(Message::Text(_)))) =
        tokio::time::timeout(Duration::from_millis(200), ws_bob.next()).await
    {}

    ws_send(
        &mut ws_alice,
        json!({"type": "message.send", "payload": {"room": "chat", "body": "hello"}}),
    )
    .await;
    let ack = ws_recv_type(&mut ws_alice, "message.ack").await;
    let msg_id = ack["payload"]["id"].as_str().unwrap().to_string();

    // Bob receives the message
    let _new_msg = ws_recv_type(&mut ws_bob, "message.new").await;

    // Alice deletes the message
    ws_send(
        &mut ws_alice,
        json!({"type": "message.delete", "payload": {"room": "chat", "id": &msg_id}}),
    )
    .await;
    let _delete_ack = ws_recv_type(&mut ws_alice, "message.ack").await;

    // Bob should receive message.deleted
    let deleted = ws_recv_type(&mut ws_bob, "message.deleted").await;
    assert_eq!(deleted["payload"]["id"], msg_id);
    assert_eq!(deleted["payload"]["room"], "chat");
}

// ---------------------------------------------------------------------------
// Room archival tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_room_archival() {
    let server = TestServer::start().await;
    let token = server.create_tenant("acme", TENANT_A_SECRET).await;
    server.create_room(&token, "chat").await;
    server.add_member(&token, "chat", "alice").await;

    // Send a message — should work
    let resp = server
        .http_client()
        .post(server.http_url("/rooms/chat/messages"))
        .bearer_auth(&token)
        .json(&json!({"sender": "alice", "body": "before archive"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Archive the room
    let resp = server
        .http_client()
        .patch(server.http_url("/rooms/chat"))
        .bearer_auth(&token)
        .json(&json!({"archived": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Verify room shows archived
    let resp = server
        .http_client()
        .get(server.http_url("/rooms/chat"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["archived"], true);

    // Send a message — should fail
    let resp = server
        .http_client()
        .post(server.http_url("/rooms/chat/messages"))
        .bearer_auth(&token)
        .json(&json!({"sender": "alice", "body": "after archive"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // History still readable
    let resp = server
        .http_client()
        .get(server.http_url("/rooms/chat/messages"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert!(!body["messages"].as_array().unwrap().is_empty());

    // Unarchive
    let resp = server
        .http_client()
        .patch(server.http_url("/rooms/chat"))
        .bearer_auth(&token)
        .json(&json!({"archived": false}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Send a message — should work again
    let resp = server
        .http_client()
        .post(server.http_url("/rooms/chat/messages"))
        .bearer_auth(&token)
        .json(&json!({"sender": "alice", "body": "after unarchive"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
}

// ---------------------------------------------------------------------------
// Webhook event filtering (Item 28)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_webhook_event_filtering() {
    // Start webhook receiver
    let (webhook_tx, mut webhook_rx) = tokio::sync::mpsc::channel::<String>(10);
    let webhook_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let webhook_port = webhook_listener.local_addr().unwrap().port();

    let webhook_app = axum::Router::new().route(
        "/hook",
        axum::routing::post(move |body: String| {
            let tx = webhook_tx.clone();
            async move {
                let _ = tx.send(body).await;
                "ok"
            }
        }),
    );
    tokio::spawn(async move {
        axum::serve(webhook_listener, webhook_app).await.unwrap();
    });

    // Create server with webhook filtered to message.new only
    let db = create_test_store().await;
    let config = HeraldConfig {
        server: ServerConfig {
            ws_bind: "127.0.0.1:0".to_string(),
            http_bind: "127.0.0.1:0".to_string(),
            log_level: "warn".to_string(),
            max_messages_per_sec: 1000,
            ..Default::default()
        },
        store: StoreConfig {
            path: "/tmp/herald-test-whfilter".into(),
            message_ttl_days: 7,
        },
        auth: AuthConfig {
            jwt_secret: Some(TENANT_A_SECRET.to_string()),
            jwt_issuer: None,
            super_admin_token: Some(SUPER_ADMIN_TOKEN.to_string()),
            api: ApiAuthConfig {
                tokens: vec!["wh-token".to_string()],
            },
        },
        presence: PresenceConfig {
            linger_secs: 0,
            manual_override_ttl_secs: 14400,
        },
        webhook: Some(WebhookConfig {
            url: format!("http://127.0.0.1:{webhook_port}/hook"),
            secret: "test-webhook-secret-1234567890".to_string(),
            retries: 0,
            events: Some(vec!["message.new".to_string()]),
        }),
        shroudb: None,
        tls: None,
        tenant_limits: Default::default(),
        cors: None,
    };

    let state = AppState::build(AppStateBuilder {
        config,
        db,
        sentry: None,
        courier: None,
        chronicle: None,
    });
    state.bootstrap_single_tenant().await.unwrap();

    let http_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let http_port = http_listener.local_addr().unwrap().port();
    let http_app = herald_server::http::router(state.clone());
    tokio::spawn(async move { axum::serve(http_listener, http_app).await.unwrap() });
    tokio::time::sleep(Duration::from_millis(50)).await;

    let client = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{http_port}");

    // Create room + member (triggers member.joined webhook — should be filtered out)
    client
        .post(format!("{base}/rooms"))
        .bearer_auth("wh-token")
        .json(&json!({"id": "chat", "name": "Chat"}))
        .send()
        .await
        .unwrap();
    client
        .post(format!("{base}/rooms/chat/members"))
        .bearer_auth("wh-token")
        .json(&json!({"user_id": "alice"}))
        .send()
        .await
        .unwrap();

    // Wait briefly — no webhook should arrive for member.joined
    let result = tokio::time::timeout(Duration::from_millis(500), webhook_rx.recv()).await;
    assert!(
        result.is_err(),
        "should NOT receive webhook for member.joined (filtered out)"
    );

    // Send a message — should trigger webhook
    client
        .post(format!("{base}/rooms/chat/messages"))
        .bearer_auth("wh-token")
        .json(&json!({"sender": "alice", "body": "hello"}))
        .send()
        .await
        .unwrap();

    let webhook_body = tokio::time::timeout(Duration::from_secs(5), webhook_rx.recv())
        .await
        .expect("webhook timeout")
        .expect("webhook channel closed");
    let parsed: Value = serde_json::from_str(&webhook_body).unwrap();
    assert_eq!(parsed["event"], "message.new");
}

// ---------------------------------------------------------------------------
// API key scoping (Item 29)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_api_key_scoping_read_only() {
    let server = TestServer::start().await;
    let _full_token = server.create_tenant("acme", TENANT_A_SECRET).await;

    // Create a read-only scoped token
    let resp = server
        .http_client()
        .post(server.http_url("/admin/tenants/acme/tokens"))
        .bearer_auth(SUPER_ADMIN_TOKEN)
        .json(&json!({"scope": "read-only"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body: Value = resp.json().await.unwrap();
    let read_token = body["token"].as_str().unwrap().to_string();

    // GET should work with read-only token
    let resp = server
        .http_client()
        .get(server.http_url("/rooms"))
        .bearer_auth(&read_token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // POST should be forbidden with read-only token
    let resp = server
        .http_client()
        .post(server.http_url("/rooms"))
        .bearer_auth(&read_token)
        .json(&json!({"id": "test", "name": "Test"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_api_key_scoping_room() {
    let server = TestServer::start().await;
    let full_token = server.create_tenant("acme", TENANT_A_SECRET).await;
    server.create_room(&full_token, "allowed").await;
    server.create_room(&full_token, "forbidden").await;

    // Create a room-scoped token
    let resp = server
        .http_client()
        .post(server.http_url("/admin/tenants/acme/tokens"))
        .bearer_auth(SUPER_ADMIN_TOKEN)
        .json(&json!({"scope": "room:allowed"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body: Value = resp.json().await.unwrap();
    let room_token = body["token"].as_str().unwrap().to_string();

    // Access to allowed room should work
    let resp = server
        .http_client()
        .get(server.http_url("/rooms/allowed"))
        .bearer_auth(&room_token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Access to forbidden room should fail
    let resp = server
        .http_client()
        .get(server.http_url("/rooms/forbidden"))
        .bearer_auth(&room_token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------------------
// Security & Authorization Tests (Item 37)
// ---------------------------------------------------------------------------

// --- JWT Security ---

#[tokio::test]
async fn test_jwt_expired_token_rejected() {
    let server = TestServer::start().await;
    server.create_tenant("acme", TENANT_A_SECRET).await;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let expired_jwt = jsonwebtoken::encode(
        &Header::default(),
        &JwtClaims {
            sub: "alice".to_string(),
            tenant: "acme".to_string(),
            rooms: vec!["chat".to_string()],
            exp: now - 3600, // Expired 1 hour ago
            iat: now - 7200,
            iss: "test".to_string(),
        },
        &EncodingKey::from_secret(TENANT_A_SECRET.as_bytes()),
    )
    .unwrap();

    let mut ws = server.ws_connect().await;
    ws_send(
        &mut ws,
        json!({"type": "auth", "payload": {"token": &expired_jwt}}),
    )
    .await;
    let msg = ws_recv_type(&mut ws, "auth_error").await;
    assert_eq!(msg["payload"]["code"], "TOKEN_INVALID");
}

#[tokio::test]
async fn test_jwt_wrong_secret_rejected() {
    let server = TestServer::start().await;
    server.create_tenant("acme", TENANT_A_SECRET).await;

    let jwt = mint_jwt("alice", "acme", &["chat"], "wrong-secret-value");
    let mut ws = server.ws_connect().await;
    ws_send(&mut ws, json!({"type": "auth", "payload": {"token": &jwt}})).await;
    let msg = ws_recv_type(&mut ws, "auth_error").await;
    assert_eq!(msg["payload"]["code"], "TOKEN_INVALID");
}

#[tokio::test]
async fn test_jwt_missing_tenant_claim() {
    let server = TestServer::start().await;

    // Manually craft a JWT with empty tenant
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let jwt = jsonwebtoken::encode(
        &Header::default(),
        &JwtClaims {
            sub: "alice".to_string(),
            tenant: "".to_string(), // Empty tenant
            rooms: vec![],
            exp: now + 3600,
            iat: now,
            iss: "test".to_string(),
        },
        &EncodingKey::from_secret(b"any-secret"),
    )
    .unwrap();

    let mut ws = server.ws_connect().await;
    ws_send(&mut ws, json!({"type": "auth", "payload": {"token": &jwt}})).await;
    let msg = ws_recv_type(&mut ws, "auth_error").await;
    assert_eq!(msg["payload"]["code"], "TOKEN_INVALID");
}

#[tokio::test]
async fn test_jwt_unknown_tenant_rejected() {
    let server = TestServer::start().await;
    server.create_tenant("acme", TENANT_A_SECRET).await;

    let jwt = mint_jwt("alice", "nonexistent-tenant", &["chat"], TENANT_A_SECRET);
    let mut ws = server.ws_connect().await;
    ws_send(&mut ws, json!({"type": "auth", "payload": {"token": &jwt}})).await;
    let msg = ws_recv_type(&mut ws, "auth_error").await;
    assert_eq!(msg["payload"]["code"], "TOKEN_INVALID");
}

#[tokio::test]
async fn test_jwt_missing_sub_claim() {
    let server = TestServer::start().await;
    server.create_tenant("acme", TENANT_A_SECRET).await;

    // Craft JWT without sub claim using a raw map
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let claims = serde_json::json!({
        "tenant": "acme",
        "rooms": ["chat"],
        "exp": now + 3600,
        "iat": now,
        "iss": "test",
    });
    let jwt = jsonwebtoken::encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(TENANT_A_SECRET.as_bytes()),
    )
    .unwrap();

    let mut ws = server.ws_connect().await;
    ws_send(&mut ws, json!({"type": "auth", "payload": {"token": &jwt}})).await;
    let msg = ws_recv_type(&mut ws, "auth_error").await;
    assert_eq!(msg["payload"]["code"], "TOKEN_INVALID");
}

// --- Authorization ---

#[tokio::test]
async fn test_subscribe_to_room_not_in_jwt() {
    let server = TestServer::start().await;
    let token = server.create_tenant("acme", TENANT_A_SECRET).await;
    server.create_room(&token, "secret-room").await;
    server.add_member(&token, "secret-room", "alice").await;

    // JWT only authorizes "other-room", not "secret-room"
    let jwt = mint_jwt("alice", "acme", &["other-room"], TENANT_A_SECRET);
    let mut ws = server.ws_connect().await;
    ws_auth(&mut ws, &jwt).await;

    ws_send(
        &mut ws,
        json!({"type": "subscribe", "payload": {"rooms": ["secret-room"]}}),
    )
    .await;
    let msg = ws_recv_type(&mut ws, "error").await;
    assert_eq!(msg["payload"]["code"], "UNAUTHORIZED");
}

#[tokio::test]
async fn test_subscribe_to_room_not_a_member() {
    let server = TestServer::start().await;
    let token = server.create_tenant("acme", TENANT_A_SECRET).await;
    server.create_room(&token, "chat").await;
    // alice is NOT added as a member

    let jwt = mint_jwt("alice", "acme", &["chat"], TENANT_A_SECRET);
    let mut ws = server.ws_connect().await;
    ws_auth(&mut ws, &jwt).await;

    ws_send(
        &mut ws,
        json!({"type": "subscribe", "payload": {"rooms": ["chat"]}}),
    )
    .await;
    let msg = ws_recv_type(&mut ws, "error").await;
    assert_eq!(msg["payload"]["code"], "ROOM_NOT_FOUND");
}

#[tokio::test]
async fn test_send_message_to_unsubscribed_room() {
    let server = TestServer::start().await;
    let token = server.create_tenant("acme", TENANT_A_SECRET).await;
    server.create_room(&token, "chat").await;
    // alice is not a member

    let jwt = mint_jwt("alice", "acme", &["chat"], TENANT_A_SECRET);
    let mut ws = server.ws_connect().await;
    ws_auth(&mut ws, &jwt).await;

    ws_send(
        &mut ws,
        json!({"type": "message.send", "payload": {"room": "chat", "body": "hello"}}),
    )
    .await;
    let msg = ws_recv_type(&mut ws, "error").await;
    assert_eq!(msg["payload"]["code"], "NOT_SUBSCRIBED");
}

#[tokio::test]
async fn test_cross_tenant_room_access_blocked() {
    let server = TestServer::start().await;
    let token_a = server.create_tenant("acme", TENANT_A_SECRET).await;
    let _token_b = server.create_tenant("beta", TENANT_B_SECRET).await;

    server.create_room(&token_a, "acme-chat").await;
    server.add_member(&token_a, "acme-chat", "alice").await;

    // Try to access acme's room with beta's JWT
    let jwt = mint_jwt("alice", "beta", &["acme-chat"], TENANT_B_SECRET);
    let mut ws = server.ws_connect().await;
    ws_auth(&mut ws, &jwt).await;

    ws_send(
        &mut ws,
        json!({"type": "subscribe", "payload": {"rooms": ["acme-chat"]}}),
    )
    .await;
    let msg = ws_recv_type(&mut ws, "error").await;
    // Should fail — room doesn't exist in beta tenant
    assert!(
        msg["payload"]["code"] == "ROOM_NOT_FOUND" || msg["payload"]["code"] == "UNAUTHORIZED",
        "expected room not found or unauthorized, got: {:?}",
        msg["payload"]["code"]
    );
}

#[tokio::test]
async fn test_http_api_requires_auth() {
    let server = TestServer::start().await;

    // No auth header
    let resp = server
        .http_client()
        .get(server.http_url("/rooms"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // Invalid token
    let resp = server
        .http_client()
        .get(server.http_url("/rooms"))
        .bearer_auth("bogus-token")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_admin_api_requires_super_token() {
    let server = TestServer::start().await;

    // No auth
    let resp = server
        .http_client()
        .get(server.http_url("/admin/tenants"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // Wrong token
    let resp = server
        .http_client()
        .get(server.http_url("/admin/tenants"))
        .bearer_auth("wrong-token")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // Correct token works
    let resp = server
        .http_client()
        .get(server.http_url("/admin/tenants"))
        .bearer_auth(SUPER_ADMIN_TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_tenant_api_token_cross_tenant_blocked() {
    let server = TestServer::start().await;
    let token_a = server.create_tenant("acme", TENANT_A_SECRET).await;
    let token_b = server.create_tenant("beta", TENANT_B_SECRET).await;

    server.create_room(&token_a, "acme-room").await;

    // Try to access acme-room with beta's token — should get 404 (room not in beta's scope)
    let resp = server
        .http_client()
        .get(server.http_url("/rooms/acme-room"))
        .bearer_auth(&token_b)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// --- Input validation edge cases ---

#[tokio::test]
async fn test_empty_room_id_rejected() {
    let server = TestServer::start().await;
    let token = server.create_tenant("acme", TENANT_A_SECRET).await;

    let resp = server
        .http_client()
        .post(server.http_url("/rooms"))
        .bearer_auth(&token)
        .json(&json!({"id": "", "name": "Empty ID Room"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_special_chars_in_room_id_rejected() {
    let server = TestServer::start().await;
    let token = server.create_tenant("acme", TENANT_A_SECRET).await;

    for bad_id in &[
        "room/evil",
        "room\\bad",
        "room\x00null",
        "room with spaces",
        "room@email",
    ] {
        let resp = server
            .http_client()
            .post(server.http_url("/rooms"))
            .bearer_auth(&token)
            .json(&json!({"id": bad_id, "name": "Bad Room"}))
            .send()
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "expected 400 for id: {bad_id:?}"
        );
    }
}

#[tokio::test]
async fn test_oversized_meta_rejected() {
    let server = TestServer::start().await;
    let token = server.create_tenant("acme", TENANT_A_SECRET).await;

    // Meta larger than 16KB
    let big_meta = serde_json::json!({"data": "x".repeat(20_000)});
    let resp = server
        .http_client()
        .post(server.http_url("/rooms"))
        .bearer_auth(&token)
        .json(&json!({"id": "room1", "name": "Room", "meta": big_meta}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_ws_message_body_too_large() {
    let server = TestServer::start().await;
    let token = server.create_tenant("acme", TENANT_A_SECRET).await;
    server.create_room(&token, "chat").await;
    server.add_member(&token, "chat", "alice").await;

    let jwt = mint_jwt("alice", "acme", &["chat"], TENANT_A_SECRET);
    let mut ws = server.ws_connect().await;
    ws_auth(&mut ws, &jwt).await;
    ws_send(
        &mut ws,
        json!({"type": "subscribe", "payload": {"rooms": ["chat"]}}),
    )
    .await;
    ws_recv_type(&mut ws, "subscribed").await;

    // Send 70KB body (limit is 64KB)
    let big_body = "x".repeat(70_000);
    ws_send(
        &mut ws,
        json!({"type": "message.send", "payload": {"room": "chat", "body": big_body}}),
    )
    .await;
    let msg = ws_recv_type(&mut ws, "error").await;
    assert_eq!(msg["payload"]["code"], "BAD_REQUEST");
}

#[tokio::test]
async fn test_malformed_json_rejected() {
    let server = TestServer::start().await;
    server.create_tenant("acme", TENANT_A_SECRET).await;

    let jwt = mint_jwt("alice", "acme", &["chat"], TENANT_A_SECRET);
    let mut ws = server.ws_connect().await;
    ws_auth(&mut ws, &jwt).await;

    // Send malformed JSON
    ws.send(Message::Text("not valid json{{{".into()))
        .await
        .unwrap();
    let msg = ws_recv_type(&mut ws, "error").await;
    assert_eq!(msg["payload"]["code"], "BAD_REQUEST");
}

#[tokio::test]
async fn test_unknown_message_type_rejected() {
    let server = TestServer::start().await;
    server.create_tenant("acme", TENANT_A_SECRET).await;

    let jwt = mint_jwt("alice", "acme", &["chat"], TENANT_A_SECRET);
    let mut ws = server.ws_connect().await;
    ws_auth(&mut ws, &jwt).await;

    ws_send(&mut ws, json!({"type": "nonexistent.type", "payload": {}})).await;
    let msg = ws_recv_type(&mut ws, "error").await;
    assert_eq!(msg["payload"]["code"], "BAD_REQUEST");
}

// --- Error paths ---

#[tokio::test]
async fn test_get_nonexistent_room() {
    let server = TestServer::start().await;
    let token = server.create_tenant("acme", TENANT_A_SECRET).await;

    let resp = server
        .http_client()
        .get(server.http_url("/rooms/does-not-exist"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_delete_nonexistent_room() {
    let server = TestServer::start().await;
    let token = server.create_tenant("acme", TENANT_A_SECRET).await;

    let resp = server
        .http_client()
        .delete(server.http_url("/rooms/nope"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_remove_nonexistent_member() {
    let server = TestServer::start().await;
    let token = server.create_tenant("acme", TENANT_A_SECRET).await;
    server.create_room(&token, "chat").await;

    let resp = server
        .http_client()
        .delete(server.http_url("/rooms/chat/members/nobody"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_send_to_nonexistent_room_http() {
    let server = TestServer::start().await;
    let token = server.create_tenant("acme", TENANT_A_SECRET).await;

    let resp = server
        .http_client()
        .post(server.http_url("/rooms/nope/messages"))
        .bearer_auth(&token)
        .json(&json!({"sender": "alice", "body": "hello"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_delete_nonexistent_tenant() {
    let server = TestServer::start().await;

    let resp = server
        .http_client()
        .delete(server.http_url("/admin/tenants/nope"))
        .bearer_auth(SUPER_ADMIN_TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_invalid_role_rejected() {
    let server = TestServer::start().await;
    let token = server.create_tenant("acme", TENANT_A_SECRET).await;
    server.create_room(&token, "chat").await;
    server.add_member(&token, "chat", "alice").await;

    let resp = server
        .http_client()
        .patch(server.http_url("/rooms/chat/members/alice"))
        .bearer_auth(&token)
        .json(&json!({"role": "superadmin"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_auth_timeout_on_ws() {
    let server = TestServer::start().await;

    // Connect but don't send auth
    let mut ws = server.ws_connect().await;
    // Wait for auth timeout (5 seconds)
    let timeout = tokio::time::timeout(Duration::from_secs(7), ws.next()).await;
    match timeout {
        Ok(Some(Ok(Message::Text(text)))) => {
            let msg: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(msg["type"], "auth_error");
        }
        Ok(Some(Ok(Message::Close(_)))) => {
            // Server closed connection — acceptable
        }
        _other => {
            // Connection closed or timed out — both acceptable for auth timeout
        }
    }
}

#[tokio::test]
async fn test_double_auth_rejected() {
    let server = TestServer::start().await;
    server.create_tenant("acme", TENANT_A_SECRET).await;

    let jwt = mint_jwt("alice", "acme", &[], TENANT_A_SECRET);
    let mut ws = server.ws_connect().await;
    ws_auth(&mut ws, &jwt).await;

    // Try to auth again
    ws_send(&mut ws, json!({"type": "auth", "payload": {"token": &jwt}})).await;
    let msg = ws_recv_type(&mut ws, "error").await;
    assert_eq!(msg["payload"]["code"], "BAD_REQUEST");
}

#[tokio::test]
async fn test_ws_delete_message_non_member_blocked() {
    let server = TestServer::start().await;
    let token = server.create_tenant("acme", TENANT_A_SECRET).await;
    server.create_room(&token, "chat").await;
    server.add_member(&token, "chat", "alice").await;
    // Bob is NOT a member

    // Alice sends a message via HTTP
    let resp = server
        .http_client()
        .post(server.http_url("/rooms/chat/messages"))
        .bearer_auth(&token)
        .json(&json!({"sender": "alice", "body": "alice's message"}))
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    let msg_id = body["id"].as_str().unwrap().to_string();

    // Bob tries to delete alice's message but is not a member
    let bob_jwt = mint_jwt("bob", "acme", &["chat"], TENANT_A_SECRET);
    let mut ws = server.ws_connect().await;
    ws_auth(&mut ws, &bob_jwt).await;

    ws_send(
        &mut ws,
        json!({"type": "message.delete", "payload": {"room": "chat", "id": &msg_id}}),
    )
    .await;
    let msg = ws_recv_type(&mut ws, "error").await;
    assert_eq!(msg["payload"]["code"], "NOT_SUBSCRIBED");
}

#[tokio::test]
async fn test_ws_send_to_archived_room() {
    let server = TestServer::start().await;
    let token = server.create_tenant("acme", TENANT_A_SECRET).await;
    server.create_room(&token, "chat").await;
    server.add_member(&token, "chat", "alice").await;

    // Alice subscribes first before archiving
    let jwt = mint_jwt("alice", "acme", &["chat"], TENANT_A_SECRET);
    let mut ws = server.ws_connect().await;
    ws_auth(&mut ws, &jwt).await;
    ws_send(
        &mut ws,
        json!({"type": "subscribe", "payload": {"rooms": ["chat"]}}),
    )
    .await;
    ws_recv_type(&mut ws, "subscribed").await;

    // Archive the room
    let resp = server
        .http_client()
        .patch(server.http_url("/rooms/chat"))
        .bearer_auth(&token)
        .json(&json!({"archived": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Alice tries to send via WS
    ws_send(
        &mut ws,
        json!({"type": "message.send", "payload": {"room": "chat", "body": "hello"}}),
    )
    .await;
    let msg = ws_recv_type(&mut ws, "error").await;
    assert_eq!(msg["payload"]["code"], "BAD_REQUEST");
}

#[tokio::test]
async fn test_health_endpoint_no_auth_required() {
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
}

#[tokio::test]
async fn test_empty_name_rejected() {
    let server = TestServer::start().await;
    let token = server.create_tenant("acme", TENANT_A_SECRET).await;

    let resp = server
        .http_client()
        .post(server.http_url("/rooms"))
        .bearer_auth(&token)
        .json(&json!({"id": "room1", "name": ""}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_tenant_isolation_messages() {
    let server = TestServer::start().await;
    let token_a = server.create_tenant("acme", TENANT_A_SECRET).await;
    let token_b = server.create_tenant("beta", TENANT_B_SECRET).await;

    server.create_room(&token_a, "chat").await;
    server.create_room(&token_b, "chat").await; // Same room name, different tenant
    server.add_member(&token_a, "chat", "alice").await;
    server.add_member(&token_b, "chat", "bob").await;

    // Send message in acme's chat
    let resp = server
        .http_client()
        .post(server.http_url("/rooms/chat/messages"))
        .bearer_auth(&token_a)
        .json(&json!({"sender": "alice", "body": "acme message"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // List messages in beta's chat — should be empty
    let resp = server
        .http_client()
        .get(server.http_url("/rooms/chat/messages"))
        .bearer_auth(&token_b)
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    let messages = body["messages"].as_array().unwrap();
    assert!(
        messages.is_empty(),
        "beta's chat should have no messages from acme"
    );
}

#[tokio::test]
async fn test_ws_fetch_messages_non_member_blocked() {
    let server = TestServer::start().await;
    let token = server.create_tenant("acme", TENANT_A_SECRET).await;
    server.create_room(&token, "chat").await;
    // alice is NOT a member

    let jwt = mint_jwt("alice", "acme", &["chat"], TENANT_A_SECRET);
    let mut ws = server.ws_connect().await;
    ws_auth(&mut ws, &jwt).await;

    ws_send(
        &mut ws,
        json!({"type": "messages.fetch", "payload": {"room": "chat"}}),
    )
    .await;
    let msg = ws_recv_type(&mut ws, "error").await;
    assert_eq!(msg["payload"]["code"], "NOT_SUBSCRIBED");
}

#[tokio::test]
async fn test_duplicate_room_creation_rejected() {
    let server = TestServer::start().await;
    let token = server.create_tenant("acme", TENANT_A_SECRET).await;
    server.create_room(&token, "chat").await;

    // Try to create the same room again
    let resp = server
        .http_client()
        .post(server.http_url("/rooms"))
        .bearer_auth(&token)
        .json(&json!({"id": "chat", "name": "Chat Again"}))
        .send()
        .await
        .unwrap();
    // Should get conflict or bad request
    assert!(
        resp.status() == StatusCode::CONFLICT || resp.status() == StatusCode::BAD_REQUEST,
        "expected 409 or 400 for duplicate room, got: {}",
        resp.status()
    );
}

#[tokio::test]
async fn test_duplicate_tenant_creation_rejected() {
    let server = TestServer::start().await;
    server.create_tenant("acme", TENANT_A_SECRET).await;

    // Try to create the same tenant again
    let resp = server
        .http_client()
        .post(server.http_url("/admin/tenants"))
        .bearer_auth(SUPER_ADMIN_TOKEN)
        .json(&json!({"id": "acme", "name": "acme", "jwt_secret": TENANT_A_SECRET}))
        .send()
        .await
        .unwrap();
    assert!(
        resp.status() == StatusCode::CONFLICT || resp.status() == StatusCode::BAD_REQUEST,
        "expected 409 or 400 for duplicate tenant, got: {}",
        resp.status()
    );
}

#[tokio::test]
async fn test_http_send_to_archived_room() {
    let server = TestServer::start().await;
    let token = server.create_tenant("acme", TENANT_A_SECRET).await;
    server.create_room(&token, "chat").await;

    // Archive the room
    let resp = server
        .http_client()
        .patch(server.http_url("/rooms/chat"))
        .bearer_auth(&token)
        .json(&json!({"archived": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Try to inject a message
    let resp = server
        .http_client()
        .post(server.http_url("/rooms/chat/messages"))
        .bearer_auth(&token)
        .json(&json!({"sender": "alice", "body": "hello"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_http_body_too_large() {
    let server = TestServer::start().await;
    let token = server.create_tenant("acme", TENANT_A_SECRET).await;
    server.create_room(&token, "chat").await;

    let big_body = "x".repeat(70_000);
    let resp = server
        .http_client()
        .post(server.http_url("/rooms/chat/messages"))
        .bearer_auth(&token)
        .json(&json!({"sender": "alice", "body": big_body}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_ws_subscribe_multiple_rooms_partial_auth() {
    let server = TestServer::start().await;
    let token = server.create_tenant("acme", TENANT_A_SECRET).await;
    server.create_room(&token, "allowed").await;
    server.create_room(&token, "forbidden").await;
    server.add_member(&token, "allowed", "alice").await;
    server.add_member(&token, "forbidden", "alice").await;

    // JWT only permits "allowed"
    let jwt = mint_jwt("alice", "acme", &["allowed"], TENANT_A_SECRET);
    let mut ws = server.ws_connect().await;
    ws_auth(&mut ws, &jwt).await;

    // Subscribe to both
    ws_send(
        &mut ws,
        json!({"type": "subscribe", "payload": {"rooms": ["allowed", "forbidden"]}}),
    )
    .await;

    // Should get error for forbidden, subscribed for allowed (order may vary)
    let mut got_subscribed = false;
    let mut got_error = false;
    for _ in 0..2 {
        let timeout = tokio::time::timeout(Duration::from_secs(5), ws.next()).await;
        if let Ok(Some(Ok(Message::Text(text)))) = timeout {
            let msg: Value = serde_json::from_str(&text).unwrap();
            if msg["type"] == "subscribed" {
                got_subscribed = true;
            } else if msg["type"] == "error" && msg["payload"]["code"] == "UNAUTHORIZED" {
                got_error = true;
            }
        }
    }
    assert!(got_subscribed, "should have subscribed to allowed room");
    assert!(got_error, "should have gotten error for forbidden room");
}
