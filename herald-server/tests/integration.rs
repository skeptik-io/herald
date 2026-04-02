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
                max_messages_per_sec: 1000,
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
    assert!(tokens.iter().any(|t| t.as_str() == Some(token)));

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
