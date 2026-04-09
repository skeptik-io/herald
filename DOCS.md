# Herald — Reference Documentation

## Server

### Binary

```
herald [config-path]
```

| Argument | Default | Description |
|---|---|---|
| `config-path` | `herald.toml` | Path to TOML configuration file |

A default tenant is auto-created on first start.

**Environment variables:**
- `SHROUDB_MASTER_KEY` — 64-character hex string (32 bytes). Required for WAL storage.

### Configuration

#### `[server]`

| Key | Type | Default | Description |
|---|---|---|---|
| `bind` | string | `0.0.0.0:6200` | Listen address (single port for HTTP + WebSocket) |
| `log_level` | string | `info` | `trace`, `debug`, `info`, `warn`, `error` |
| `max_events_per_sec` | u32 | `10` | Per-connection WS event rate limit |
| `api_rate_limit` | u32 | `100` | HTTP API requests per minute |
| `shutdown_timeout_secs` | u64 | `30` | Graceful shutdown drain timeout |

#### `[store]`

| Key | Type | Default | Description |
|---|---|---|---|
| `path` | string | `./herald-data` | Data directory for WAL + snapshots |
| `event_ttl_days` | u32 | `7` | Events older than this are pruned hourly |

#### `[auth]`

| Key | Type | Required | Description |
|---|---|---|---|
| `password` | string | yes | Server admin password — used as bearer token for `/admin/*` endpoints |
| `token_window_secs` | u64 | no | HMAC token validity window in seconds |

#### `[presence]`

| Key | Type | Default | Description |
|---|---|---|---|
| `linger_secs` | u64 | `10` | Delay before broadcasting offline |
| `manual_override_ttl_secs` | u64 | `14400` | DND/away expiry (4 hours) |

#### `[webhook]`

| Key | Type | Description |
|---|---|---|
| `url` | string | Webhook endpoint URL |
| `secret` | string | HMAC-SHA256 key for `X-Herald-Signature` |
| `retries` | u32 | Retry count (default 3) |
| `events` | string[] | Event types to send (default: all) |

#### `[tls]`

| Key | Type | Description |
|---|---|---|
| `cert_path` | string | PEM certificate file |
| `key_path` | string | PEM private key file |

#### `[shroudb]` (Remote engines — optional)

| Key | Type | Description |
|---|---|---|
| `sentry_addr` | string | Sentry TCP address |
| `sentry_token` | string | Sentry auth token |
| `courier_addr` | string | Courier TCP address |
| `courier_token` | string | Courier auth token |
| `chronicle_addr` | string | Chronicle TCP address |
| `chronicle_token` | string | Chronicle auth token |

When `[shroudb]` is absent, engines run embedded (in-process) using the same storage directory.

---

## WebSocket Auth

WebSocket auth uses HMAC-SHA256 signed tokens (key+secret model). No JWT.

### Connection

Connect via query params:

```
wss://herald.example.com/ws?key=<tenant_key>&token=<hmac_token>&user_id=<user>&streams=stream1,stream2
```

| Param | Description |
|---|---|
| `key` | Tenant key (auto-generated on tenant creation) |
| `token` | HMAC-SHA256 signed token (generated client-side using tenant secret) |
| `user_id` | User identifier |
| `streams` | Comma-separated stream IDs to authorize |

The token is validated on WebSocket upgrade. No in-frame `auth` event is needed.

---

## WebSocket Protocol

Single port (default `:6200`). WebSocket upgrade at `/ws`. JSON text frames only.

JSON text frames. Envelope: `{"type": "...", "ref": "...", "payload": {...}}`

### Client → Server

| Type | Payload | Description |
|---|---|---|
| `subscribe` | `{streams: []}` | Subscribe to streams |
| `unsubscribe` | `{streams: []}` | Unsubscribe |
| `event.publish` | `{stream, body, meta?, parent_id?}` | Send a event |
| `cursor.update` | `{stream, seq}` | Update read position |
| `presence.set` | `{status}` | `online`, `away`, `dnd` |
| `typing.start` / `typing.stop` | `{stream}` | Ephemeral |
| `events.fetch` | `{stream, before?, limit?}` | History |
| `event.trigger` | `{stream, event, data?}` | Trigger ephemeral event (not persisted) |
| `reaction.add` | `{stream, event_id, emoji}` | Add a reaction to a event |
| `reaction.remove` | `{stream, event_id, emoji}` | Remove a reaction from a event |
| `ping` | — | Keepalive |

### Server → Client

