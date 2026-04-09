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

## Session Plan

| Session | Items | Scope | Status |
|---------|-------|-------|--------|
| ~~`extract-infra`~~ | ~~H-7~~ | ~~Rip metering + signup out of Herald, revert to transport-only~~ | **done** |
| ~~`e2ee-sdks`~~ | ~~M-3~~ | ~~E2EE in Go, Python, Ruby + cross-SDK interop tests~~ | **done** |
| ~~`clustering`~~ | ~~L-1~~ | ~~Pub/sub backplane for cross-instance fanout (storage already shared via remote store)~~ | **done** |
| ~~`embed-engines`~~ | ~~N-1~~ | ~~In-process engines (Sentry/Courier/Chronicle), Chronicle v1.5 upgrade, audit query API~~ | **done** |
| ~~`at-least-once`~~ | ~~N-2~~ | ~~Opt-in ack mode for at-least-once delivery~~ | **done** |
| ~~`contract-tests`~~ | ~~N-3~~ | ~~Define spec + wire all 4 existing SDKs~~ | **done** |
| ~~`sdk-hardening`~~ | ~~N-4~~ | ~~Error handling, incomplete features, missing tests across TS chat/admin packages~~ | **done** |
| `chat-extensions` | N-5 | Ephemeral events, per-message delivery status, event middleware | |
| `sdk-packaging` | N-6 | READMEs for chat SDKs, publish chat packages in release pipeline | |
| `openapi` | S-1 | `utoipa` annotations → `openapi.yaml` from Rust handlers | |
| `admin-codegen` | S-2 | Replace hand-rolled admin SDKs with generated + add PHP, C# | |
| `mobile-sdks` | S-3 | Swift, Kotlin, Dart WS client SDKs (hand-rolled, WS not HTTP) | |
| ~~`moat-clients`~~ | ~~—~~ | ~~shroudb crate updates for Moat prefix routing~~ | **superseded by N-1** |
| ~~`audit-log`~~ | ~~—~~ | ~~Chronicle rip-out, skeptik-audit-log integration~~ | **superseded by N-1** |

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

## NEXT — Ready to implement

- [x] **N-1: In-process engines + modernize audit** `session:embed-engines`
  Herald connects to Sentry, Courier, and Chronicle via TCP clients. Sentry already has an embedded mode (`EmbeddedSentryOps` wrapping `SentryEngine<EmbeddedStore>`). Extend this pattern to all three engines, and make it work with **both** store backends — `EmbeddedStore` (single-instance) and `RemoteStore` (managed multi-instance). All three engine crates are generic over `S: Store`, so the same engine code works in either mode: embedded store uses local WAL with namespaced prefixes, remote store uses the shared ShroudB instance. This eliminates external engine processes and Moat from the deployment. Simultaneously upgrade Chronicle from v1.0 to v1.5.0 (first-class `tenant_id`, query API, `Engine::Custom("herald")`, diff tracking, per-tenant hash chains). Managed deployment: Sigil handles user auth (standalone), Herald runs Sentry/Courier/Chronicle in-process against the shared remote store, all instances see the same engine state.

  **Phase 1 — Generalize engine init over Store backend**
  - [x] Refactor `EmbeddedSentryOps` → generic `InProcessSentryOps<S: Store>` that accepts any Store impl
  - [x] `main.rs` — create engine store from current backend: `EmbeddedStore::new(engine, "sentry")` in embedded mode, `RemoteStore` with `"sentry"` prefix in remote mode
  - [x] Integration test: Sentry authorization works in both embedded and remote store modes

  **Phase 2 — In-process Courier engine**
  - [x] Add `shroudb-courier-engine` dependency to Cargo.toml
  - [x] `integrations/in_process_courier.rs` — `InProcessCourierOps<S: Store>` wrapping `CourierEngine<S>`, implements `CourierOps`
  - [x] `main.rs` — initialize in-process Courier from current store backend, fall back to remote TCP if `courier_addr` configured
  - [x] Courier delivery dedup for multi-instance: claim-based pattern via atomic store op before delivering (prevents N instances sending the same notification)
  - [x] Integration test: offline notification delivery via in-process Courier
  - [x] Integration test (clustered): notification delivered exactly once across two instances

  **Phase 3 — In-process Chronicle engine + upgrade to v1.5**
  - [x] Upgrade `shroudb-chronicle-client` and `shroudb-chronicle-core` to v1.5 in Cargo.toml
  - [x] Add `shroudb-chronicle-engine` v1.5 dependency to Cargo.toml
  - [x] `integrations/in_process_chronicle.rs` — `InProcessChronicleOps<S: Store>` wrapping `ChronicleEngine<S>`, implements `ChronicleOps`
  - [x] `main.rs` — initialize in-process Chronicle from current store backend, fall back to remote TCP if `chronicle_addr` configured
  - [x] Update `AuditEvent` → Chronicle v1.5 `Event` with first-class `tenant_id`, `resource_type`/`resource_id` (split), `Engine::Custom("herald")`, `diff: Option<Value>`
  - [x] Update `ChronicleOps` trait to expose query methods: `query()`, `count()`
  - [x] Migrate existing audit call sites (WS `event.publish`, HTTP `stream.create`) to v1.5 event format
  - [x] Integration test: ingest audit event, query back by tenant, verify tenant isolation
  - [x] Integration test (clustered): audit event written on instance A, queryable from instance B

  **Phase 4 — Audit query API + coverage expansion**
  - [x] Add `GET /admin/tenants/{id}/audit` endpoint with filters (resource_type, resource_id, actor, action, time range, pagination) backed by Chronicle v1.5 query API
  - [x] Add `GET /admin/tenants/{id}/audit/count` endpoint
  - [x] Add audit entries for missing operations: stream delete, member add/remove, tenant CRUD, token create/revoke, block/unblock, reaction add/remove
  - [x] Integration test: publish event, query audit log, verify entry with correct tenant/actor/resource
  - [x] Integration test: tenant A cannot query tenant B's audit entries
  - [x] Integration test: filter by resource_type, actor, action, time range
  - [x] Integration test: audit log captures all admin operations (stream CRUD, member changes, token management)

  **Phase 5 — Admin SDK support**
  - [x] `audit.query()` / `audit.count()` in TypeScript admin SDK
  - [x] `audit.query()` / `audit.count()` in Go admin SDK
  - [x] `audit.query()` / `audit.count()` in Python admin SDK
  - [x] `audit.query()` / `audit.count()` in Ruby admin SDK

  **Cleanup**
  - [x] Remove `moat_addr` config field (no longer needed — engines are in-process or addressed individually)
  - [x] Remote TCP client crates kept as fallback overrides behind `[shroudb]` config section
  - [x] `cargo build`, `cargo clippy`, `cargo test --workspace` clean
  - [x] `cargo build --no-default-features` clean
  - [x] Live integration tests pass

