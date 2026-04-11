# Herald

Persistent realtime event streams with built-in authorization. Standalone Rust project using ShroudB for storage and authorization. Event body is opaque — Herald is a transport and delivery layer.

## Read Order

1. `AGENTS.md` — workspace layout, message pipeline, integration architecture
2. `ARCHITECTURE.md` — full protocol specification
3. `DOCS.md` — configuration reference, API reference

## Commands

```bash
cargo build --release
cargo test --workspace
cargo test --test bench_latency -- --nocapture --test-threads=1
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

All must pass before any push. Also run `cargo build --no-default-features` and `cargo clippy --no-default-features -- -D warnings` — both must be clean. Fix any issue encountered during checks, even if pre-existing.

### Feature matrix

Herald uses compile-time feature flags to produce different server binaries. All combinations must build and pass clippy:

```bash
cargo build --no-default-features                           # transport only
cargo build --no-default-features --features chat           # transport + chat
cargo build --no-default-features --features presence       # transport + presence
cargo build --release                                       # default = chat + presence
```

## Engine Architecture

Herald uses a Moat-style engine composition system. The core transport (streams, events, subscribe, replay, auth) is always present. Domain-specific features are provided by **engines** that register via the `HeraldEngine` trait.

### Engines

| Engine | Feature Flag | Provides |
|--------|-------------|----------|
| **Chat** | `chat` | Event edit/delete, reactions, read cursors, typing indicators, user blocks, block-based fanout filtering |
| **Presence** | `presence` | Manual presence overrides (away/dnd/offline), per-override expiry ("until Monday 9am"), watchlist with override awareness, `__presence` broadcast stream, batch presence queries, admin presence API, WAL-persisted overrides |

### Extension points (`HeraldEngine` trait)

- `tenant_routes()` / `admin_routes()` — HTTP route registration
- `on_connect()` / `on_disconnect()` / `on_last_disconnect()` — connection lifecycle hooks
- `fanout_filter()` — per-subscriber delivery filtering (e.g. block check)
- `namespaces()` — WAL namespace registration
- `timers()` — background tasks (typing TTL expiry, presence override sweep)

### Key files

- `herald-server/src/engine.rs` — `HeraldEngine` trait, `EngineSet`, `TimerTask`
- `herald-server/src/engines/mod.rs` — engine registration, `default_engines()`
- `herald-server/src/engines/chat/` — chat engine (ws_handler, http_events, http_blocks)
- `herald-server/src/engines/presence/` — presence engine (ws_handler, http, mod)

### Docker images

Built from a single Dockerfile with `HERALD_FEATURES` build arg. See `docker-bake.hcl`:

- `herald/herald` — transport only (no features)
- `herald/chat` — transport + chat
- `herald/presence` — transport + presence
- `herald/social` — transport + chat + presence (default)

## Rules

- **Herald is not a ShroudB engine.** Standalone project. Uses ShroudB crates (storage, store, engine crates) as dependencies.
- **No external database.** Storage is ShroudB WAL (`shroudb-storage`). Encrypted at rest. Master key from `SHROUDB_MASTER_KEY` env var.
- **Multi-tenant by default.** Every store key is `{tenant_id}/...`. Every registry uses `(tenant_id, stream_id)` composite keys. A default tenant is auto-created on first start.
- **Event body is opaque.** Herald stores and delivers event bodies as-is. No server-side encryption, decryption, indexing, or search. Consumers handle their own encryption and search.
- **Sentry for authorization.** Embedded (in-process) or Remote (TCP with circuit breaker). Fail-open when circuit trips.
- **HMAC-SHA256 token auth.** No JWT. Tenants use a key+secret model. WebSocket auth via query params (`/ws?key=...&token=...&user_id=...&streams=...`). Admin API uses `auth.password` as bearer token.
- **No unwrap on fallible ops in production.** Use `?`, `if let`, or `match`. Mocks in `integrations/mod.rs` may use `lock().unwrap()`.
- **Circuit breakers on all remote calls.** 5 failures -> open, 30s cooldown. Sentry fail-open (permit when circuit trips).
- **Latency instrumented.** Event pipeline stages (store, fanout) have Prometheus histograms. Check `/metrics`.

## Presence

Herald owns presence as a first-class subsystem (not just connection-derived online/offline).

- **Manual overrides:** Users can set away, dnd, or offline (appear offline while connected)
- **Per-override expiry:** `presence.set { status: "dnd", until: "2026-04-14T09:00:00Z" }` — reverts automatically
- **WAL persistence:** Manual overrides survive server restarts
- **`__presence` stream:** Per-tenant broadcast stream for global real-time presence. Clients subscribe to `__presence` for tenant-wide presence without per-user watchlists.
- **Watchlist awareness:** Watchlist notifications respect manual overrides (a user set to "offline" won't trigger `watchlist.online`)
- **Batch queries:** `GET /presence?user_ids=a,b,c` for bulk presence lookup
- **Admin API:** `POST /presence/{user_id}` for server-side presence overrides

## Test Structure

- `tests/integration.rs` — multi-tenant tests (WAL storage, no Postgres)
- `tests/bench_latency.rs` — WAL latency benchmark
- `test-integration/test-live.ts` — live SDK integration tests (starts real server)
- `contract-tests/` — cross-SDK contract validation against OpenAPI spec

Tests use `EphemeralKey` (random master key per test) and `tempfile` directories for isolation.

## SDK Structure

### WebSocket SDKs (client-side)

- `herald-sdk-typescript/` — Browser WebSocket client (event-driven, reconnect, dedup, E2EE)
- `herald-chat-sdk-typescript/` — Chat frame extensions (edit, delete, cursor, typing, reactions)
- `herald-presence-sdk-typescript/` — Presence frame extensions (setPresence, clearOverride)
- `herald-chat-core/` — Framework-agnostic chat state machine (messages, members, presence, typing, cursors)
- `herald-chat-react/` — React hooks and headless components

### HTTP Admin SDKs (server-side, codegen-managed)

- `herald-admin-typescript/` — Node.js
- `herald-admin-go/` — Go
- `herald-admin-python/` — Python (zero external deps)
- `herald-admin-ruby/` — Ruby (net/http)
- `herald-admin-php/` — PHP
- `herald-admin-csharp/` — C#

Admin SDKs are generated by `codegen/generate.ts` from the OpenAPI spec. Do not edit generated files directly — update the emitter templates in `codegen/emitters/`.

## Ecosystem

- **ShroudB Sentry** — ABAC authorization (embedded or remote)
- **ShroudB Sigil** — standalone tenant user auth service (signup, login, sessions). Not embedded in Herald — runs separately.
- **Meterd** (`/Users/nlucas/dev/skeptik/meterd/`) — MAU tracking, quota enforcement, Stripe billing. Runs at the proxy layer via Envoy ExtProc, not embedded in Herald.