| Type | Payload | Description |
|---|---|---|
| `auth_ok` | `{user_id, connection_id, server_time, heartbeat_interval}` | Auth success (sent after upgrade) |
| `auth_error` | `{code, event}` | Auth failure |
| `subscribed` | `{stream, members, cursor, latest_seq}` | Stream joined |
| `event.new` | `{stream, id, seq, sender, body, meta?, sent_at}` | New event |
| `event.ack` | `{id, seq, sent_at}` | Send confirmed |
| `events.batch` | `{stream, events[], has_more}` | History/catch-up |
| `presence.changed` | `{user_id, presence}` | Presence change |
| `cursor.moved` | `{stream, user_id, seq}` | Read position change |
| `member.joined` / `member.left` | `{stream, user_id, role}` | Membership |
| `typing` | `{stream, user_id, active}` | Typing indicator |
| `event.received` | `{stream, event, sender, data?}` | Ephemeral event from another client |
| `watchlist.online` | `{user_ids: []}` | Watched users came online |
| `watchlist.offline` | `{user_ids: []}` | Watched users went offline |
| `reaction.changed` | `{stream, event_id, emoji, user_id, action}` | Reaction added/removed (`action`: `"add"` or `"remove"`) |
| `stream.subscriber_count` | `{stream, count}` | Stream subscriber count changed |
| `system.token_expiring` | `{expires_at}` | Token expiring in 60s |
| `error` | `{code, event}` | Error |

### Ephemeral Events

Ephemeral events are lightweight events that fan out to stream subscribers but are NOT persisted to storage. They are ideal for:
- Custom application events
- Live cursors, selections
- Game state updates
- Collaborative editing signals
- Any high-frequency, transient data

The sender does NOT receive their own event (excluded from fan-out). The event is acknowledged with a `pong` if a `ref` is provided.

Ephemeral events do not trigger webhooks and do not affect event history or sequence numbers.

### Error Codes

`TOKEN_EXPIRED`, `TOKEN_INVALID`, `UNAUTHORIZED`, `NOT_SUBSCRIBED`, `STREAM_NOT_FOUND`, `RATE_LIMITED`, `BAD_REQUEST`, `INTERNAL`

---

## HTTP API

All endpoints share the single port (default `:6200`).

### Tenant API (Bearer token from tenant's key+secret)

| Method | Path | Description |
|---|---|---|
| `POST /streams` | Create stream | `{id, name, meta?, public?}` |
| `GET /streams` | List streams | Returns `{streams: [...]}` |
| `GET /streams/:id` | Get stream | |
| `PATCH /streams/:id` | Update stream | `{name?, meta?, archived?}` |
| `DELETE /streams/:id` | Delete stream | |
| `POST /streams/:id/members` | Add member | `{user_id, role?}` |
| `GET /streams/:id/members` | List members | |
| `PATCH /streams/:id/members/:uid` | Update role | `{role}` |
| `DELETE /streams/:id/members/:uid` | Remove member | |
| `POST /streams/:id/events` | Inject event | `{sender, body, meta?, exclude_connection?}` |
| `GET /streams/:id/events` | List events | `?before=&after=&limit=` |
| `DELETE /streams/:id/events/:msg_id` | Delete/redact event | Soft-deletes: clears body, sets `meta.deleted=true` |
| `PATCH /streams/:id/events/:msg_id` | Edit event | `{body}` |
| `GET /streams/:id/events/:msg_id/reactions` | List reactions | Returns `{reactions: [{emoji, count, users}]}` |
| `POST /streams/:id/trigger` | Trigger ephemeral event | `{event, data?, exclude_connection?}` |
| `GET /streams/:id/cursors` | Read cursors | |
| `POST /blocks` | Block a user | `{user_id, blocked_id}` |
| `DELETE /blocks` | Unblock a user | `{user_id, blocked_id}` |
| `GET /blocks/:user_id` | List blocked users | Returns `{blocked: [...]}` |
| `GET /streams/:id/presence` | Stream presence | |
| `GET /presence/:uid` | User presence | |
| `GET /stats` | Tenant-scoped stats | Connection count, stream count, event rate for this tenant |

### Admin API (Bearer token from `auth.password`)

| Method | Path | Description |
|---|---|---|
| `POST /admin/tenants` | Create tenant | `{id, name}` — auto-generates `key` and `secret` |
| `GET /admin/tenants` | List tenants | |
| `GET /admin/tenants/:id` | Get tenant | |
| `PATCH /admin/tenants/:id` | Update tenant | `{name?, plan?, config?}` |
| `DELETE /admin/tenants/:id` | Delete tenant | |
| `GET /admin/tenants/:id/streams` | List tenant streams | Admin view of streams for a tenant |
| `GET /admin/tenants/:id/audit` | Query audit log | `?operation=&resource_type=&resource_id=&actor=&result=&since=&until=&limit=` |
| `GET /admin/tenants/:id/audit/count` | Count audit entries | Same filters as query (excluding `limit`) |
| `GET /admin/connections` | List active connections | Active WebSocket connections |
| `GET /admin/events` | List admin events | Recent admin events |
| `GET /admin/events/stream` | SSE admin event stream | Server-Sent Events stream |
| `GET /admin/errors` | List recent errors | Error log |
| `GET /admin/stats` | Platform stats | Aggregate stats across all tenants |

