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
- `SHROUDB_MASTER_KEY` — 64-character hex string (32 bytes). Required. Encrypts all stored data.

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

#### `[tls]`

| Key | Type | Description |
|---|---|---|
| `cert_path` | string | PEM certificate file |
| `key_path` | string | PEM private key file |

#### `[shroudb]` (Remote engines — optional)

| Key | Type | Description |
|---|---|---|
| `cipher_addr` | string | Cipher TCP address |
| `cipher_token` | string | Cipher auth token |
| `veil_addr` | string | Veil TCP address |
| `veil_token` | string | Veil auth token |
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
| `messages.search` | `{room, query, limit?}` | Search (requires Veil) |
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
| `system.token_expiring` | `{expires_at}` | JWT expiring in 60s |
| `error` | `{code, message}` | Error |

### Error Codes

`TOKEN_EXPIRED`, `TOKEN_INVALID`, `UNAUTHORIZED`, `NOT_SUBSCRIBED`, `ROOM_NOT_FOUND`, `RATE_LIMITED`, `BAD_REQUEST`, `SEARCH_UNAVAILABLE`, `INTERNAL`

---

## HTTP API (Port 6201)

### Tenant API (Bearer token from `api_tokens` table)

| Method | Path | Description |
|---|---|---|
| `POST /rooms` | Create room | `{id, name, encryption_mode?, meta?}` |
| `GET /rooms/:id` | Get room | |
| `PATCH /rooms/:id` | Update room | `{name?, meta?}` |
| `DELETE /rooms/:id` | Delete room | |
| `POST /rooms/:id/members` | Add member | `{user_id, role?}` |
| `GET /rooms/:id/members` | List members | |
| `PATCH /rooms/:id/members/:uid` | Update role | `{role}` |
| `DELETE /rooms/:id/members/:uid` | Remove member | |
| `POST /rooms/:id/messages` | Inject message | `{sender, body, meta?}` |
| `GET /rooms/:id/messages` | List messages | `?before=&after=&limit=` |
| `GET /rooms/:id/messages/search` | Search | `?q=&limit=` |
| `GET /rooms/:id/cursors` | Read cursors | |
| `GET /rooms/:id/presence` | Room presence | |
| `GET /presence/:uid` | User presence | |

### Admin API (Bearer token from `auth.super_admin_token`)

| Method | Path | Description |
|---|---|---|
| `POST /admin/tenants` | Create tenant | `{id, name, jwt_secret}` |
| `GET /admin/tenants` | List tenants | |
| `GET /admin/tenants/:id` | Get tenant | |
| `PATCH /admin/tenants/:id` | Update tenant | `{name?, plan?, config?}` |
| `DELETE /admin/tenants/:id` | Delete tenant | |
| `POST /admin/tenants/:id/tokens` | Create API token | |
| `GET /admin/tenants/:id/tokens` | List API tokens | |

### Operational (No auth required)

| Method | Path | Description |
|---|---|---|
| `GET /health` | Health check | `{status, connections, rooms, uptime_secs}` |
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
| `herald_message_encrypt_seconds` | histogram | Cipher encrypt |
| `herald_message_store_seconds` | histogram | WAL store |
| `herald_message_index_seconds` | histogram | Veil index |
| `herald_message_fanout_seconds` | histogram | Fan-out |
| `herald_message_decrypt_seconds` | histogram | Cipher decrypt |
| `herald_search_seconds` | histogram | Veil search |

---

## Storage

Herald uses ShroudB's WAL-based storage engine. All data is encrypted at rest using a master key derived via HKDF.

**Namespaces:**
- `herald.tenants` — tenant configurations
- `herald.api_tokens` — per-tenant API bearer tokens
- `herald.rooms` — room metadata (key: `{tenant_id}/{room_id}`)
- `herald.members` — room membership (key: `{tenant_id}/{room_id}/{user_id}`)
- `herald.messages` — messages (key: `{tenant_id}/{room_id}/{seq:020}`)
- `herald.cursors` — read positions (key: `{tenant_id}/{room_id}/{user_id}`)

**Encryption modes** (per-room, set at creation time via `encryption_mode` field):

| Mode | Message Bodies | Search | Cost |
|---|---|---|---|
| `plaintext` (default) | Stored as-is in WAL | Veil indexes if available | ~0.10ms per message |
| `server_encrypted` | Cipher-encrypted before WAL write | Veil blind indexes | ~0.25ms per message (embedded) |

Both modes benefit from storage-level encryption (the WAL is always AES-256-GCM encrypted via the master key). `server_encrypted` adds an additional per-message Cipher keyring encryption on top, so message bodies are double-encrypted and only readable with both the master key and the room's Cipher keyring.

To create an encrypted room:
```bash
curl -X POST http://localhost:6201/rooms \
  -H "Authorization: Bearer $TOKEN" \
  -d '{"id": "secure", "name": "Secure Room", "encryption_mode": "server_encrypted"}'
```

Each `server_encrypted` room automatically gets:
- A Cipher keyring: `herald-{tenant_id}-room-{room_id}` (AES-256-GCM)
- A Veil blind index: `herald-{tenant_id}-room-{room_id}`

---

## ShroudB Engine Modes

Cipher, Veil, and Sentry initialize **embedded by default** — no configuration needed. They share Herald's storage engine with separate namespaces. Remote mode is available as an override via `[shroudb]` config.

| Engine | Default | Remote Override | Purpose |
|---|---|---|---|
| **Cipher** | Embedded (automatic) | `cipher_addr` in `[shroudb]` | Message encryption |
| **Veil** | Embedded (automatic) | `veil_addr` in `[shroudb]` | Blind index search |
| **Sentry** | Embedded (automatic) | `sentry_addr` in `[shroudb]` | Authorization |
| **Courier** | Disabled | `courier_addr` in `[shroudb]` | Offline notifications |
| **Chronicle** | Disabled | `chronicle_addr` in `[shroudb]` | Audit trail |

Remote wrappers add circuit breaker (5 failures → open, 30s cooldown) + 10s request timeout. Sentry is fail-open when circuit trips.
