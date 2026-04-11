# Herald

Persistent realtime event streams with built-in authorization. Multi-tenant by default. Event body is opaque — Herald is a transport and delivery layer.

## What It Does

Herald delivers events in real time over WebSocket with persistence, ordering, and replay. Browsers connect via WebSocket. Your backend manages streams and members via HTTP. No external database — Herald uses ShroudB's WAL-based storage engine.

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

Publish, subscribe, replay, delivery acknowledgment, ephemeral triggers, stream/member CRUD, multi-tenant auth, connection-derived presence (online/offline).

### Chat Engine

Event editing and deletion, emoji reactions, read cursors, typing indicators, user blocking with fanout filtering.

### Presence Engine

Manual presence overrides (away, dnd, appear offline), per-override expiry ("away until Monday 9am"), WAL-persisted overrides that survive restarts, `__presence` broadcast stream for global real-time presence, watchlist with override awareness, batch presence queries, admin presence API.

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

- **Storage**: ShroudB WAL engine. No external database.
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