### Tenant Self-Service API (`/self/*`)

| Method | Path | Description |
|---|---|---|
| `GET /self/connections` | List tenant connections | Active WebSocket connections for this tenant |
| `GET /self/events` | List tenant events | Recent events for this tenant |
| `GET /self/errors` | List tenant errors | Recent errors for this tenant |
| `POST /self/secret/rotate` | Rotate tenant secret | Generates a new secret, invalidates the old one |
| `POST /self/tokens` | Create API token | `{scope?}` — optional scope restriction |
| `GET /self/tokens` | List API tokens | |
| `DELETE /self/tokens/:token` | Delete API token | |

### Operational (No auth required)

| Method | Path | Description |
|---|---|---|
| `GET /health` | Health check | `{status, connections, streams, uptime_secs}` |
| `GET /health/live` | Liveness probe | Returns 200 if process is running |
| `GET /health/ready` | Readiness probe | Returns 200 when store is initialized |
| `GET /metrics` | Prometheus metrics | Histograms + counters |

### Prometheus Metrics

| Metric | Type | Description |
|---|---|---|
| `herald_connections_total` | gauge | WebSocket connections |
| `herald_streams_total` | gauge | Streams |
| `herald_events_sent_total` | counter | Events sent |
| `herald_events_dropped_total` | counter | Dropped (backpressure) |
| `herald_ws_auth_failures_total` | counter | Auth failures |
| `herald_uptime_seconds` | gauge | Uptime |
| `herald_event_total_seconds` | histogram | End-to-end send latency |
| `herald_event_store_seconds` | histogram | WAL store |
| `herald_event_fanout_seconds` | histogram | Fan-out |

---

## Additional Features

### Public Streams

Streams can be created with `"public": true` to allow any authenticated user to subscribe without being pre-added as a member. When a user subscribes to a public stream, they are automatically added as a member with `Member` role.

Public streams still require the stream ID to be present in the connection's `streams` param -- authorization is enforced, but membership is not required upfront.

Private streams (default, `public: false`) continue to require membership before subscribing.

### Stream Archival

Streams can be archived via `PATCH /streams/:id` with `{"archived": true}`. Archived streams remain readable but no new events can be sent. Unarchive by setting `archived` to `false`.

### Webhook Event Filtering

The `[webhook]` config supports an `events` field to filter which events trigger webhook calls:

```toml
[webhook]
url = "https://example.com/hook"
secret = "whsec_..."
events = ["event.new", "member.joined", "member.left"]
```

When `events` is omitted, all events are sent. Supported event types match server-to-client event types.

### API Key Scoping

API tokens can be created with a `scope` to restrict access:

```
POST /self/tokens
{"scope": "read-only"}
```

When a scoped token is used, the scope is available via middleware for route-level enforcement. `scope: null` (default) grants full access.

### Pagination

List endpoints support `limit` and `offset` query parameters:

| Parameter | Type | Default | Description |
|---|---|---|---|
| `limit` | u32 | 50 | Maximum items to return |
| `offset` | u32 | 0 | Number of items to skip |

Applies to: `GET /streams`, `GET /streams/:id/members`, `GET /streams/:id/events` (also supports `before`/`after` sequence-based cursors), `GET /admin/tenants`, `GET /self/tokens`.

---

## Storage

Herald uses ShroudB's WAL-based storage engine. Event bodies are stored as opaque bytes — Herald does not interpret, encrypt, or index them.

**Namespaces:**
- `herald.tenants` — tenant configurations
- `herald.api_tokens` — per-tenant API bearer tokens
- `herald.streams` — stream metadata (key: `{tenant_id}/{stream_id}`)
- `herald.members` — stream membership (key: `{tenant_id}/{stream_id}/{user_id}`)
- `herald.events` — events (key: `{tenant_id}/{stream_id}/{seq:020}`)
- `herald.cursors` — read positions (key: `{tenant_id}/{stream_id}/{user_id}`)

---

## ShroudB Engine Modes

Sentry initializes **embedded by default** — no configuration needed. It shares Herald's storage engine with separate namespaces. Remote mode is available as an override via `[shroudb]` config.

| Engine | Default | Remote Override | Purpose |
|---|---|---|---|
| **Sentry** | Embedded (automatic) | `sentry_addr` in `[shroudb]` | Authorization |
| **Courier** | Disabled | `courier_addr` in `[shroudb]` | Offline notifications |
| **Chronicle** | Disabled | `chronicle_addr` in `[shroudb]` | Audit trail |

