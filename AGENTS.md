# Herald

> Multi-tenant WebSocket chat server with ShroudB Sentry authorization. Message body is opaque — Herald is a transport+storage layer.

## Quick Context

- **Role**: Real-time messaging. Browsers via WebSocket (:6200), backends via HTTP (:6201).
- **Not a ShroudB engine**: Standalone Rust project. Uses ShroudB crates for storage.
- **Storage**: ShroudB WAL engine (`shroudb-storage`). ~80µs writes. No Postgres.
- **Multi-tenant**: `tenant` JWT claim. Per-tenant rooms, members, JWT secrets. Admin API for tenant CRUD.
- **Opaque message body**: Herald stores and delivers message bodies as-is. Consumers handle their own encryption and search.
- **ShroudB Sentry**: ABAC authorization available as embedded (in-process) or remote (TCP with circuit breakers).

## Workspace Layout

```
herald/                              7,441 lines Rust
├── herald-core/                     9 files — domain types (no I/O)
│   ├── auth.rs                      JwtClaims (with tenant claim)
│   ├── cursor.rs, member.rs         Cursor, Member, Role
│   ├── message.rs, room.rs          Message, Room
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
│   │   ├── messages.rs              Inject, list
│   │   ├── presence.rs              Presence queries
│   │   └── health.rs                /health + /metrics (Prometheus histograms)
│   │
│   ├── integrations/                ShroudB engine wrappers
│   │   ├── mod.rs                   Traits + mocks: SentryOps, CourierOps, ChronicleOps
│   │   ├── sentry.rs                RemoteSentryOps (TCP client)
│   │   ├── embedded_sentry.rs       EmbeddedSentryOps (in-process SentryEngine)
│   │   ├── courier.rs               RemoteCourierOps
│   │   ├── chronicle.rs             RemoteChronicleOps
│   │   ├── resilient.rs             Circuit breaker wrappers (timeout + backoff)
│   │   └── circuit_breaker.rs       State machine: Closed → Open → HalfOpen
│   │
│   └── tests/
│       ├── integration.rs           6 multi-tenant tests (WAL, no Postgres)
│       └── bench_latency.rs         WAL latency benchmark
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
  → WAL store (dual key: seq + ID index)                  [message_store histogram]
  → Metrics counter increment
  → message.ack to sender
  → Fan-out message.new to room subscribers                [message_fanout histogram]
  ─── Total recorded ───                                   [message_total histogram]
  → Webhook POST (signed, async)
  → Chronicle INGEST audit event (async)
  → Courier NOTIFY offline members (async)
```

## Integration Architecture

ShroudB Sentry uses a trait pattern:

```
Trait (SentryOps)
├── MockSentryOps          — allow-all (tests)
├── RemoteSentryOps        — TCP client (shroudb-sentry-client)
│   └── ResilientSentry    — circuit breaker + 10s timeout wrapper
└── EmbeddedSentryOps      — in-process SentryEngine (shroudb-sentry-engine)
```

Config chooses mode: `[shroudb]` section → remote. No section → embedded or disabled.

## Key Design Decisions

- **ShroudB WAL replaces Postgres** — ~80µs writes vs ~580µs. Single binary, no external database.
- **Message body is opaque** — Herald stores and delivers bodies as-is. Consumers handle their own encryption and search.
- **Multi-tenant via composite keys** — `{tenant_id}/{room_id}` in every namespace.
- **Circuit breakers on all remote calls** — 5 failures → open, 30s cooldown. Sentry fail-open.
- **Latency histograms per pipeline stage** — Prometheus-compatible, atomic buckets, no external crate.
- **Single-tenant mode** — `--single-tenant` bootstraps a `default` tenant from config. Zero admin overhead.
