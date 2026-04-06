# Herald

Multi-tenant WebSocket chat server. Rooms, messages, presence, cursors, and real-time fan-out — with ABAC authorization via ShroudB Sentry. Message body is opaque; consumers handle their own encryption and search.

## What It Does

Herald replaces Pusher/Soketi/Ably with a purpose-built chat server. Browsers connect via WebSocket for real-time messaging. Your backend manages rooms and members via HTTP. No external database required — Herald uses ShroudB's WAL-based storage engine for persistence. Message bodies are treated as opaque bytes — Herald stores and delivers them as-is.

```
                    wss://herald.example.com/ws
Browser ←— WebSocket (/ws) ——→  HERALD  ←—— HTTP API ——→ Your Backend
                                   │
                           ShroudB engines (embedded)
                           └── Sentry — authorization policies
```

WebSocket and HTTP API share the same port. Behind any reverse proxy (nginx, Caddy, cloud LB), TLS terminates at the proxy and Herald serves both protocols on one port. For direct TCP deployments, a standalone WebSocket port is also available.

### Create a room and add members (HTTP)

```bash
curl -X POST http://localhost:6201/rooms \
  -H "Authorization: Bearer $API_TOKEN" \
  -d '{"id": "general", "name": "General Chat"}'

curl -X POST http://localhost:6201/rooms/general/members \
  -H "Authorization: Bearer $API_TOKEN" \
  -d '{"user_id": "alice", "role": "owner"}'
```

### Connect, subscribe, send messages (WebSocket)

```json
→ {"type": "auth", "payload": {"token": "<jwt with tenant claim>"}}
← {"type": "auth_ok", "payload": {"user_id": "alice"}}

→ {"type": "subscribe", "payload": {"rooms": ["general"]}}
← {"type": "subscribed", "payload": {"room": "general", "members": [...], "cursor": 0, "latest_seq": 42}}

→ {"type": "message.send", "ref": "m1", "payload": {"room": "general", "body": "hello!"}}
← {"type": "message.ack", "ref": "m1", "payload": {"id": "msg_abc", "seq": 43}}
```

## Architecture

- **Storage**: ShroudB WAL engine — ~80µs writes (vs ~580µs Postgres). No external database.
- **Multi-tenant**: Each tenant has isolated rooms, members, and JWT secrets. `--single-tenant` mode for self-hosted.
- **Opaque message body**: Herald stores and delivers message bodies as-is. Consumers handle their own encryption and search.
- **ShroudB Sentry**: ABAC authorization runs **embedded by default** — no config needed. Remote mode available via `[shroudb]` config.
- **Circuit breakers**: Remote Sentry connections have timeout + circuit breaker + graceful degradation.
- **Latency histograms**: Per-stage Prometheus metrics (store, fanout).

## Performance

| Throughput | p50 Latency |
|-----------|-------------|
| ~8,000 msg/s | 0.10ms |

## Quick Start

### From source

```bash
cargo build --release
cp herald.toml.example herald.toml
# Edit: set auth.jwt_secret, auth.super_admin_token

SHROUDB_MASTER_KEY="$(openssl rand -hex 32)" ./target/release/herald herald.toml
```

### Docker

```bash
docker pull ghcr.io/skeptik-io/herald:latest

# Single port — HTTP API + WebSocket (/ws) on 6201
docker run -d \
  -p 6201:6201 \
  -e SHROUDB_MASTER_KEY="$(openssl rand -hex 32)" \
  -e HERALD_JWT_SECRET="your-secret" \
  -e HERALD_SUPER_ADMIN_TOKEN="your-admin-token" \
  -e HERALD_API_TOKENS="your-api-token" \
  -v herald-data:/data/herald-data \
  ghcr.io/skeptik-io/herald:latest
```

Behind a reverse proxy (nginx, Caddy, Traefik), expose port 6201 and route `/ws` for WebSocket upgrades. No TLS config needed in Herald — the proxy handles it.

### Docker Compose

```yaml
services:
  herald:
    image: ghcr.io/skeptik-io/herald:latest
    ports:
      - "6201:6201"
    environment:
      SHROUDB_MASTER_KEY: "your-64-hex-char-master-key"
      HERALD_JWT_SECRET: "your-secret"
      HERALD_SUPER_ADMIN_TOKEN: "your-admin-token"
      HERALD_API_TOKENS: "your-api-token"
    volumes:
      - herald-data:/data/herald-data

volumes:
  herald-data:
```

## Multi-Tenancy

Single-tenant (`--single-tenant`, default): auto-creates a `default` tenant from config. Multi-tenant (`--multi-tenant`): tenants managed via admin API.

JWTs must include a `tenant` claim: `{"sub": "alice", "tenant": "acme", "rooms": ["general"], ...}`

## SDKs

Published to GitHub Packages on each release.

| Package | Language | Install |
|---|---|---|
| `@skeptik-io/herald-sdk` | TypeScript | `npm install @skeptik-io/herald-sdk` |
| `@skeptik-io/herald-admin` | TypeScript | `npm install @skeptik-io/herald-admin` |
| `herald-admin-go` | Go | `go get github.com/skeptik-io/herald/herald-admin-go` |
| `herald-admin` | Python | Release artifact |
| `herald-admin` | Ruby | `gem install herald-admin --source https://rubygems.pkg.github.com/skeptik-io` |

## Ecosystem

- **ShroudB Sigil** — tenant user authentication
- **ShroudB Sentry** — ABAC authorization (embedded or remote)
- **Meterd** — MAU tracking, quota enforcement, Stripe billing

Herald treats message bodies as opaque bytes. If you need encryption, encrypt before sending. If you need search, index in your own backend via webhooks.

See [DOCS.md](DOCS.md) for complete reference. See [ARCHITECTURE.md](ARCHITECTURE.md) for protocol specification.

## License

Proprietary. Copyright (c) 2026 Herald. All rights reserved. See [LICENSE](LICENSE) for details.