Remote wrappers add circuit breaker (5 failures → open, 30s cooldown) + 10s request timeout. Sentry is fail-open when circuit trips.

---

## Watchlist (Friend Online/Offline Tracking)

Track online/offline status of specific users across the entire tenant, regardless of shared streams. Include a `watchlist` query param in the WebSocket connection URL.

On connect, the server sends `watchlist.online` with any watched users who are already online. When a watched user connects (first connection) or disconnects (last connection, after linger), the server sends `watchlist.online` or `watchlist.offline` to all watchers.

Watchlist tracking is per-tenant and respects the presence linger period -- quick reconnects do not produce spurious offline events.

Example connection:
```
/ws?key=...&token=...&user_id=alice&streams=chat&watchlist=bob,charlie
```

Example server events:
```json
{"type": "watchlist.online", "payload": {"user_ids": ["bob"]}}
{"type": "watchlist.offline", "payload": {"user_ids": ["charlie"]}}
```

---

## Subscription Count Broadcasts

When the subscriber count for a stream changes (subscribe, unsubscribe, or disconnect), the server broadcasts a `stream.subscriber_count` event to all current subscribers of that stream.

Example server event:
```json
{"type": "stream.subscriber_count", "payload": {"stream": "chat", "count": 5}}
```

This fires on explicit subscribe/unsubscribe and on disconnect (after connection cleanup). The count reflects the number of active WebSocket connections subscribed to the stream.

---

## Server-Side Ephemeral Events

`POST /streams/:id/trigger` sends an ephemeral event to all stream subscribers without persisting to storage. The sender is `_server`. Useful for server-initiated signals (session events, notifications, live updates).

Supports `exclude_connection` to skip a specific WebSocket connection.

---

## Sender Exclusion

When injecting events via the HTTP API, pass `exclude_connection` (a connection ID as a number) to exclude that specific WebSocket connection from receiving the fan-out. This is useful for optimistic UI updates where the client already shows the event locally.

```json
POST /streams/chat/events
{
  "sender": "alice",
  "body": "Hello!",
  "exclude_connection": 12345
}
```

The connection ID is available in the `auth_ok` response or can be tracked by the application server. If the specified connection ID does not match any active subscriber, the event is delivered to all subscribers as normal.

---

## Authorized Connections

WebSocket connections are authenticated on upgrade via query params. Invalid tokens are rejected before the WebSocket handshake completes. Auth failures are tracked via the `herald_ws_auth_failures_total` Prometheus metric.

---

## Reactions

Reactions are per-event emoji/string markers. Any stream member can add or remove their own reaction via WebSocket.

**WebSocket flow:**

```json
// Add reaction
{"type": "reaction.add", "ref": "r1", "payload": {"stream": "chat", "event_id": "abc-123", "emoji": "thumbsup"}}

// All stream subscribers receive:
{"type": "reaction.changed", "payload": {"stream": "chat", "event_id": "abc-123", "emoji": "thumbsup", "user_id": "alice", "action": "add"}}

// Remove reaction
{"type": "reaction.remove", "ref": "r2", "payload": {"stream": "chat", "event_id": "abc-123", "emoji": "thumbsup"}}
```

**HTTP API:** `GET /streams/:id/events/:msg_id/reactions` returns all reactions for a event grouped by emoji, including counts and user lists.

Emoji strings are limited to 32 bytes. Each user can have at most one reaction per emoji per event (idempotent add).

---

## File/Media Attachments

Attachments are stored in the event `meta` field with a standardized schema. Herald validates the structure but does not store files -- it validates and passes through attachment metadata. File storage and URL generation is the responsibility of the application server.

**Schema:**

```json
{
  "sender": "alice",
  "body": "check this file",
  "meta": {
    "attachments": [
      {
        "url": "https://cdn.example.com/file.pdf",
        "content_type": "application/pdf",
        "size": 102400,
        "name": "report.pdf"
      }
    ]
  }
}
```

**Validation rules:**
- Maximum 10 attachments per event
- Each attachment must have a `url` field (string)
- Applied on both HTTP inject and WebSocket `event.publish`

---

## User Blocking

Per-tenant block list managed via HTTP API. When user A blocks user B, the block is stored server-side. Client SDKs should use the block list to filter events locally.

**HTTP API:**

| Method | Path | Body | Description |
|---|---|---|---|
| `POST /blocks` | | `{user_id, blocked_id}` | Block a user |
| `DELETE /blocks` | | `{user_id, blocked_id}` | Unblock a user |
| `GET /blocks/:user_id` | | | List blocked users |

The block list is per-tenant and directional (A blocking B does not mean B blocks A). Blocking does not remove the user from streams or prevent event delivery at the server level -- it provides the data for client-side filtering, which is the standard approach used by major chat platforms.
