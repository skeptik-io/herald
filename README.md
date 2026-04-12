# Herald

Realtime event transport with built-in authorization. Multi-tenant by default. Event body is opaque — Herald is a delivery layer, not a database.

## What It Does

Herald delivers events in real time over WebSocket with ordering and a short-lived catch-up buffer (default **7-day** retention) for reconnects. Browsers connect via WebSocket. Your backend manages streams and members via HTTP. Herald uses ShroudB's WAL storage internally so it survives restarts without needing an external database — **but it is not your application database**. See [Herald is not your database](#herald-is-not-your-database) below.

```
                    wss://herald.example.com/ws
Browser <-- WebSocket (/ws) -->  HERALD  <-- HTTP API --> Your Backend
                                   |
                           ShroudB engines (embedded)
                           +-- Sentry (authorization)
```

WebSocket and HTTP API share the same port. Behind any reverse proxy, TLS terminates at the proxy and Herald serves both protocols on one port.

### Create a stream and add members (HTTP)

```bash
curl -X POST http://localhost:6200/streams \
  -H "Authorization: Basic $(echo -n 'key:secret' | base64)" \
  -d '{"id": "general", "name": "General"}'

curl -X POST http://localhost:6200/streams/general/members \
  -H "Authorization: Basic $(echo -n 'key:secret' | base64)" \
  -d '{"user_id": "alice", "role": "owner"}'
```

### Connect, subscribe, publish (WebSocket)

```json
--> {"type": "subscribe", "payload": {"streams": ["general"]}}
<-- {"type": "subscribed", "payload": {"stream": "general", "members": [...], "cursor": 0, "latest_seq": 42}}

--> {"type": "event.publish", "ref": "r1", "payload": {"stream": "general", "body": "hello!"}}
<-- {"type": "event.ack", "ref": "r1", "payload": {"id": "evt_abc", "seq": 43}}
```

## Engine Architecture

Herald's core is a transport layer. Domain-specific features are provided by **engines** — isolated modules that register routes, lifecycle hooks, and background tasks.

| Image | Features | Use Case |
|-------|----------|----------|
| `ghcr.io/skeptik-io/herald` | Transport + Chat + Presence | Chat apps with rich presence (default) |
| `ghcr.io/skeptik-io/herald-transport` | Transport only | IoT, activity feeds, service-to-service events |
| `ghcr.io/skeptik-io/herald-chat` | Transport + Chat | Messaging with reactions, typing, read receipts — basic presence only |
| `ghcr.io/skeptik-io/herald-presence` | Transport + Presence | Rich presence without chat (creator platforms, team dashboards) |

### Transport (always included)

Publish, subscribe, catch-up on reconnect (within the retention window), delivery acknowledgment, ephemeral triggers, stream/member CRUD, multi-tenant auth, connection-derived presence (online/offline).

### Chat Engine

Event editing and deletion, emoji reactions, read cursors, typing indicators, user blocking with fanout filtering.

### Presence Engine

Manual presence overrides (away, dnd, appear offline), per-override expiry ("away until Monday 9am"), WAL-persisted overrides that survive restarts, `__presence` broadcast stream for global real-time presence, watchlist with override awareness, batch presence queries, admin presence API.

## Herald is not your database

Herald is a **transport layer**, in the same category as Pusher, Ably, or Centrifugo. It is **not** a message store, not a history API, and not your application's source of truth across time.

### Two authorities, one reconciliation

- **Inside the buffer window (default 7 days):** Herald is authoritative for *deltas* — new events, edits, deletes, reactions, cursor moves. A client that went offline briefly reconnects with its last-seen seq and receives every delta it missed via replay, including mutations to older messages. This is why those mutation events are persisted: without them, a 20-second wifi drop would silently lose a deletion that happened during the gap.
- **Across the buffer window (cold load, long absence, new device):** your app's database is authoritative for *state*. The client hydrates from your DB, then subscribes to Herald for further deltas.
- **Both paths must converge on the same end state.** They do if your webhook mirrors every Herald event — including chat mutations — into your DB as it happens.

### What that means in practice

