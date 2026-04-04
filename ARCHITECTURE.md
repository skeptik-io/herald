# Herald — Architecture & Protocol Specification

Herald is a product-agnostic WebSocket realtime server. It handles streams, events, presence, cursors, and real-time fan-out. It does not know about any consuming application's domain model.

Herald is a standalone Rust project. It optionally integrates with [ShroudB](https://github.com/nicklucas/shroudb) for authorization, offline notifications, and audit — but runs independently without it. Event bodies are opaque — Herald stores and delivers them as-is. Consumers handle their own encryption and search.

---

## Table of Contents

1. [System Architecture](#1-system-architecture)
2. [Generic Primitives](#2-generic-primitives)
3. [WebSocket Protocol](#3-websocket-protocol)
4. [HTTP API](#4-http-api)
5. [Connection Management](#5-connection-management)
6. [Presence Model](#6-presence-model)
7. [Fan-Out Model](#7-fan-out-model)
8. [Storage Model](#8-storage-model)
9. [Stream Lifecycle](#9-stream-lifecycle)
10. [Crate Structure](#10-crate-structure)
11. [Configuration](#11-configuration)
12. [Implementation Phasing](#12-implementation-phasing)

---

## 1. System Architecture

### Deployment Topology

```
  Browser tabs (N)              App Backend
       │                            │
       │ WebSocket :6200            │ HTTP :6201
       ▼                            ▼
  ┌──────────────────────────────────────┐
  │               HERALD                  │
  │                                       │
  │   Connection Registry                 │
  │   Stream Registry                       │
  │   Presence Tracker                    │
  │   Event Pipeline                    │
  │   ShroudB WAL storage              │
  └──────────┬────────────────────────────┘
             │
       (optional, TCP)
             │
  ┌──────────▼──────────┐
  │    ShroudB Moat     │
  │     :8200 / :8201   │
  │                     │
  │  Sentry  │  Courier │
  │  Chronicle          │
  └─────────────────────┘
```

Herald and ShroudB are separate services. Herald connects to Moat (or individual ShroudB engines) over TCP using published Rust client crates as regular Cargo dependencies. ShroudB Sentry integration is behind a `shroudb` feature flag — when the feature is disabled or the `[shroudb]` config section is absent, Herald relies on JWT `streams` claim for authorization only.

### What Herald Is

- A WebSocket server for real-time bidirectional messaging
- An HTTP API for backend stream/member management
- A event fan-out engine with per-stream sequence ordering
- A presence tracker derived from connection state
- A catch-up buffer for seamless reconnection

### What Herald Is Not

- Not a ShroudB engine (no RESP3, no Moat embedding, no engine conventions)
- Storage is ShroudB WAL engine — ~80µs writes
- Not an application server (no business logic, no domain awareness)

---

## 2. Generic Primitives

| Concept | Description |
|---------|-------------|
| **Stream** | A named channel with a member list and an opaque `meta` JSON blob. The app defines what streams mean (DMs, group realtimes, support threads) via `meta`. |
| **Event** | An opaque `body` + opaque `meta` JSON + sender ID + monotonic sequence number. Herald stores and delivers events but never interprets `body` or `meta`. |
| **Member** | A user ID + role (`owner`, `admin`, `member`) in a stream. Roles are informational — Herald does not enforce role-based permissions beyond stream membership. |
| **Presence** | A user's online status, derived from WebSocket connections with manual override support. One of: `online`, `away`, `dnd`, `offline`. |
| **Cursor** | A per-user, per-stream read position expressed as a sequence number. The gap between a user's cursor and the stream's latest sequence is the unread count. |

### The `meta` Field

Both streams and events carry an opaque `meta: JsonValue` field. Herald stores it, indexes nothing in it, and passes it through to clients and webhooks unchanged. Apps use `meta` to extend Herald's primitives with domain-specific data:

```json
// Fanvalt: PPV media attachment
{ "type": "media", "vault_item_id": "vi_123", "price_cents": 500 }

// Slack-like: code snippet
{ "type": "code", "language": "rust", "filename": "main.rs" }

// Support tool: ticket reference
{ "type": "ticket", "ticket_id": "SUPP-1234", "priority": "high" }
```

---

## 3. WebSocket Protocol

Port `6200`. JSON text frames only — binary frames are rejected and the connection is closed.

### Frame Envelope

Every event follows this structure:

```json
{
  "type": "string",
  "ref": "string | null",
  "payload": {}
}
```

- `type` — Event type identifier
- `ref` — Client-generated correlation ID. Server echoes it in responses. Optional for server-initiated events.
- `payload` — Type-specific data

### Client → Server Events

#### `auth`

Must be the first event after connection. Connection is closed if not received within 5 seconds.

```json
{
  "type": "auth",
  "ref": "abc123",
  "payload": {
    "token": "eyJhbGciOi...",
    "last_seen_at": 1712000000000
  }
}
```

- `token` — JWT minted by the app backend (see [JWT Claims](#jwt-claims))
- `last_seen_at` — Optional. Millisecond timestamp of last received event. If provided, Herald replays missed events for all streams in the JWT `streams` claim.

#### `auth.refresh`

Extend the session without reconnecting. Sent in response to `system.token_expiring`.

```json
{
  "type": "auth.refresh",
  "ref": "bcd890",
  "payload": {
    "token": "eyJhbGciOi..."
  }
}
```

#### `subscribe`

Subscribe to streams. Only streams listed in the JWT `streams` claim are permitted.

```json
{
  "type": "subscribe",
  "ref": "def456",
  "payload": {
    "streams": ["stream_a", "stream_b"]
  }
}
```

Server responds with one `subscribed` event per stream.

#### `unsubscribe`

```json
{
  "type": "unsubscribe",
  "ref": "ghi789",
  "payload": {
    "streams": ["stream_a"]
  }
}
```

#### `event.publish`

```json
{
  "type": "event.publish",
  "ref": "jkl012",
  "payload": {
    "stream": "stream_a",
    "body": "Hello world",
    "meta": { "reply_to": "msg_xyz", "attachments": [] }
  }
}
```

- `body` — Event content (opaque, passed through as-is)
- `meta` — Opaque JSON, passed through unmodified

#### `cursor.update`

Fire-and-forget. No ack is sent.

```json
{
  "type": "cursor.update",
  "payload": {
    "stream": "stream_a",
    "seq": 42
  }
}
```

#### `presence.set`

```json
{
  "type": "presence.set",
  "ref": "mno345",
  "payload": {
    "status": "dnd"
  }
}
```

`status` — One of `online`, `away`, `dnd`. The `offline` status is never set manually — it is derived from connection state.

#### `typing.start` / `typing.stop`

Ephemeral. Not persisted. Auto-expires after 5 seconds without renewal.

```json
{
  "type": "typing.start",
  "payload": { "stream": "stream_a" }
}
```

#### `events.fetch`

Fetch historical events for scroll-back or catch-up.

```json
{
  "type": "events.fetch",
  "ref": "pqr678",
  "payload": {
    "stream": "stream_a",
    "before": 42,
    "limit": 50
  }
}
```

- `before` — Sequence number. Returns events with `seq < before`.
- `limit` — Max events (server-capped at 100).

#### `ping`

Application-level keepalive for clients that cannot access WebSocket-level ping/pong.

```json
{ "type": "ping", "ref": "vwx234" }
```

### Server → Client Events

#### `auth_ok`

```json
{
  "type": "auth_ok",
  "ref": "abc123",
  "payload": {
    "user_id": "user_1",
    "server_time": 1712000000000,
    "heartbeat_interval": 30000
  }
}
```

#### `auth_error`

Connection is closed after sending.

```json
{
  "type": "auth_error",
  "ref": "abc123",
  "payload": {
    "code": "TOKEN_EXPIRED",
    "event": "JWT has expired"
  }
}
```

#### `subscribed`

Sent once per stream in response to `subscribe`.

```json
{
  "type": "subscribed",
  "ref": "def456",
  "payload": {
    "stream": "stream_a",
    "members": [
      { "user_id": "user_1", "role": "owner", "presence": "online" },
      { "user_id": "user_2", "role": "member", "presence": "away" }
    ],
    "cursor": 38,
    "latest_seq": 42
  }
}
```

- `cursor` — This user's last read position
- `latest_seq` — Stream's current sequence number
- The gap `latest_seq - cursor` is the unread count

#### `event.new`

Broadcast to all stream subscribers when a event is sent. The sender also receives this (for multi-tab consistency).

```json
{
  "type": "event.new",
  "payload": {
    "stream": "stream_a",
    "id": "msg_abc",
    "seq": 43,
    "sender": "user_1",
    "body": "Hello world",
    "meta": { "reply_to": "msg_xyz" },
    "sent_at": 1712000001000
  }
}
```

#### `event.ack`

Sent to the originating connection only.

```json
{
  "type": "event.ack",
  "ref": "jkl012",
  "payload": {
    "id": "msg_abc",
    "seq": 43,
    "sent_at": 1712000001000
  }
}
```

#### `events.batch`

Response to `events.fetch` and reconnect catch-up.

```json
{
  "type": "events.batch",
  "ref": "pqr678",
  "payload": {
    "stream": "stream_a",
    "events": [
      { "id": "msg_aaa", "seq": 40, "sender": "user_2", "body": "...", "meta": {}, "sent_at": 1712000000100 },
      { "id": "msg_bbb", "seq": 41, "sender": "user_1", "body": "...", "meta": {}, "sent_at": 1712000000500 }
    ],
    "has_more": true
  }
}
```

#### `presence.changed`

Broadcast to all streams the affected user belongs to.

```json
{
  "type": "presence.changed",
  "payload": {
    "user_id": "user_2",
    "presence": "offline"
  }
}
```

#### `cursor.moved`

```json
{
  "type": "cursor.moved",
  "payload": {
    "stream": "stream_a",
    "user_id": "user_2",
    "seq": 41
  }
}
```

#### `member.joined` / `member.left` / `member.updated`

```json
{
  "type": "member.joined",
  "payload": {
    "stream": "stream_a",
    "user_id": "user_3",
    "role": "member"
  }
}
```

#### `stream.updated` / `stream.deleted`

```json
{
  "type": "stream.deleted",
  "payload": { "stream": "stream_a" }
}
```

#### `system.token_expiring`

Sent 60 seconds before the JWT expires.

```json
{
  "type": "system.token_expiring",
  "payload": { "expires_at": 1712003600000 }
}
```

#### `typing`

Broadcast to stream subscribers. Ephemeral.

```json
{
  "type": "typing",
  "payload": {
    "stream": "stream_a",
    "user_id": "user_2",
    "active": true
  }
}
```

#### `error`

```json
{
  "type": "error",
  "ref": "original_ref",
  "payload": {
    "code": "NOT_SUBSCRIBED",
    "event": "Not subscribed to stream_b"
  }
}
```

#### `pong`

```json
{ "type": "pong", "ref": "vwx234" }
```

### Error Codes

| Code | Meaning |
|------|---------|
| `TOKEN_EXPIRED` | JWT `exp` has passed |
| `TOKEN_INVALID` | JWT signature verification failed |
| `UNAUTHORIZED` | Stream not in JWT `streams` claim, or Sentry denied |
| `NOT_SUBSCRIBED` | Action requires subscription to the stream |
| `ROOM_NOT_FOUND` | Stream does not exist |
| `RATE_LIMITED` | Too many requests |
| `BAD_REQUEST` | Malformed frame or invalid payload |
| `INTERNAL` | Server error |

### JWT Claims

Minted by the app backend. Validated by Herald.

```json
{
  "sub": "user_id",
  "streams": ["stream_a", "stream_b"],
  "exp": 1712003600,
  "iat": 1712000000,
  "iss": "app-backend"
}
```

| Claim | Type | Required | Description |
|-------|------|----------|-------------|
| `sub` | string | yes | User ID |
| `streams` | string[] | yes | Streams the user may subscribe to |
| `exp` | number | yes | Expiration timestamp (Unix seconds) |
| `iat` | number | yes | Issued-at timestamp |
| `iss` | string | yes | Issuer (must match `auth.jwt_issuer` in config) |

Herald validates the signature using the configured HMAC secret or RSA/EC public key. The `streams` claim is the authorization boundary — `subscribe` requests for unlisted streams are rejected with `UNAUTHORIZED`.

---

## 4. HTTP API

Port `6201`. Backend-to-Herald communication. Authenticated via `Authorization: Bearer <token>` where `<token>` is one of the values in `auth.api.tokens` in Herald's config.

All request and response bodies are JSON. All timestamps are Unix milliseconds.

### Stream Management

#### `POST /streams`

Create a stream.

```json
{
  "id": "stream_abc",
  "name": "General Chat",
  "meta": { "type": "dm", "app_id": "fanvalt" }
}
```

- `id` — Client-generated stream ID. Must be unique.
- `meta` — Opaque JSON, stored and returned unchanged

Response: `201 Created`

```json
{
  "id": "stream_abc",
  "name": "General Chat",
  "meta": { "type": "dm", "app_id": "fanvalt" },
  "created_at": 1712000000000
}
```

#### `GET /streams/:id`

Response: `200 OK` — stream object with member count and latest sequence number.

#### `PATCH /streams/:id`

Update stream `name` or `meta`. Broadcasts `stream.updated` to subscribers.

#### `DELETE /streams/:id`

Deletes the stream and all events from the catch-up buffer. Broadcasts `stream.deleted` to all subscribers and terminates their subscriptions.

Response: `204 No Content`

### Member Management

#### `POST /streams/:id/members`

```json
{
  "user_id": "user_3",
  "role": "member"
}
```

Response: `201 Created`. Broadcasts `member.joined` to stream subscribers.

#### `DELETE /streams/:id/members/:user_id`

Response: `204 No Content`. Broadcasts `member.left`. If the user has active subscriptions to this stream, they are terminated.

#### `PATCH /streams/:id/members/:user_id`

```json
{ "role": "admin" }
```

Broadcasts `member.updated`.

#### `GET /streams/:id/members`

Response: Member list with current presence.

```json
{
  "members": [
    { "user_id": "user_1", "role": "owner", "presence": "online", "joined_at": 1712000000000 },
    { "user_id": "user_2", "role": "member", "presence": "away", "joined_at": 1712000001000 }
  ]
}
```

### Events

#### `POST /streams/:id/events`

Inject a event from the backend (system events, bot events, migration imports).

```json
{
  "sender": "system",
  "body": "Welcome to the stream!",
  "meta": { "system": true }
}
```

Triggers the full ingest pipeline (store, fan-out, webhook).

#### `GET /streams/:id/events`

Query history from the catch-up buffer.

| Param | Type | Description |
|-------|------|-------------|
| `before` | number | Return events with `seq < before` |
| `after` | number | Return events with `seq > after` |
| `limit` | number | Max results (default 50, max 100) |

Response:

```json
{
  "events": [
    { "id": "msg_abc", "seq": 43, "sender": "user_1", "body": "...", "meta": {}, "sent_at": 1712000001000 }
  ],
  "has_more": true
}
```

### Presence & Cursors

#### `GET /presence/:user_id`

```json
{
  "user_id": "user_1",
  "status": "online",
  "connections": 2,
  "last_seen_at": 1712000001000
}
```

#### `GET /streams/:id/presence`

All stream members' presence.

#### `GET /streams/:id/cursors`

```json
{
  "cursors": [
    { "user_id": "user_1", "seq": 42 },
    { "user_id": "user_2", "seq": 38 }
  ]
}
```

### Operational

#### `GET /health`

```json
{
  "status": "ok",
  "connections": 142,
  "streams": 87,
  "uptime_secs": 3600,
  "sentry": true
}
```

#### `GET /metrics`

Prometheus-format metrics (connections, events/sec, fan-out latency, etc.).

---

## 5. Connection Management

### Multi-Tab

Each browser tab opens its own WebSocket connection. Herald tracks connections per user:

```
user_1 → [conn_A (tab 1), conn_B (tab 2), conn_C (mobile)]
```

All connections for the same user receive all events for their subscribed streams. There is no leader election, no cross-tab coordination via localStorage or BroadcastChannel, and no stale state. Every tab gets every event independently.

### Connection State Machine

```
CONNECTING → AUTHENTICATING → ACTIVE → CLOSING → CLOSED
                  │                │
                  ▼                ▼
             AUTH_FAILED       EXPIRED
```

| State | Description |
|-------|-------------|
| `CONNECTING` | WebSocket handshake in progress |
| `AUTHENTICATING` | Connected, waiting for `auth` event (5s timeout) |
| `ACTIVE` | Authenticated, can subscribe/send/receive |
| `EXPIRED` | JWT expired, waiting for `auth.refresh` (10s grace) |
| `CLOSING` | Graceful shutdown, close frame sent |
| `CLOSED` | Connection terminated |

### Reconnect Protocol

1. Client detects disconnection (WebSocket `onclose` / `onerror`)
2. Client reconnects with exponential backoff: 1s, 2s, 4s, 8s, 16s, max 30s (with random jitter)
3. Client sends `auth` with `last_seen_at` set to the `sent_at` timestamp of the last received `event.new`
4. Herald responds with:
   - `auth_ok`
   - One `subscribed` per previously subscribed stream (with current member state)
   - One `events.batch` per stream with all events where `sent_at > last_seen_at`
5. Client applies catch-up events, deduplicating by event `id`

The `last_seen_at` approach is deliberately coarse. It may deliver some duplicate events. The client deduplicates by `id`. This is simpler and more reliable than tracking per-stream sequence numbers across reconnections.

### Token Refresh

1. Herald sends `system.token_expiring` 60 seconds before JWT `exp`
2. Client calls its app backend to mint a new JWT
3. Client sends `auth.refresh` with the new token
4. Herald validates, updates auth context, responds with `auth_ok`
5. If no refresh arrives: connection enters `EXPIRED` state for 10 seconds, then closes

### Heartbeat

- **WebSocket-level**: Herald sends ping frames every 30 seconds. If no pong is received within 10 seconds, the connection is closed.
- **Application-level**: `ping` / `pong` events for clients that cannot access WebSocket-level ping/pong (some browser APIs).

---

## 6. Presence Model

Presence is derived from WebSocket connection state with manual override support. No external dependencies (no Redis, no TTLs, no heartbeat polling).

### State Resolution

```
presence(user) = manual_override ?? (connections.len() > 0 ? online : offline)
```

1. If the user has set a manual override (`dnd` or `away` via `presence.set`): use that value
2. If the user has one or more active WebSocket connections: `online`
3. If the user has zero connections: `offline`

### Manual Override

- Set via `presence.set` WebSocket frame
- Persists across connection drops (stored in-memory by user ID)
- Expires after configurable TTL (default: 4 hours) and reverts to connection-derived
- `offline` is never set manually — it's always derived from connection state

### Linger Period

When a user's last connection drops, Herald waits `presence.linger_secs` (default: 10 seconds) before broadcasting `offline`. If a new connection arrives during this period, the offline broadcast is cancelled.

This prevents "offline → online → offline" flicker during page refreshes, tab switches, or brief network interruptions.

### Broadcast Scope

Presence changes are broadcast only to streams the affected user is a member of. If user A changes to `dnd`, only users subscribed to streams that user A belongs to receive `presence.changed`.

---

## 7. Fan-Out Model

Herald uses an in-memory pub/sub model. No external event broker.

### Data Structures

```
StreamRegistry: DashMap<StreamId, StreamState>

StreamState {
    members: HashSet<UserId>,                    // all members (online + offline)
    subscribers: DashMap<UserId, Vec<ConnId>>,    // active subscriptions
    sequence: AtomicU64,                         // monotonic event counter
}

ConnectionRegistry: DashMap<ConnId, ConnectionHandle>

ConnectionHandle {
    user_id: UserId,
    tx: mpsc::Sender<OutgoingFrame>,    // channel to the WebSocket write task
    subscriptions: HashSet<StreamId>,
}
```

### Event Fan-Out Path

1. Event arrives (from WebSocket `event.publish` or HTTP `POST /streams/:id/events`)
2. Sequence number assigned: `stream.sequence.fetch_add(1, SeqCst)`
3. Event stored in ShroudB WAL storage
4. For each user in `stream.subscribers`:
   - Look up all `ConnId`s for that user
   - Send the event JSON frame via each connection's `mpsc::Sender`
   - If the channel is full (backpressure), try for 100ms, then drop for that connection
5. Webhook POST to app backend
6. For offline members: Courier notification (if configured)

### Sender Receives Own Event

The sender's connections also receive `event.new`. This provides multi-tab consistency — if a user sends from tab 1, tab 2 sees the event via the normal fan-out path rather than requiring client-side coordination.

The sender's originating connection also receives `event.ack` (with the `ref` correlation ID) for optimistic UI confirmation.

### Backpressure

Each connection has a bounded `mpsc` channel (capacity: 256 frames). If the channel is full:

1. Attempt to send for 100ms
2. If still full, drop the event for that connection
3. Increment a `events_dropped` counter (visible in `/metrics`)
4. The client recovers via `events.fetch` on next interaction

### Ordering Guarantees

- **Within a stream**: Events are totally ordered by sequence number. Sequence numbers are assigned atomically before fan-out. Each connection's `mpsc` channel preserves insertion order.
- **Across streams**: No ordering guarantee. Streams are independent.

---

## 8. Storage Model

### Herald: WAL Catch-Up Buffer

Herald uses ShroudB WAL engine as the primary persistence layer for reconnect catch-up and short-term history queries. 

```sql
CREATE TABLE streams (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    meta            TEXT,            -- JSON blob
    created_at      INTEGER NOT NULL
);

CREATE TABLE members (
    stream_id   TEXT NOT NULL,
    user_id   TEXT NOT NULL,
    role      TEXT NOT NULL DEFAULT 'member',
    joined_at INTEGER NOT NULL,
    PRIMARY KEY (stream_id, user_id)
);

CREATE TABLE events (
    id         TEXT PRIMARY KEY,
    stream_id    TEXT NOT NULL,
    seq        INTEGER NOT NULL,
    sender     TEXT NOT NULL,
    body       BLOB NOT NULL,        -- opaque bytes, passed through as-is
    meta       TEXT,                  -- JSON blob (always plaintext)
    sent_at    INTEGER NOT NULL,
    expires_at INTEGER NOT NULL       -- TTL for automatic cleanup
);

CREATE INDEX idx_events_stream_seq ON events(stream_id, seq);
CREATE INDEX idx_events_expires ON events(expires_at);

CREATE TABLE cursors (
    stream_id    TEXT NOT NULL,
    user_id    TEXT NOT NULL,
    seq        INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (stream_id, user_id)
);
```

### Retention

Events have a configurable TTL (default: 7 days). A background task runs hourly:

```sql
DELETE FROM events WHERE expires_at < ?;
```

### App Sync: Webhook

Herald POSTs event events to the app backend so it can persist them in its own database.

```
POST https://app.example.com/webhooks/herald
X-Herald-Signature: sha256=a1b2c3d4e5f6...
X-Herald-Timestamp: 1712000001
Content-Type: application/json

{
  "event": "event.new",
  "stream": "stream_a",
  "id": "msg_abc",
  "seq": 43,
  "sender": "user_1",
  "body": "Hello world",
  "meta": { "reply_to": "msg_xyz" },
  "sent_at": 1712000001000
}
```

#### Webhook Signature Verification

`X-Herald-Signature` contains `sha256=HMAC-SHA256(secret, timestamp + "." + body)` where `secret` is the `webhook.secret` value from Herald's config. The app backend verifies by:

1. Extracting the timestamp from `X-Herald-Timestamp`
2. Rejecting timestamps older than 5 minutes (replay protection)
3. Computing `HMAC-SHA256(shared_secret, timestamp + "." + raw_body)`
4. Comparing against the signature in `X-Herald-Signature` using constant-time comparison

#### Webhook Events

| Event | Trigger |
|-------|---------|
| `event.new` | New event sent |
| `member.joined` | Member added to stream |
| `member.left` | Member removed from stream |
| `stream.deleted` | Stream deleted |

#### Delivery

Fire-and-forget with retries: 3 attempts with exponential backoff (1s, 2s, 4s). If all retries fail, the event is logged. The app backend can catch up via `GET /streams/:id/events?after=last_known_seq`.

#### What the App Persists

The webhook delivers event bodies as-is (opaque). The app persists events in its own database for:

- Long-term storage (beyond Herald's 7-day buffer)
- Domain-specific queries (PPV resolution, attachment access control, etc.)
- Full-text search (Postgres FTS, Elasticsearch, etc.)
- Business analytics

---

## 9. Stream Lifecycle

### Creation

1. App backend calls `POST /streams` on Herald's HTTP API
2. Herald inserts stream record into WAL
3. Return `201 Created`

### Member Management

1. App backend calls `POST /streams/:id/members`
2. Herald inserts member record
3. Herald adds user to in-memory `StreamState.members`
4. Herald broadcasts `member.joined` to stream subscribers
5. If the user has active WebSocket connections, they can now `subscribe` to this stream (requires a JWT with the stream in the `streams` claim)

### Deletion

1. App backend calls `DELETE /streams/:id`
2. Herald broadcasts `stream.deleted` to all subscribers
3. Herald terminates all subscriptions for this stream
4. Herald deletes all events from catch-up buffer
5. Herald removes stream from WAL and in-memory state

---

## 10. Crate Structure

```
herald/
├── Cargo.toml                      # workspace root
├── ARCHITECTURE.md                 # this document
├── herald.toml.example             # example config
├── Dockerfile
├── deny.toml
├── rust-toolchain.toml
│
├── herald-core/                    # Domain types (no I/O, no async)
│   └── src/
│       ├── lib.rs
│       ├── stream.rs                 # Stream, StreamId, StreamConfig
│       ├── event.rs              # Event, EventId, Sequence
│       ├── member.rs               # Member, Role
│       ├── presence.rs             # PresenceStatus, ManualOverride
│       ├── cursor.rs               # Cursor, CursorUpdate
│       ├── protocol.rs             # ClientFrame, ServerFrame, FrameType enums
│       ├── error.rs                # HeraldError
│       └── auth.rs                 # JwtClaims
│
├── herald-server/                  # The binary
│   └── src/
│       ├── main.rs                 # startup, signal handling, shutdown
│       ├── config.rs               # HeraldConfig (herald.toml)
│       ├── state.rs                # AppState (shared across handlers)
│       │
│       ├── ws/                     # WebSocket server (:6200)
│       │   ├── mod.rs
│       │   ├── upgrade.rs          # HTTP → WebSocket upgrade handler
│       │   ├── connection.rs       # Per-connection state machine
│       │   ├── handler.rs          # Frame dispatch (auth, subscribe, send, etc.)
│       │   └── fanout.rs           # Stream-level event fan-out
│       │
│       ├── http/                   # HTTP API (:6201)
│       │   ├── mod.rs              # Axum router
│       │   ├── streams.rs            # CRUD
│       │   ├── members.rs          # Member management
│       │   ├── events.rs         # Query, inject
│       │   ├── presence.rs         # Presence queries
│       │   ├── cursors.rs          # Cursor queries
│       │   └── health.rs           # Health + metrics
│       │
│       ├── integrations/           # Optional ShroudB integration
│       │   ├── mod.rs              # Capabilities struct (all Option<T>)
│       │   ├── sentry.rs           # Policy eval via shroudb-sentry-client
│       │   ├── courier.rs          # Offline delivery via shroudb-courier-client
│       │   └── chronicle.rs        # Audit via shroudb-chronicle-client
│       │
│       ├── store/                  # ShroudB WAL storage
│       │   ├── mod.rs
│       │   ├── events.rs
│       │   ├── streams.rs
│       │   ├── members.rs
│       │   └── cursors.rs
│       │
│       ├── registry/               # In-memory state
│       │   ├── mod.rs
│       │   ├── connection.rs       # ConnectionRegistry
│       │   ├── stream.rs             # StreamRegistry
│       │   └── presence.rs         # PresenceTracker
│       │
│       ├── pipeline/               # Event processing
│       │   └── ingest.rs           # validate → store → fanout → webhook
│       │
│       └── webhook/                # Outbound webhooks
│           └── mod.rs              # Signed delivery with retries
│
├── herald-client/                  # Rust client library (for testing / backend integration)
│   └── src/
│       ├── lib.rs
│       ├── ws.rs                   # WebSocket client
│       └── http.rs                 # HTTP API client
│
└── herald-sdk-typescript/          # Browser SDK (npm package)
    ├── package.json
    └── src/
        ├── index.ts
        ├── client.ts               # HeraldClient class
        ├── connection.ts           # WebSocket lifecycle, reconnect, backoff
        ├── types.ts                # Frame type definitions
        └── store.ts                # Client-side event store (dedup, ordering)
```

### Key Dependencies

```toml
[workspace.dependencies]
tokio = { version = "1", features = ["full"] }
axum = { version = "0.8", features = ["ws"] }
rusqlite = { version = "0.32", features = ["bundled"] }
dashmap = "6"
jsonwebtoken = "9"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
zeroize = "1"
tracing = "0.1"
tracing-subscriber = "0.3"
clap = { version = "4", features = ["derive"] }

# Optional — ShroudB client crates from the shroudb crate registry
shroudb-sentry-client = { version = "1", optional = true }
shroudb-courier-client = { version = "1", optional = true }
shroudb-chronicle-client = { version = "1", optional = true }

[features]
default = []
shroudb = [
    "shroudb-sentry-client",
    "shroudb-courier-client",
    "shroudb-chronicle-client",
]
```

### Degraded Mode

All ShroudB integration is optional. Herald operates in degraded mode when engines are unavailable:

| Feature | Without ShroudB | With ShroudB |
|---------|----------------|--------------|
| Event storage | Opaque body in WAL | Opaque body in WAL |
| Stream authorization | JWT `streams` claim only | JWT + Sentry policy eval |
| Offline notifications | Skipped | Courier delivery |
| Audit trail | Local structured logs | Chronicle audit events |

---

## 11. Configuration

See `herald.toml.example` for a complete annotated example.

```toml
[server]
ws_bind = "0.0.0.0:6200"
http_bind = "0.0.0.0:6201"
log_level = "info"

[store]
path = "./herald-data/herald.db"
event_ttl_days = 7

[auth]
jwt_secret = "your-hmac-secret"
jwt_issuer = "app-backend"

[auth.api]
tokens = ["herald-api-token-1"]

[presence]
linger_secs = 10
manual_override_ttl_secs = 14400

[webhook]
url = "https://app.example.com/webhooks/herald"
secret = "webhook-signing-secret"
retries = 3

# Optional — omit to run without ShroudB
[shroudb]
moat_addr = "shroudb-moat.railway.internal:8201"
auth_token = "herald-service-token"
```

---

## 12. Implementation Phasing

| Phase | Scope | ShroudB? |
|-------|-------|----------|
| **1. Skeleton** | `herald-core` types. `herald-server` with axum + WebSocket. Auth, subscribe, send/receive with in-memory state. WAL store. | No |
| **2. Fan-out + Presence** | ConnectionRegistry, StreamRegistry, PresenceTracker. Multi-connection fan-out. Cursors. Reconnect catch-up via `last_seen_at`. | No |
| **3. HTTP API + Webhook** | Stream CRUD, member management, event query/inject. Signed webhook delivery with retries. | No |
| **4. Sentry + Courier + Chronicle** | Optional Sentry policy eval. Courier offline notifications. Chronicle audit trail. | Yes |
| **5. TypeScript SDK** | `herald-sdk-typescript` — HeraldClient, reconnect with backoff, event dedup, client-side store. | No |
| **6. Hardening** | Rate limiting. TLS config. Prometheus metrics. Graceful shutdown. Docker image. | No |