- [x] **N-2: At-least-once delivery** `session:at-least-once`
  Opt-in ack mode for at-least-once delivery. Per-stream seq high-water-mark acks, cursor-based reconnect catchup.
  - [x] Design doc: delivery guarantee upgrade path
  - [x] Per-subscriber ack tracking (seq-based)
  - [x] Redelivery queue for unacked events
  - [x] Client SDK: ack mechanism (opt-in, backwards compatible)
  - [x] Integration test: kill subscriber mid-delivery, verify redelivery on reconnect
  - [x] Integration test: verify no duplicate delivery under normal conditions

- [x] **N-3: SDK contract tests** `session:contract-tests`
  No shared wire-protocol compliance tests. Risk of SDK drift.
  - [x] Define contract test spec (JSON fixtures for each API endpoint + WS frame type)
  - [x] TypeScript admin SDK passes contract tests
  - [x] Go admin SDK passes contract tests
  - [x] Python admin SDK passes contract tests
  - [x] Ruby admin SDK passes contract tests
  - [x] CI: contract tests run on SDK changes

- [x] **N-4: SDK hardening** `session:sdk-hardening`
  Audit found error handling gaps, incomplete features, and missing tests across the three TypeScript chat/admin packages. Fix everything before codegen replaces the admin SDK and before external consumption of chat packages.

  **herald-admin-typescript**
  - [x] Fix silent JSON parse failures — empty catch in transport masks real errors; throw or propagate
  - [x] Complete HTTP error mapping — handle 400, 403, 409, 429, 500, 503 (not just 401/404)
  - [x] Type `Promise<unknown>` methods — `connections()`, `adminEvents()`, `errors()`, `stats()` need typed responses
  - [x] Migrate test-integration off deprecated top-level `presence`/`blocks` → `chat.*`, then remove
  - [x] Add timeout configuration to `HeraldAdminOptions` (propagate to fetch via AbortSignal)
  - [x] Contract tests still pass, CI green

  **herald-chat-core**
  - [x] Wire `scrollIdleMs` — debounce `autoMarkRead()` calls in `setAtLiveEdge()` and `handleEvent()` by the configured delay instead of firing immediately. Clear timer when user scrolls away from edge. Test: cursor not sent instantly, only after idle.
  - [x] Wire `PendingMessage` — implement wrapper class delegating to existing `retrySend()`/`cancelSend()`, change `send()` return type from `Promise<string>` to `Promise<PendingMessage>`, update react hook
  - [x] Fix `prependBatch` insertion — use binary insertion (O(n)) instead of `Array.sort` (O(n log n)), consistent with `appendEvent`
  - [x] Guard edit-after-delete — reject body update if `deleted: true`
  - [x] Make Notifier resilient — wrap each listener call in try/catch so one bad listener doesn't break the loop
  - [x] Make `loadMore` limit configurable — accept as `ChatCoreOptions.loadMoreLimit`, default 50
  - [x] Unit tests pass, no regressions

  **herald-chat-react**
  - [x] Add input validation — undefined `streamId` throws early with useful error
  - [x] `send()` returns `PendingMessage` — consumers use status/retry/cancel instead of raw promises
  - [x] Hook + component tests pass, CI green

  **Cross-cutting**
  - [x] Existing integration tests pass
  - [x] All existing contract tests pass

