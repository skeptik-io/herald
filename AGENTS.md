# Herald

> Multi-tenant WebSocket chat server with embedded ShroudB encryption, search, and authorization.

## Quick Context

- **Role**: Real-time messaging. Browsers via WebSocket (:6200), backends via HTTP (:6201).
- **Not a ShroudB engine**: Standalone Rust project. Uses ShroudB crates for storage + crypto.
- **Storage**: ShroudB WAL engine (`shroudb-storage`). Encrypted at rest. ~80µs writes. No Postgres.
- **Multi-tenant**: `tenant` JWT claim. Per-tenant rooms, members, JWT secrets. Admin API for tenant CRUD.
- **ShroudB engines**: Cipher/Veil/Sentry available as embedded (in-process) or remote (TCP with circuit breakers).

## Workspace Layout

```
herald/                              7,441 lines Rust
├── herald-core/                     9 files — domain types (no I/O)
│   ├── auth.rs                      JwtClaims (with tenant claim)
│   ├── cursor.rs, member.rs         Cursor, Member, Role
│   ├── message.rs, room.rs          Message, Room, EncryptionMode
│   ├── presence.rs, error.rs        PresenceStatus, HeraldError, ErrorCode
│   └── protocol.rs                  ClientMessage, ServerMessage, all payloads
│
├── herald-server/                   39 files — the binary
│   ├── main.rs                      Startup, --single/multi-tenant, WAL init, TLS, shutdown
│   ├── config.rs                    HeraldConfig (TOML)
│   ├── state.rs                     AppState, Metrics (histograms), tenant cache, JWT validation
│   ├── latency.rs                   Prometheus histogram (atomic buckets, no external crate)
│   ├── webhook.rs                   Signed delivery with retries
│   │
│   ├── store/                       ShroudB Store trait — all async, tenant-scoped composite keys
│   │   ├── tenants.rs               Tenant CRUD + API token management
│   │   ├── rooms.rs                 key: {tenant}/{room}
│   │   ├── members.rs               key: {tenant}/{room}/{user}
│   │   ├── messages.rs              key: {tenant}/{room}/{seq:020} + secondary ID index
│   │   └── cursors.rs               key: {tenant}/{room}/{user}
│   │
│   ├── registry/                    In-memory state (DashMap)
│   │   ├── connection.rs            ConnectionRegistry — per-tenant user tracking
│   │   ├── room.rs                  RoomRegistry — composite (tenant, room) keys
│   │   └── presence.rs              PresenceTracker — connection-derived + manual override
│   │
│   ├── ws/                          WebSocket server (:6200)
│   │   ├── connection.rs            Auth, rate limit, reconnect catch-up, presence linger
│   │   ├── handler.rs               Frame dispatch with per-stage latency instrumentation
│   │   ├── fanout.rs                Room fan-out
│   │   └── upgrade.rs               HTTP → WS upgrade
│   │
│   ├── http/                        HTTP API (:6201)
│   │   ├── mod.rs                   Router + tenant/admin auth middleware
│   │   ├── admin.rs                 Tenant CRUD + API token management
│   │   ├── rooms.rs, members.rs     Room/member CRUD
│   │   ├── messages.rs              Inject, list, search
│   │   ├── presence.rs              Presence queries
│   │   └── health.rs                /health + /metrics (Prometheus histograms)
│   │
│   ├── integrations/                ShroudB engine wrappers
│   │   ├── mod.rs                   Traits + mocks: CipherOps, VeilOps, SentryOps, CourierOps, ChronicleOps
│   │   ├── cipher.rs                RemoteCipherOps (TCP client)
│   │   ├── embedded_cipher.rs       EmbeddedCipherOps (in-process CipherEngine)
│   │   ├── veil.rs                  RemoteVeilOps (TCP client)
│   │   ├── embedded_veil.rs         EmbeddedVeilOps (in-process VeilEngine)
│   │   ├── sentry.rs                RemoteSentryOps (TCP client)
│   │   ├── embedded_sentry.rs       EmbeddedSentryOps (in-process SentryEngine)
│   │   ├── courier.rs               RemoteCourierOps
│   │   ├── chronicle.rs             RemoteChronicleOps
│   │   ├── resilient.rs             Circuit breaker wrappers (timeout + backoff)
│   │   └── circuit_breaker.rs       State machine: Closed → Open → HalfOpen
│   │
│   └── tests/
│       ├── integration.rs           6 multi-tenant tests (WAL, no Postgres)
│       └── bench_latency.rs         3 benchmarks: plaintext, encrypted remote, encrypted embedded
│
├── herald-sdk-typescript/           Browser WebSocket client
├── herald-admin-typescript/         Node.js HTTP admin
├── herald-admin-go/                 Go HTTP admin
├── herald-admin-python/             Python HTTP admin
├── herald-admin-ruby/               Ruby HTTP admin
│
├── ARCHITECTURE.md                  Full protocol specification
├── DOCS.md                          Complete reference
├── Dockerfile                       Multi-stage production build
└── docker-compose.yml               Herald + optional ShroudB Moat
```

## Commands

```bash
cargo build --release
cargo test --workspace                  # Integration tests (WAL, no Postgres)
cargo test --test bench_latency -- --nocapture --test-threads=1  # Latency benchmarks
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

## Message Pipeline

```
Client sends message.send
  → Rate limit check (per-connection sliding window)
  → JWT tenant + rooms claim check
  → Optional Sentry EVALUATE (embedded or remote)
  ─── Latency instrumented from here ───
  → Cipher ENCRYPT body (if server_encrypted room)       [message_encrypt histogram]
  → WAL store (dual key: seq + ID index)                  [message_store histogram]
  → Veil PUT blind index (if configured)                  [message_index histogram]
  → Metrics counter increment
  → message.ack to sender
  → Fan-out message.new to room subscribers                [message_fanout histogram]
  ─── Total recorded ───                                   [message_total histogram]
  → Webhook POST (signed, async)
  → Chronicle INGEST audit event (async)
  → Courier NOTIFY offline members (async)
```

## Integration Architecture

All ShroudB engines use the same trait pattern:

```
Trait (CipherOps)
├── MockCipherOps          — base64 encode/decode (tests)
├── RemoteCipherOps        — TCP client (shroudb-cipher-client)
│   └── ResilientCipher    — circuit breaker + 10s timeout wrapper
└── EmbeddedCipherOps      — in-process CipherEngine (shroudb-cipher-engine)
```

Config chooses mode: `[shroudb]` section → remote. No section → embedded or disabled.

## Key Design Decisions

- **ShroudB WAL replaces Postgres** — ~80µs writes vs ~580µs. Single binary, no external database.
- **Multi-tenant via composite keys** — `{tenant_id}/{room_id}` in every namespace. Crypto isolation via HKDF.
- **Embedded engines eliminate TCP overhead** — Cipher encrypt drops from 0.3ms to 0.02ms.
- **Circuit breakers on all remote calls** — 5 failures → open, 30s cooldown. Sentry fail-open.
- **Latency histograms per pipeline stage** — Prometheus-compatible, atomic buckets, no external crate.
- **Single-tenant mode** — `--single-tenant` bootstraps a `default` tenant from config. Zero admin overhead.
