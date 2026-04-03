# Herald — Reference Documentation

## Server

### Binary

```
herald [--single-tenant|--multi-tenant] [config-path]
```

| Flag | Default | Description |
|---|---|---|
| `--single-tenant` | yes | Auto-creates a `default` tenant from config |
| `--multi-tenant` | no | Tenants managed via admin API. Requires `auth.super_admin_token`. |
| `config-path` | `herald.toml` | Path to TOML configuration file |

**Environment variables:**
- `SHROUDB_MASTER_KEY` — 64-character hex string (32 bytes). Required for WAL storage.

### Configuration

#### `[server]`

| Key | Type | Default | Description |
|---|---|---|---|
| `ws_bind` | string | `0.0.0.0:6200` | WebSocket listen address |
| `http_bind` | string | `0.0.0.0:6201` | HTTP API listen address |
| `log_level` | string | `info` | `trace`, `debug`, `info`, `warn`, `error` |
| `max_messages_per_sec` | u32 | `10` | Per-connection WS message rate limit |
| `api_rate_limit` | u32 | `100` | HTTP API requests per minute |
| `shutdown_timeout_secs` | u64 | `30` | Graceful shutdown drain timeout |

#### `[store]`

| Key | Type | Default | Description |
|---|---|---|---|
| `path` | string | `./herald-data` | Data directory for WAL + snapshots |
| `message_ttl_days` | u32 | `7` | Messages older than this are pruned hourly |

#### `[auth]`

| Key | Type | Required | Description |
|---|---|---|---|
| `jwt_secret` | string | single-tenant | HMAC-SHA256 secret for default tenant JWT validation |
| `jwt_issuer` | string | no | Required JWT `iss` claim for default tenant |
| `super_admin_token` | string | multi-tenant | Bearer token for `/admin/*` endpoints |

#### `[auth.api]`

| Key | Type | Description |
|---|---|---|
| `tokens` | string[] | Bearer tokens for default tenant (single-tenant mode) |

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

## JWT Claims

| Claim | Type | Required | Description |
|---|---|---|---|
| `sub` | string | yes | User ID |
| `tenant` | string | yes | Tenant ID |
| `rooms` | string[] | yes | Room IDs the user may subscribe to |
| `exp` | number | yes | Expiration (Unix seconds) |
| `iat` | number | yes | Issued-at |
| `iss` | string | no | Issuer (validated against tenant config) |
| `watchlist` | string[] | no | User IDs to track online/offline status |

---

## WebSocket Protocol

WebSocket is available at two endpoints:

- **`/ws` on the HTTP port** (primary) — use this behind reverse proxies and in Docker. Browsers connect to `wss://your-domain/ws`. TLS is terminated by the proxy.
- **Standalone WS port** (advanced) — direct TCP connections. Configure with `ws_bind`. Use when Herald terminates TLS itself via `[tls]` config, or on private networks.

JSON text frames. Envelope: `{"type": "...", "ref": "...", "payload": {...}}`

### Client → Server

| Type | Payload | Description |
|---|---|---|
| `auth` | `{token, last_seen_at?}` | Authenticate (required within 5s) |
| `auth.refresh` | `{token}` | Refresh JWT without reconnecting |
| `subscribe` | `{rooms: []}` | Subscribe to rooms |
| `unsubscribe` | `{rooms: []}` | Unsubscribe |
| `message.send` | `{room, body, meta?}` | Send a message |
| `cursor.update` | `{room, seq}` | Update read position |
| `presence.set` | `{status}` | `online`, `away`, `dnd` |
| `typing.start` / `typing.stop` | `{room}` | Ephemeral |
| `messages.fetch` | `{room, before?, limit?}` | History |
| `event.trigger` | `{room, event, data?}` | Trigger ephemeral event (not persisted) |
| `ping` | — | Keepalive |

### Server → Client

| Type | Payload | Description |
|---|---|---|
| `auth_ok` | `{user_id, server_time, heartbeat_interval}` | Auth success |
| `auth_error` | `{code, message}` | Auth failure |
| `subscribed` | `{room, members, cursor, latest_seq}` | Room joined |
| `message.new` | `{room, id, seq, sender, body, meta?, sent_at}` | New message |
| `message.ack` | `{id, seq, sent_at}` | Send confirmed |
| `messages.batch` | `{room, messages[], has_more}` | History/catch-up |
| `presence.changed` | `{user_id, presence}` | Presence change |
| `cursor.moved` | `{room, user_id, seq}` | Read position change |
| `member.joined` / `member.left` | `{room, user_id, role}` | Membership |
| `typing` | `{room, user_id, active}` | Typing indicator |
| `event.received` | `{room, event, sender, data?}` | Ephemeral event from another client |
| `watchlist.online` | `{user_ids: []}` | Watched users came online |
| `watchlist.offline` | `{user_ids: []}` | Watched users went offline |
| `room.subscriber_count` | `{room, count}` | Room subscriber count changed |
| `system.token_expiring` | `{expires_at}` | JWT expiring in 60s |
| `error` | `{code, message}` | Error |

