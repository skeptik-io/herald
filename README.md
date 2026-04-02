# Herald

Multi-tenant WebSocket chat server. Rooms, messages, presence, cursors, and real-time fan-out — with optional embedded encryption, search, and authorization via ShroudB.

## What It Does

Herald replaces Pusher/Soketi/Ably with a purpose-built chat server. Browsers connect via WebSocket for real-time messaging. Your backend manages rooms and members via HTTP. No external database required — Herald uses ShroudB's WAL-based storage engine for encrypted-at-rest persistence.

```
Browser ←— WebSocket :6200 —→  HERALD  ←— HTTP :6201 —→ Your Backend
                                  │
                          ShroudB engines (embedded or remote)
                          ├── Cipher — message encryption
                          ├── Veil — encrypted search
                          └── Sentry — authorization policies
```

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

- **Storage**: ShroudB WAL engine — encrypted at rest, ~80µs writes (vs ~580µs Postgres). No external database.
- **Multi-tenant**: Each tenant has isolated rooms, members, and JWT secrets. `--single-tenant` mode for self-hosted.
- **ShroudB engines**: Cipher, Veil, and Sentry run embedded (in-process) or remote (TCP). Embedded: 0.25ms encrypted messages. Remote: 1ms.
- **Circuit breakers**: All remote engine connections have timeout + circuit breaker + graceful degradation.
- **Latency histograms**: Per-stage Prometheus metrics (encrypt, store, index, fanout).

## Performance

| Mode | Throughput | p50 Latency |
|------|-----------|-------------|
| Plaintext | ~8,000 msg/s | 0.10ms |
| Encrypted (embedded Cipher + Veil) | ~4,000 msg/s | 0.25ms |
| Encrypted (remote Cipher + Veil) | ~1,400 msg/s | 1.00ms |

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
# Pull from GitHub Container Registry
docker pull ghcr.io/skeptik-io/herald:latest

# Run with config + persistent volume
docker run -d \
  -p 6200:6200 -p 6201:6201 \
  -e SHROUDB_MASTER_KEY="$(openssl rand -hex 32)" \
  -v ./herald.toml:/data/herald.toml:ro \
  -v herald-data:/data/herald-data \
  ghcr.io/skeptik-io/herald:latest
```

### Docker Compose

```yaml
services:
  herald:
    image: ghcr.io/skeptik-io/herald:latest
    ports:
      - "6200:6200"
      - "6201:6201"
    environment:
      SHROUDB_MASTER_KEY: "your-64-hex-char-master-key"
    volumes:
      - ./herald.toml:/data/herald.toml:ro
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

See [DOCS.md](DOCS.md) for complete reference. See [ARCHITECTURE.md](ARCHITECTURE.md) for protocol specification.
