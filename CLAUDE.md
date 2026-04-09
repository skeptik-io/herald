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

All must pass before any push.

## Rules

- **Herald is not a ShroudB engine.** Standalone project. Uses ShroudB crates (storage, store, engine crates) as dependencies.
- **No external database.** Storage is ShroudB WAL (`shroudb-storage`). Encrypted at rest. Master key from `SHROUDB_MASTER_KEY` env var.
- **Multi-tenant by default.** Every store key is `{tenant_id}/...`. Every registry uses `(tenant_id, stream_id)` composite keys. JWT must include `tenant` claim.
- **Event body is opaque.** Herald stores and delivers event bodies as-is. No server-side encryption, decryption, indexing, or search. Consumers handle their own encryption and search.
- **Sentry for authorization.** Embedded (in-process) or Remote (TCP with circuit breaker). Fail-open when circuit trips.
- **Single-tenant DX.** `--single-tenant` (default) auto-creates a `default` tenant from config-level `jwt_secret` and `api.tokens`. No admin API needed.
- **No unwrap on fallible ops in production.** Use `?`, `if let`, or `match`. Mocks in `integrations/mod.rs` may use `lock().unwrap()`.
- **Circuit breakers on all remote calls.** 5 failures → open, 30s cooldown. Sentry fail-open (permit when circuit trips).
- **Latency instrumented.** Event pipeline stages (store, fanout) have Prometheus histograms. Check `/metrics`.

## Test Structure

- `tests/integration.rs` — multi-tenant tests (WAL storage, no Postgres)
- `tests/bench_latency.rs` — WAL latency benchmark

Tests use `EphemeralKey` (random master key per test) and `tempfile` directories for isolation.

## SDK Structure

- `herald-sdk-typescript/` — Browser WebSocket client (event-driven, reconnect, dedup)
- `herald-admin-typescript/` — Node.js HTTP admin (namespaced: streams, members, events, presence)
- `herald-admin-go/` — Go HTTP admin (context-based)
- `herald-admin-python/` — Python HTTP admin (zero external deps)
- `herald-admin-ruby/` — Ruby HTTP admin (net/http)

## Ecosystem

- **ShroudB Sentry** — ABAC authorization (embedded or remote)
- **ShroudB Sigil** — standalone tenant user auth service (signup, login, JWT sessions). Not embedded in Herald — runs separately.
- **Meterd** (`/Users/nlucas/dev/skeptik/meterd/`) — MAU tracking, quota enforcement, Stripe billing. Runs at the proxy layer via Envoy ExtProc, not embedded in Herald.