- [ ] **N-5: ChatCore completeness** `session:chat-extensions`
  ChatCore handles the chat-generic transport/state layer but has gaps in features that any chat SDK is expected to provide. Specifically: ephemeral events are silently dropped, there's no per-message delivery status, and the event processing pipeline is closed (no middleware for consumers to layer domain logic on top). These are chat primitives, not application-specific features.

  **Ephemeral event support**
  - [ ] Register `event.received` handler — currently the only SDK event type ChatCore ignores
  - [ ] Forward through middleware chain, then emit on `ephemeral:{streamId}` Notifier slice

  **Lightweight stream subscriptions**
  - [ ] Add `listen(streamId)` — subscribes to stream via the same transport, events flow through middleware and Notifier, but no stores initialized (no MessageStore, CursorStore, ScrollState overhead)
  - [ ] Covers inbox/notification rooms (e.g. `user:{userId}`) without forcing consumers onto a separate raw HeraldClient. Same middleware chain, same Notifier slices
  - [ ] `unlisten(streamId)` to unsubscribe. `leaveStream` remains for full-state streams

  **Per-message read status**
  - [ ] Extend `Message.status`: add `read` → `sending | sent | failed | read`
  - [ ] Track remote cursors in CursorStore (`remoteCursors: Map<streamId, Map<userId, seq>>`). Wire `handleCursor` (currently a no-op) to update remote cursor positions
  - [ ] When a remote cursor advances past a self-sent message's seq, flip that message's status to `read`. Reactive via Notifier
  - [ ] Group chat semantics: `status = "read"` means "at least one remote user's cursor >= this seq." Consumers needing per-user granularity derive it from the remote cursor data in CursorStore directly
  - [ ] No `delivered` state — Herald has no delivery receipt primitive (would require a new protocol frame). `sent` (server ack) is the strongest signal the transport provides

  **Event middleware**
  - [ ] Define `ChatEvent` discriminated union over all 10 event types (event, edited, deleted, reaction, presence, cursor, typing, member.joined, member.left, ephemeral)
  - [ ] Add `middleware?: Array<(event: ChatEvent, next: () => void) => void>` to `ChatCoreOptions`
  - [ ] `next()` applies the store mutation synchronously. Enrichment before `next()`, side-effects after. Skipping `next()` drops the event
  - [ ] Middleware sees raw events, not computed effects. Cursor → read-status flips happen inside the store update. Consumers reacting to computed state changes (e.g. "message marked read") use Notifier `subscribe()`, not middleware

  **Validation**
  - [ ] Unit tests: ephemeral events forwarded, per-message status transitions (sent → read on remote cursor), middleware chain (enrich, skip, passthrough, ordering)
  - [ ] Existing chat-core unit tests still pass (all additions are opt-in)

- [ ] **N-6: SDK packaging and documentation** `session:sdk-packaging`
  Chat packages are not published in the release pipeline. No READMEs in any chat SDK directory. Blocks external consumption.

  **READMEs**
  - [ ] `herald-chat-sdk-typescript/README.md` — WS client wrapper, frame types, E2EE integration
  - [ ] `herald-chat-core/README.md` — ChatCore API, stores, optimistic sends, liveness, scroll state
  - [ ] `herald-chat-react/README.md` — Provider, hooks, render-prop components, useSyncExternalStore pattern
  - [ ] `contract-tests/README.md` — How to run, spec format, adding new SDKs

  **Release pipeline**
  - [ ] Add `herald-chat-sdk` to `publish-npm` job — scope to `@skeptik-io/herald-chat-sdk`, build + publish
  - [ ] Add `herald-chat-core` to `publish-npm` job — scope to `@skeptik-io/herald-chat`, build + publish
  - [ ] Add `herald-chat-react` to `publish-npm` job — scope to `@skeptik-io/herald-chat-react`, build + publish
  - [ ] Verify dependency chain resolves: herald-sdk → herald-chat-sdk → herald-chat → herald-chat-react (peer deps point to scoped `@skeptik-io/*` names)
  - [ ] Tag and verify all 5 TS packages publish successfully

