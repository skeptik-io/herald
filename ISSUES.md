# Herald — Issue Tracker

Work items derived from market analysis and current codebase state. Each item must be fully implemented **and** covered by integration/sanity tests before checking off.

---

## HIGH — Blocking adoption or revenue

- [x] **H-1: Add LICENSE file**
  Choose and add a license (MIT, Apache-2.0, BSL, or proprietary). Without one, the code is "all rights reserved" and legally blocks all external use.
  - [x] LICENSE file added to repo root
  - [x] License field added to Cargo.toml and package.json files
  - [x] README updated with license badge/section

- [x] **H-2: Ship chat SDK layer (`herald-chat-*`)**
  Three new packages: `herald-chat-core` (framework-agnostic stores, scroll, liveness, notifier), `herald-chat-sdk-typescript` (thin client wrapper), `herald-chat-react` (provider, hooks, components).
  - [x] `herald-chat-core` — stores (messages, streams, presence, typing) finalized
  - [x] `herald-chat-core` — scroll manager (infinite scroll, anchor preservation)
  - [x] `herald-chat-core` — liveness detection (online/offline, reconnect triggers)
  - [x] `herald-chat-core` — notifier (browser notifications, unread counts)
  - [x] `herald-chat-core` — unit tests passing
  - [x] `herald-chat-sdk-typescript` — client wrapping herald-sdk-typescript with chat semantics
  - [x] `herald-chat-sdk-typescript` — unit tests passing
  - [x] `herald-chat-react` — ChatProvider context + hooks (useMessages, usePresence, useTyping, etc.)
  - [x] `herald-chat-react` — component library (MessageList, MessageInput, PresenceIndicator, TypingIndicator)
  - [x] `herald-chat-react` — unit tests passing
  - [x] Integration test: full chat flow (connect, send, receive, scroll, presence) against live server

- [x] **H-3: Ship chat server features (`herald-server/src/chat/`)**
  Server-side modules for blocks, reactions, extended cursors, presence HTTP, and WS handler extensions.
  - [x] `chat/http_blocks.rs` — block/unblock HTTP endpoints working
  - [x] `chat/http_events_ext.rs` — reactions, edit, delete via HTTP
  - [x] `chat/http_presence.rs` — presence query endpoints
  - [x] `chat/store_blocks.rs` — block storage and lookups
  - [x] `chat/store_cursors.rs` — extended cursor operations
  - [x] `chat/store_reactions.rs` — reaction storage and aggregation
  - [x] `chat/ws_handler.rs` — WS frame handling for chat-specific operations
  - [x] `chat/mod.rs` — module wiring and route registration
  - [x] Integration tests: blocks, reactions, cursors, presence against live server
  - [x] `cargo clippy --workspace --all-targets -- -D warnings` clean
  - [x] `cargo test --workspace` passing

- [x] **H-4: Remote store backend (`store_backend.rs`)**
  `StoreBackend` enum wrapping `EmbeddedStore` and `RemoteStore` behind the `Store` trait. Required for shared/managed deployments.
  - [x] `StoreBackend` implements full `Store` trait (get, put, delete, range, subscribe)
  - [x] `BackendSubscription` handles both embedded and remote subscription events
  - [x] Config flag to select embedded vs remote backend
  - [x] Health endpoint reflects backend type
  - [x] Integration test: server boots and operates with embedded backend
  - [x] Integration test: server boots and operates with remote backend (Docker ShroudB, gated by HERALD_TEST_REMOTE_STORE)
  - [x] `cargo test --workspace` passing

- [x] **H-5: Land WS handler refactor**
  Large rewrite of `ws/handler.rs` (net ~0 LOC but significant restructure). Must be clean before other server work builds on it.
  - [x] Refactored handler compiles and passes `cargo clippy`
  - [x] All existing integration tests pass (`cargo test --workspace`)
  - [x] Bench latency test shows no regression (`cargo test --test bench_latency -- --nocapture --test-threads=1`)