### Ephemeral Events

Ephemeral events are lightweight events that fan out to room subscribers but are NOT persisted to storage. They are ideal for:
- Custom application events
- Live cursors, selections
- Game state updates
- Collaborative editing signals
- Any high-frequency, transient data

The sender does NOT receive their own event (excluded from fan-out). The event is acknowledged with a `pong` if a `ref` is provided.

Ephemeral events do not trigger webhooks and do not affect message history or sequence numbers.

### Error Codes

`TOKEN_EXPIRED`, `TOKEN_INVALID`, `UNAUTHORIZED`, `NOT_SUBSCRIBED`, `ROOM_NOT_FOUND`, `RATE_LIMITED`, `BAD_REQUEST`, `INTERNAL`

---

## HTTP API (Port 6201)

### Tenant API (Bearer token from `api_tokens` table)

| Method | Path | Description |
|---|---|---|
| `POST /rooms` | Create room | `{id, name, meta?, public?}` |
| `GET /rooms` | List rooms | Returns `{rooms: [...]}` |
| `GET /rooms/:id` | Get room | |
| `PATCH /rooms/:id` | Update room | `{name?, meta?, archived?}` |
| `DELETE /rooms/:id` | Delete room | |
| `POST /rooms/:id/members` | Add member | `{user_id, role?}` |
| `GET /rooms/:id/members` | List members | |
| `PATCH /rooms/:id/members/:uid` | Update role | `{role}` |
| `DELETE /rooms/:id/members/:uid` | Remove member | |
| `POST /rooms/:id/messages` | Inject message | `{sender, body, meta?, exclude_connection?}` |
| `GET /rooms/:id/messages` | List messages | `?before=&after=&limit=` |
| `DELETE /rooms/:id/messages/:msg_id` | Delete/redact message | Soft-deletes: clears body, sets `meta.deleted=true` |
| `GET /rooms/:id/cursors` | Read cursors | |
| `GET /rooms/:id/presence` | Room presence | |
| `GET /presence/:uid` | User presence | |
| `GET /stats` | Tenant-scoped stats | Connection count, room count, message rate for this tenant |

### Admin API (Bearer token from `auth.super_admin_token`)

| Method | Path | Description |
|---|---|---|
| `POST /admin/tenants` | Create tenant | `{id, name, jwt_secret}` |
| `GET /admin/tenants` | List tenants | |
| `GET /admin/tenants/:id` | Get tenant | |
| `PATCH /admin/tenants/:id` | Update tenant | `{name?, plan?, config?}` |
| `DELETE /admin/tenants/:id` | Delete tenant | |
| `POST /admin/tenants/:id/tokens` | Create API token | `{scope?}` — optional scope restriction |
| `GET /admin/tenants/:id/tokens` | List API tokens | |
| `DELETE /admin/tenants/:id/tokens/:token` | Delete API token | Verifies token belongs to tenant |
| `GET /admin/tenants/:id/rooms` | List tenant rooms | Admin view of rooms for a tenant |
| `GET /admin/connections` | List active connections | Active WebSocket connections |
| `GET /admin/events` | List admin events | Recent admin events |
| `GET /admin/events/stream` | SSE admin event stream | Server-Sent Events stream |
| `GET /admin/errors` | List recent errors | Error log |
| `GET /admin/stats` | Platform stats | Aggregate stats across all tenants |

### Operational (No auth required)

| Method | Path | Description |
|---|---|---|
| `GET /health` | Health check | `{status, connections, rooms, uptime_secs}` |
| `GET /health/live` | Liveness probe | Returns 200 if process is running |
| `GET /health/ready` | Readiness probe | Returns 200 when store is initialized |
| `GET /metrics` | Prometheus metrics | Histograms + counters |

### Prometheus Metrics

| Metric | Type | Description |
|---|---|---|
| `herald_connections_total` | gauge | WebSocket connections |
| `herald_rooms_total` | gauge | Rooms |
| `herald_messages_sent_total` | counter | Messages sent |
| `herald_messages_dropped_total` | counter | Dropped (backpressure) |
| `herald_ws_auth_failures_total` | counter | Auth failures |
| `herald_uptime_seconds` | gauge | Uptime |
| `herald_message_total_seconds` | histogram | End-to-end send latency |
| `herald_message_store_seconds` | histogram | WAL store |
| `herald_message_fanout_seconds` | histogram | Fan-out |