---

## COMPLETED

- [x] **L-1: Cross-instance fanout** `session:clustering`
  Remote store mode (H-4) enables shared persistence across Herald instances. The remaining gap is in-memory fanout: a subscriber on instance A won't receive events published on instance B. Add a pub/sub backplane.
  - [x] Design doc: backplane options (Redis Pub/Sub, NATS, ShroudB subscriptions)
  - [x] Backplane integration in fanout layer (publish to backplane after local fanout)
  - [x] Backplane consumer delivers remote events to local subscribers
  - [x] Sequence counters move to shared store (currently AtomicU64 per-process)
  - [x] Integration test: publish on instance A, receive on instance B
  - [x] Integration test: instance A dies, subscribers reconnect to B, no event loss within TTL

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

---

## NEXT — SDK codegen pipeline

Hand-rolling SDKs does not scale. Four admin SDKs already drift (the contract tests caught a dead `events.search` method on all four). Before adding mobile or server SDKs, the API surface must be machine-described and SDKs generated from a single source of truth.

- [ ] **S-1: OpenAPI spec from Rust handlers** `session:openapi`
  Add `utoipa` annotations to every HTTP handler in `herald-server/src/http/` and `herald-server/src/chat/`. Generate `openapi.yaml` as a build artifact. This becomes the single source of truth for the Herald HTTP API.
  - [ ] Add `utoipa` and `utoipa-axum` dependencies to Cargo.toml
  - [ ] Annotate all tenant-scoped endpoints (streams, members, events, trigger, presence, cursors, blocks)
  - [ ] Annotate all admin endpoints (tenants, tokens, audit, connections, stats, errors)
  - [ ] Annotate health/liveness/readiness/metrics endpoints
  - [ ] Define request/response schemas via `ToSchema` derives or manual `utoipa::ToSchema` impls
  - [ ] Serve `/openapi.json` endpoint (opt-in, behind feature flag)
  - [ ] CI: generate `openapi.yaml` and commit to repo root as versioned artifact
  - [ ] Validate generated spec against contract-tests/spec/spec.json (no endpoint drift)
  - [ ] `cargo build`, `cargo clippy`, `cargo test --workspace` clean

- [ ] **S-2: Admin SDK codegen** `session:admin-codegen`
  Replace hand-rolled admin SDKs with generated ones. Use openapi-generator (or a lighter alternative if openapi-generator output quality is unacceptable — evaluate during this session). Generated SDKs must pass the existing contract tests with zero behavioral diff.
  - [ ] Evaluate codegen tooling: openapi-generator vs Kiota vs custom templates. Pick one.
  - [ ] Define generator config + mustache/template overrides for Herald SDK style (namespaced clients, chat grouping, error types)
  - [ ] Generate TypeScript admin SDK, diff against hand-rolled, pass contract tests
  - [ ] Generate Go admin SDK, diff against hand-rolled, pass contract tests
  - [ ] Generate Python admin SDK, diff against hand-rolled, pass contract tests
  - [ ] Generate Ruby admin SDK, diff against hand-rolled, pass contract tests
  - [ ] Generate PHP admin SDK, pass contract tests
  - [ ] Generate C#/.NET admin SDK, pass contract tests
  - [ ] CI: codegen runs on OpenAPI spec changes, generated code committed
  - [ ] Remove hand-rolled admin SDK source (replaced by generated)
  - [ ] Contract tests pass for all 6 admin SDKs

- [ ] **S-3: Mobile client SDKs (Swift, Kotlin, Dart)** `session:mobile-sdks`
  WebSocket client SDKs for native mobile. These are **not** admin SDKs (no codegen from OpenAPI — the WS protocol is not HTTP). Hand-rolled but informed by the WS frame spec in contract-tests/spec/spec.json.
  - [ ] Swift WebSocket client SDK with reconnect, presence, cursors, ack mode
  - [ ] Kotlin WebSocket client SDK with reconnect, presence, cursors, ack mode
  - [ ] Dart/Flutter WebSocket client SDK with reconnect, presence, cursors, ack mode
  - [ ] E2EE support in each mobile SDK (x25519 + AES-256-GCM, interop with TS/Go/Python/Ruby)
  - [ ] Integration test per SDK: connect, subscribe, publish, receive, reconnect catchup, ack
  - [ ] CI: type check / build each SDK

---

## UPSTREAM — ShroudB crate issues

- [x] **U-1: `shroudb-sentry-engine` v1.4.10 incompatible with `shroudb-chronicle-core` v1.5.0**
  Fixed in v1.4.11.

- [x] **U-2: `shroudb-sentry-engine` v1.4.11 references `Policy.version` not yet published**
  Fixed — `shroudb-sentry-core` v1.4.11 published with `Policy.version` field.