- **Do not treat Herald as a queryable history API.** Events past the TTL are pruned hourly; requests for them return empty ranges silently. If you need long-term history, search, audit, or analytics, answer those queries from your own database.
- **Do not treat Herald as the source of truth for chat state.** Mutations are persisted in the WAL for replay correctness, not for durability. They vanish with the TTL just like events do. Mirror them to your DB if they need to last.
- **Integration shape:** browsers talk to Herald over WebSocket for realtime; your backend receives the webhook and writes to its own DB. On a cold load, clients hydrate history from your app's DB (via your own API), then subscribe to Herald for live updates. The chat SDK's `seedHistory` / `loadMoreWith(fetcher)` hooks are the seam for this.

If you're picking between "keep it in Herald" and "keep it in my DB" for anything past a reconnect window, the answer is **your DB**. Treat Herald as an expiring pipe that happens to remember the last few days so reconnects are seamless.

## Quick Start

### Docker

```bash
docker pull ghcr.io/skeptik-io/herald:latest

docker run -d \
  -p 6200:6200 \
  -e SHROUDB_MASTER_KEY="$(openssl rand -hex 32)" \
  -e HERALD_AUTH_PASSWORD="your-admin-password" \
  -v herald-data:/data \
  ghcr.io/skeptik-io/herald:latest
```

### From source

```bash
cargo build --release
SHROUDB_MASTER_KEY="$(openssl rand -hex 32)" ./target/release/herald
```

### Docker Compose

```yaml
services:
  herald:
    image: ghcr.io/skeptik-io/herald:latest
    ports:
      - "6200:6200"
    environment:
      SHROUDB_MASTER_KEY: "your-64-hex-char-master-key"
      HERALD_AUTH_PASSWORD: "your-admin-password"
    volumes:
      - herald-data:/data

volumes:
  herald-data:
```

## Architecture

- **Internal storage**: ShroudB WAL engine for the catch-up buffer and registry state. No external database required *to run Herald* — but your application still needs its own database for anything beyond the retention window (see [Herald is not your database](#herald-is-not-your-database)).
- **Multi-tenant**: Each tenant has isolated streams, members, and API keys. A default tenant is auto-created on first start.
- **Opaque event body**: Herald stores and delivers event bodies as-is. Consumers handle their own encryption and search.
- **HMAC-SHA256 auth**: Tenants use a key+secret model. WebSocket auth via signed query params. Admin API uses bearer token.
- **ShroudB Sentry**: ABAC authorization runs embedded by default. Remote mode available.
- **Circuit breakers**: Remote connections have timeout + circuit breaker + graceful degradation.
- **Latency histograms**: Per-stage Prometheus metrics at `/metrics`.

## Performance

| Throughput | p50 Latency |
|-----------|-------------|
| ~8,000 msg/s | 0.10ms |

## SDKs

Published to GitHub Packages on each release.

### WebSocket (client-side)

| Package | Description |
|---------|-------------|
| `@skeptik-io/herald-sdk` | Browser WebSocket client — events, reconnect, dedup, E2EE |
| `@skeptik-io/herald-chat-sdk` | Chat frame extensions — edit, delete, cursor, typing, reactions |
| `@skeptik-io/herald-presence-sdk` | Presence frame extensions — setPresence, clearOverride |
| `@skeptik-io/herald-chat` | Framework-agnostic chat state machine |
| `@skeptik-io/herald-chat-react` | React hooks and headless components |

### HTTP Admin (server-side, all codegen-managed)

| Package | Language |
|---------|----------|
| `@skeptik-io/herald-admin` | TypeScript |
| `herald-admin-go` | Go |
| `herald-admin` | Python |
| `herald-admin` | Ruby |
| `herald-admin-php` | PHP |
| `Herald.Admin` | C# |

## Ecosystem

- **ShroudB Sentry** — ABAC authorization (embedded or remote)
- **ShroudB Sigil** — standalone tenant user auth service (not embedded in Herald)
- **Meterd** — MAU tracking, quota enforcement, Stripe billing (proxy layer via Envoy ExtProc)

See [DOCS.md](DOCS.md) for configuration and API reference. See [ARCHITECTURE.md](ARCHITECTURE.md) for protocol specification.

## License

Proprietary. Copyright (c) 2026 Herald. All rights reserved. See [LICENSE](LICENSE) for details.