- [x] **H-6: Integration test suite (`test-integration/`)**
  Live integration tests running against a real Herald server. Addresses the "zero integration tests" liability.
  - [x] Test harness: server startup/teardown, JWT minting, WebSocket + HTTP client helpers
  - [x] Test: tenant CRUD (create, get, list, update, delete)
  - [x] Test: stream CRUD with membership
  - [x] Test: event publish via WS, receive on subscriber, verify persistence via HTTP
  - [x] Test: event edit and delete (WS + HTTP)
  - [x] Test: reconnect catchup (`last_seen_at` → receive missed events)
  - [x] Test: presence set/get, linger window (user goes offline after delay)
  - [x] Test: typing start/stop with TTL expiry
  - [x] Test: read cursor update and query
  - [x] Test: reactions add/remove/aggregate
  - [x] Test: user blocking (blocked user's events filtered)
  - [x] Test: webhook delivery with HMAC verification
  - [x] Test: rate limiting (HTTP 429 after burst) — documented: not enforced on health in single-tenant
  - [x] Test: JWT auth failure (invalid token, expired token, wrong tenant)
  - [x] Test: multi-tenant isolation (tenant A cannot see tenant B's streams/events)
  - [x] Test: ephemeral event trigger (received but not persisted)
  - [x] CI wiring: tests run in CI pipeline

- [x] **H-7: Extract metering and signup from Herald** `session:next`
  Herald is a transport server. Billing (Meterd), signup (Sigil), and plan enforcement are infrastructure/gateway concerns that were incorrectly embedded in the application. Meterd already has an Envoy ExtProc (`skeptik/meterd/envoy-extproc`) that handles quota checks and usage tracking at the proxy layer. Sigil is a standalone auth service that should remain standalone.
  - [x] Remove `metering.rs` — ~644 LOC (MeteringClient, quota checks, circuit breaker, flush loop)
  - [x] Remove `signup.rs` — ~515 LOC (SigilHttpClient, signup/login/refresh handlers)
  - [x] Remove from `state.rs`: `metering`, `sigil`, `signup_rate_limits`, `plan_limits_cache` fields; `get_plan_limits()`, `get_plan_limits_cached()`, `event_ttl_ms()` methods
  - [x] Remove from `AppStateBuilder`: `metering`, `sigil` fields
  - [x] Remove from `main.rs`: Meterd client init, Sigil client init, metering flush task, peak connection tracking to Meterd
  - [x] Remove from `ws/handler.rs`: `metering.check_quota()` and `metering.track()` calls in `handle_publish`
  - [x] Remove from `http/events.rs`: `metering.check_quota()` and `metering.track()` calls in `inject_event`
  - [x] Remove from `http/streams.rs`: `get_plan_limits_cached()` call in stream creation
  - [x] Remove from `http/mod.rs`: signup routes (`/signup`, `/login`, `/auth/refresh`), `signup_rate_limit_middleware`, plan-limit-based rate limiting in tenant middleware
  - [x] Remove from `http/admin.rs`: `plan_limits_cache.remove()` in tenant delete
  - [x] Remove from `webhook.rs`: `metering.track("webhooks_sent", ...)` call
  - [x] Remove from `ws/connection.rs`: `get_plan_limits_cached()` for connection limit check — revert to config-only `tenant_limits.max_connections_per_tenant`
  - [x] Remove `MeteringConfig` and `PlanLimits` from `config.rs`
  - [x] Remove `lib.rs` module declarations for `metering` and `signup`
  - [x] Add `per_tenant_event_ttl_days: Option<u32>` to tenant config (stored in DB, set via admin API) — replaces Meterd-sourced retention
  - [x] `event_ttl_ms()` reads from tenant config field, falls back to global `store.event_ttl_days`
  - [x] Connection limits and rate limits remain config-driven (transport self-defense)
  - [x] `cargo build`, `cargo clippy`, `cargo test --workspace` clean
  - [x] `cargo build --no-default-features` clean
  - [x] Live integration tests pass (remove signup/metering test cases)
  - [x] Update ARCHITECTURE.md / DOCS.md: document that metering is handled at proxy layer (Envoy + Meterd ExtProc), signup is a separate service (Sigil)

---

## Session Plan (remaining work)

| Session | Items | Scope |
|---------|-------|-------|
| ~~`extract-infra`~~ | ~~H-7~~ | ~~Rip metering + signup out of Herald, revert to transport-only~~ **(done)** |
| `e2ee-sdks` | M-3 | E2EE in Go, Python, Ruby + cross-SDK interop tests |
| `moat-clients` | D-1 | shroudb crate updates for Moat prefix routing + integration tests |
| `audit-log` | D-2 | Chronicle rip-out, skeptik-audit-log integration, new API endpoints, 4 admin SDKs |
| ~~`clustering`~~ | ~~L-1~~ | ~~Pub/sub backplane for cross-instance fanout (storage already shared via remote store)~~ **(done)** |
| `mobile-swift` | L-2 (Swift) | Swift WebSocket client SDK |
| `mobile-kotlin` | L-2 (Kotlin) | Kotlin WebSocket client SDK |
| `mobile-dart` | L-2 (Dart) | Dart/Flutter WebSocket client SDK |
| `admin-sdks-2` | L-3 | PHP + .NET admin SDKs together |
| `at-least-once` | L-4 | Fanout layer redesign for delivery guarantees |
| `contract-tests` | L-6 | Define spec + wire all 4 existing SDKs |

---

## MEDIUM — Competitive differentiation & monetization readiness

- [x] **M-1: Catchup pagination**
  200-event catchup cap with `has_more` but no follow-up mechanism. Clients can lose events.
  - [x] Add cursor-based pagination to reconnect catchup (WS `EventsFetch` with `after` cursor)
  - [x] Client SDK: automatic paginated catchup on reconnect until fully caught up
  - [x] Integration test: publish 250 events, paginate with fetch({after}), verify all received
  - [x] Integration test: verify ordering preserved across pages

- [x] **M-2: OpenTelemetry / distributed tracing**
  Replace `X-Request-Id` with proper trace propagation for enterprise observability.
  - [x] Add `tracing-opentelemetry` crate with OTLP exporter (opt-in `otel` feature)
  - [x] Propagate W3C `traceparent` header on HTTP requests
  - [x] Inject trace context into WebSocket frame processing (debug event per frame)
  - [x] Integration test: verify traceparent present and propagated
  - [x] `/metrics` endpoint continues to work alongside OTLP export

- [x] **M-3: E2EE in non-TypeScript SDKs** `session:e2ee-sdks`
  E2EE is browser-only. Expand to server-side SDKs.
  - [x] E2EE module for Go admin SDK (x25519 + AES-256-GCM)
  - [x] E2EE module for Python admin SDK
  - [x] E2EE module for Ruby admin SDK
  - [x] Integration test: encrypt in Go, decrypt in TypeScript (cross-SDK interop)
  - [x] Integration test: blind token search works across SDK implementations

- [x] **M-4: Per-tenant retention tiers**
  TTL is global 7-day. Enable per-tenant configuration for tiered pricing.
  - [x] Event insert uses per-tenant TTL via `state.event_ttl_ms(tenant_id)`
  - [x] TTL cleanup job already respects per-event expiry timestamps
  - [x] Add `event_ttl_days` field to tenant record (stored in DB, set via admin API) — replaces Meterd-sourced retention after H-7
  - [x] Admin API: `PATCH /admin/tenants/{id}` accepts `event_ttl_days`
  - [x] Integration test: tenant A (7d) events expire, tenant B (30d) events survive
  - [x] Admin SDK support in all 4 SDKs

---

## DEFERRED — Blocked on external changes

- [ ] **D-1: Sentry/Courier integration tests against Moat** `session:moat-clients` *(blocked on shroudb crate changes)*
  Herald's TCP clients (SentryClient, CourierClient) send raw engine commands. Moat's RESP3 port expects engine-prefixed commands (e.g., `SENTRY EVALUATE ...`). TCP clients need updates in shroudb crates to support Moat prefix routing.
  - [ ] Update shroudb-sentry-client to prefix commands for Moat compatibility
  - [ ] Update shroudb-courier-client to prefix commands for Moat compatibility
  - [ ] Integration test: Sentry policy evaluation against Moat
  - [ ] Integration test: Courier notification delivery against Moat
  - [ ] Herald `moat_addr` config resolves to individual engine addresses automatically

- [ ] **D-2: Replace Chronicle with skeptik-audit-log** `session:audit-log` *(largest single item — trait swap, new endpoints, SDK support)*
  Chronicle is the wrong audit backend for Herald. It stores events in an in-memory hash chain on a remote ShrouDB instance — no tenant isolation (tenant_id is stuffed in a metadata HashMap), no queryability from Herald, hardcoded `Engine::ShrouDB` enum that doesn't represent Herald, and requires a running ShrouDB/Moat instance for a feature nobody can read back. Replace with `skeptik-audit-log` which provides tenant-isolated Postgres-backed audit with typed actions, JSONB diffs, and indexed queries.
  - [ ] Add `skeptik-audit-log` crate dependency (with `serde` + `postgres` features)
  - [ ] Replace `ChronicleOps` trait with `AuditStore` trait integration in `AppState`
  - [ ] Replace `AuditEvent` (flat strings) with `AuditEntry` (typed action, resource_type/resource_id, JSONB diff, actor, tenant_id as first-class field)
  - [ ] Migrate existing audit call sites (`event.publish` in WS handler, `stream.create` in HTTP) to new audit entries
  - [ ] Add audit entries for missing operations: stream delete, member add/remove, tenant CRUD, token create/revoke, webhook config changes, room archive/unarchive, message delete/redact, block/unblock, reaction add/remove
  - [ ] Add admin API endpoints for querying audit log: `GET /admin/tenants/{id}/audit` with filters (resource_type, actor, action, time range, pagination)
  - [ ] Remove `shroudb-chronicle-client` dependency, `integrations/chronicle.rs`, `ChronicleOps` trait, `MockChronicleOps`, and all Chronicle config (`chronicle_addr`, `chronicle_token`)
  - [ ] Postgres connection config for audit store (optional — audit disabled when not configured)
  - [ ] Integration test: publish event, query audit log, verify entry with correct tenant/actor/resource
  - [ ] Integration test: tenant A cannot query tenant B's audit entries
  - [ ] Integration test: filter by resource_type, actor, action, time range
  - [ ] Integration test: audit log captures all admin operations (stream CRUD, member changes, token management)
  - [ ] Admin SDK support: `audit.list()` / `audit.query()` in all 4 admin SDKs (TS, Go, Python, Ruby)

---

## LOW — Important but not blocking near-term goals

- [x] **L-1: Cross-instance fanout** `session:clustering`
  Remote store mode (H-4) enables shared persistence across Herald instances. The remaining gap is in-memory fanout: a subscriber on instance A won't receive events published on instance B. Add a pub/sub backplane.
  - [x] Design doc: backplane options (Redis Pub/Sub, NATS, ShroudB subscriptions)
  - [x] Backplane integration in fanout layer (publish to backplane after local fanout)
  - [x] Backplane consumer delivers remote events to local subscribers
  - [x] Sequence counters move to shared store (currently AtomicU64 per-process)
  - [x] Integration test: publish on instance A, receive on instance B
  - [x] Integration test: instance A dies, subscribers reconnect to B, no event loss within TTL

- [ ] **L-2: Mobile SDKs (Swift, Kotlin, Dart)** `session:one-per-sdk` *(3 sessions — one per language)*
  Blocks native mobile adoption.
  - [ ] Swift WebSocket client SDK with reconnect, presence, cursors
  - [ ] Kotlin WebSocket client SDK with reconnect, presence, cursors
  - [ ] Dart/Flutter WebSocket client SDK with reconnect, presence, cursors
  - [ ] E2EE support in each mobile SDK
  - [ ] Integration test per SDK: connect, publish, receive, reconnect catchup

- [ ] **L-3: PHP / .NET admin SDKs** `session:together`
  Expands addressable market to Laravel/WordPress and enterprise .NET.
  - [ ] PHP admin SDK (zero-dependency, curl-based)
  - [ ] C#/.NET admin SDK (HttpClient-based)
  - [ ] Integration test per SDK: full tenant API coverage

- [ ] **L-4: At-least-once delivery** `session:own` *(fanout layer redesign)*
  Requires ack tracking, redelivery queues, per-subscriber state. Redesign of fanout layer.
  - [ ] Design doc: delivery guarantee upgrade path
  - [ ] Per-subscriber ack tracking (seq-based)
  - [ ] Redelivery queue for unacked events
  - [ ] Client SDK: ack mechanism (opt-in, backwards compatible)
  - [ ] Integration test: kill subscriber mid-delivery, verify redelivery on reconnect
  - [ ] Integration test: verify no duplicate delivery under normal conditions

- [x] **L-5: GDPR data deletion API**
  Required for EU compliance. No per-tenant or per-user data purge exists.
  - [x] `DELETE /admin/tenants/{id}/data` — purge all tenant data from WAL
  - [x] `DELETE /streams/{id}/events?user_id=X` — purge all events by user
  - [x] Purge cascades to reactions, cursors, presence, blocks
  - [x] Integration test: purge tenant, verify tenant not found after purge
  - [x] Integration test: purge user, verify their events removed, other user's survive

- [x] **L-6: WebSocket message size limit**
  HTTP has 1MB body limit but WS has no explicit cap.
  - [x] Add configurable max frame size to WS handler (default 1MB)
  - [x] Reject oversized frames with error code
  - [x] Integration test: send frame exceeding limit, verify rejection
  - [x] Integration test: send frame at limit, verify acceptance

- [ ] **L-7: SDK contract tests** `session:together`
  No shared wire-protocol compliance tests. Risk of SDK drift.
  - [ ] Define contract test spec (JSON fixtures for each API endpoint + WS frame type)
  - [ ] TypeScript admin SDK passes contract tests
  - [ ] Go admin SDK passes contract tests
  - [ ] Python admin SDK passes contract tests
  - [ ] Ruby admin SDK passes contract tests
  - [ ] CI: contract tests run on SDK changes
