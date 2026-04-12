# Herald

> Realtime event transport with built-in authorization. Multi-tenant, ShroudB Sentry ABAC. Event body is opaque — Herald is a **delivery layer, not a database**. The ShroudB WAL backing it is a short-horizon catch-up buffer (default 7-day TTL), not an application message store. Consumers own the canonical log in their own database.

## Quick Context

- **Role**: Real-time event delivery. Browsers via WebSocket (`/ws`), backends via HTTP API. Single port (default `:6200`).
- **Not a ShroudB engine**: Standalone Rust project. Uses ShroudB crates for storage.
- **Internal storage**: ShroudB WAL engine (`shroudb-storage`), ~80µs writes. Backs the catch-up buffer and registry state only — not an app-facing message store. Default retention is 7 days (`store.event_ttl_days`); older events are pruned hourly. Apps still need their own database for history, audit, and search.
- **Multi-tenant**: Per-tenant key+secret (HMAC-SHA256). Admin API for tenant CRUD. Default tenant auto-created on first start.
- **Opaque event body**: Herald stores and delivers event bodies as-is. Consumers handle their own encryption and search.
- **ShroudB Sentry**: ABAC authorization available as embedded (in-process) or remote (TCP with circuit breakers).

## Workspace Layout

```
herald/                              7,441 lines Rust
├── herald-core/                     9 files — domain types (no I/O)
│   ├── auth.rs                      TokenClaims (HMAC-SHA256 key+secret)
│   ├── cursor.rs, member.rs         Cursor, Member, Role
│   ├── event.rs, stream.rs          Event, Stream
│   ├── presence.rs, error.rs        PresenceStatus, HeraldError, ErrorCode
│   └── protocol.rs                  ClientEvent, ServerEvent, all payloads
│
├── herald-server/                   39 files — the binary
│   ├── main.rs                      Startup, WAL init, TLS, shutdown
│   ├── config.rs                    HeraldConfig (TOML)
│   ├── state.rs                     AppState, Metrics (histograms), tenant cache, HMAC token validation
│   ├── latency.rs                   Prometheus histogram (atomic buckets, no external crate)
│   ├── webhook.rs                   Signed delivery with retries
│   │
│   ├── store/                       ShroudB Store trait — all async, tenant-scoped composite keys
│   │   ├── tenants.rs               Tenant CRUD + API token management
│   │   ├── streams.rs               key: {tenant}/{stream}
│   │   ├── members.rs               key: {tenant}/{stream}/{user}
│   │   ├── events.rs                key: {tenant}/{stream}/{seq:020} + secondary ID index
│   │   └── cursors.rs               key: {tenant}/{stream}/{user}
│   │
│   ├── registry/                    In-memory state (DashMap)
│   │   ├── connection.rs            ConnectionRegistry — per-tenant user tracking
│   │   ├── stream.rs                StreamRegistry — composite (tenant, stream) keys
│   │   └── presence.rs              PresenceTracker — connection-derived + manual override
│   │
│   ├── ws/                          WebSocket server (/ws on :6200)
│   │   ├── connection.rs            Rate limit, reconnect catch-up, presence linger
│   │   ├── handler.rs               Frame dispatch with per-stage latency instrumentation
│   │   ├── fanout.rs                Stream fan-out
│   │   └── upgrade.rs               HTTP → WS upgrade
│   │
│   ├── http/                        HTTP API (same port :6200)
│   │   ├── mod.rs                   Router + tenant/admin auth middleware
│   │   ├── admin.rs                 Tenant CRUD (key+secret auto-generated)
│   │   ├── streams.rs, members.rs   Stream/member CRUD
│   │   ├── events.rs                Inject, list
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

## Event Pipeline

```
Client sends event.publish
  → Rate limit check (per-connection sliding window)
  → Tenant + streams authorization check
  → Optional Sentry EVALUATE (embedded or remote)
  ─── Latency instrumented from here ───
  → WAL store (dual key: seq + ID index)                  [event_store histogram]
  → Metrics counter increment
  → event.ack to sender
  → Fan-out event.new to stream subscribers                [event_fanout histogram]
  ─── Total recorded ───                                   [event_total histogram]
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
- **Event body is opaque** — Herald stores and delivers bodies as-is. Consumers handle their own encryption and search.
- **Multi-tenant via composite keys** — `{tenant_id}/{stream_id}` in every namespace.
- **Circuit breakers on all remote calls** — 5 failures → open, 30s cooldown. Sentry fail-open.
- **Latency histograms per pipeline stage** — Prometheus-compatible, atomic buckets, no external crate.
- **Default tenant** — auto-created on first start. Zero admin overhead for single-tenant deployments.