---

## Additional Features

### Public Rooms

Rooms can be created with `"public": true` to allow any authenticated user to subscribe without being pre-added as a member. When a user subscribes to a public room, they are automatically added as a member with `Member` role.

Public rooms still require the room ID to be present in the JWT `rooms` claim -- authorization is enforced, but membership is not required upfront.

Private rooms (default, `public: false`) continue to require membership before subscribing.

### Room Archival

Rooms can be archived via `PATCH /rooms/:id` with `{"archived": true}`. Archived rooms remain readable but no new messages can be sent. Unarchive by setting `archived` to `false`.

### Webhook Event Filtering

The `[webhook]` config supports an `events` field to filter which events trigger webhook calls:

```toml
[webhook]
url = "https://example.com/hook"
secret = "whsec_..."
events = ["message.new", "member.joined", "member.left"]
```

When `events` is omitted, all events are sent. Supported event types match server-to-client message types.

### API Key Scoping

API tokens can be created with a `scope` to restrict access:

```
POST /admin/tenants/:id/tokens
{"scope": "read-only"}
```

When a scoped token is used, the scope is available via middleware for route-level enforcement. `scope: null` (default) grants full access.

### Pagination

List endpoints support `limit` and `offset` query parameters:

| Parameter | Type | Default | Description |
|---|---|---|---|
| `limit` | u32 | 50 | Maximum items to return |
| `offset` | u32 | 0 | Number of items to skip |

Applies to: `GET /rooms`, `GET /rooms/:id/members`, `GET /rooms/:id/messages` (also supports `before`/`after` sequence-based cursors), `GET /admin/tenants`, `GET /admin/tenants/:id/tokens`.

---

## Storage

Herald uses ShroudB's WAL-based storage engine. Message bodies are stored as opaque bytes — Herald does not interpret, encrypt, or index them.

**Namespaces:**
- `herald.tenants` — tenant configurations
- `herald.api_tokens` — per-tenant API bearer tokens
- `herald.rooms` — room metadata (key: `{tenant_id}/{room_id}`)
- `herald.members` — room membership (key: `{tenant_id}/{room_id}/{user_id}`)
- `herald.messages` — messages (key: `{tenant_id}/{room_id}/{seq:020}`)
- `herald.cursors` — read positions (key: `{tenant_id}/{room_id}/{user_id}`)

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

Track online/offline status of specific users across the entire tenant, regardless of shared rooms. Add a `watchlist` array to the JWT claims containing user IDs to watch.

On connect, the server sends `watchlist.online` with any watched users who are already online. When a watched user connects (first connection) or disconnects (last connection, after linger), the server sends `watchlist.online` or `watchlist.offline` to all watchers.

Watchlist tracking is per-tenant and respects the presence linger period -- quick reconnects do not produce spurious offline events.

Example JWT claims:
```json
{
  "sub": "alice",
  "tenant": "acme",
  "rooms": ["chat"],
  "watchlist": ["bob", "charlie"],
  "exp": 1700000000,
  "iat": 1699996400,
  "iss": "myapp"
}
```

Example server events:
```json
{"type": "watchlist.online", "payload": {"user_ids": ["bob"]}}
{"type": "watchlist.offline", "payload": {"user_ids": ["charlie"]}}
```

---

## Subscription Count Broadcasts

When the subscriber count for a room changes (subscribe, unsubscribe, or disconnect), the server broadcasts a `room.subscriber_count` event to all current subscribers of that room.

Example server event:
```json
{"type": "room.subscriber_count", "payload": {"room": "chat", "count": 5}}
```

This fires on explicit subscribe/unsubscribe and on disconnect (after connection cleanup). The count reflects the number of active WebSocket connections subscribed to the room.

---

## Sender Exclusion

When injecting messages via the HTTP API, pass `exclude_connection` (a connection ID as a number) to exclude that specific WebSocket connection from receiving the fan-out. This is useful for optimistic UI updates where the client already shows the message locally.

```json
POST /rooms/chat/messages
{
  "sender": "alice",
  "body": "Hello!",
  "exclude_connection": 12345
}
```

The connection ID is available in the `auth_ok` response or can be tracked by the application server. If the specified connection ID does not match any active subscriber, the message is delivered to all subscribers as normal.

---

## Authorized Connections

WebSocket connections must authenticate within 5 seconds or they are disconnected. Unauthenticated connections do not count toward the per-tenant connection limit. The connection limit is enforced only after successful JWT validation.

This means:
- Unauthenticated connections cannot exhaust tenant quota
- The 5-second auth timeout prevents resource exhaustion from idle connections
- Auth failures are tracked via the `herald_ws_auth_failures_total` Prometheus metric
