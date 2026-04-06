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
  - [ ] Integration test: server boots and operates with remote backend (mock or real)
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

---

## MEDIUM — Competitive differentiation & monetization readiness

- [ ] **M-1: Per-plan limit enforcement**
  Plan limits are defined in Meterd. When metering is enabled, Meterd is the sole authority. When disabled, global config applies uniformly.
  - [x] Enforce connection limit per plan at WS auth (Meterd-cached or global config)
  - [x] Enforce rate limit per plan at HTTP middleware (Meterd-cached or global config)
  - [x] Enforce stream count per plan at stream creation (Meterd-cached or global config)
  - [x] Meterd quota check at WS and HTTP publish (check_quota with circuit breaker, fail-open)
  - [x] Plan limits cached per-tenant with 5-min TTL from Meterd quota snapshot
  - [x] Block filtering enforced in event delivery (blocked user's messages not delivered)
  - [ ] Integration test: plan-based limits with running Meterd (requires Rust SDK + Meterd instance)

- [ ] **M-2: Billing/metering integration (Meterd)**
  Wire per-tenant usage counters to Meterd for MAU tracking and Stripe billing.
  - [x] Meterd client integration in Herald server (hand-rolled HTTP, pending Rust SDK)
  - [x] Report `events_published` per tenant per billing period
  - [x] Report `webhooks_sent` per tenant per billing period
  - [x] Report peak concurrent connections per tenant (every 60s in stats loop)
  - [x] Wire check_quota() to enforcement points (WS + HTTP publish)
  - [x] Circuit breaker on Meterd remote calls (5 failures → open, 30s cooldown)
  - [x] Events re-buffered on flush failure (not lost)
  - [x] fetch_plan_limits validates all expected meters present, warns on missing
  - [x] Overage handling: soft quota warnings before hard cutoff
  - [ ] Integration test: usage events flow to Meterd (requires Rust SDK + Meterd instance)
  - [ ] Replace hand-rolled HTTP client with Meterd Rust SDK

- [x] **M-3: Catchup pagination**
  200-event catchup cap with `has_more` but no follow-up mechanism. Clients can lose events.
  - [x] Add cursor-based pagination to reconnect catchup (WS `EventsFetch` with `after` cursor)
  - [x] Client SDK: automatic paginated catchup on reconnect until fully caught up
  - [x] Integration test: publish 250 events, paginate with fetch({after}), verify all received
  - [x] Integration test: verify ordering preserved across pages

- [ ] **M-4: E2EE in non-TypeScript SDKs**
  E2EE is browser-only. Expand to server-side SDKs.
  - [ ] E2EE module for Go admin SDK (x25519 + AES-256-GCM)
  - [ ] E2EE module for Python admin SDK
  - [ ] E2EE module for Ruby admin SDK
  - [ ] Integration test: encrypt in Go, decrypt in TypeScript (cross-SDK interop)
  - [ ] Integration test: blind token search works across SDK implementations

- [ ] **M-5: OpenTelemetry / distributed tracing**
  Replace `X-Request-Id` with proper trace propagation for enterprise observability.
  - [ ] Add `tracing-opentelemetry` crate with OTLP exporter
  - [ ] Propagate W3C `traceparent` header on HTTP requests
  - [ ] Inject trace context into WebSocket frame processing spans
  - [ ] Integration test: verify trace IDs propagate through publish → store → fanout pipeline
  - [ ] `/metrics` endpoint continues to work alongside OTLP export

- [ ] **M-6: Per-tenant retention tiers**
  TTL is global 7-day. Enable per-tenant configuration for premium pricing.
  - [x] `retention_days` field in PlanLimits — read from Meterd, falls back to global config
  - [x] Event insert uses per-tenant TTL via state.event_ttl_ms(tenant_id)
  - [x] TTL cleanup job already respects per-event expiry timestamps (no changes needed)
  - [x] Retention per tenant managed via Meterd plan configuration
  - [ ] Integration test: tenant A (7d) events expire, tenant B (30d) events survive (requires Meterd)
  - [ ] Integration test: changing retention applies to future expiry only (requires Meterd)

- [ ] **M-7: Self-serve tenant provisioning**
  Tenant creation is admin-API-only. Enable product-led growth.
  - [ ] Public signup endpoint (rate-limited, captcha or email verification)
  - [ ] Auto-provision tenant with default plan and limits
  - [ ] Generate initial API token on signup
  - [ ] Integration test: signup → receive token → create stream → publish event
  - [ ] Integration test: signup rate limiting

---

## LOW — Important but not blocking near-term goals

- [ ] **L-1: Clustering / HA**
  Single-node is a production liability. Requires fundamental rearchitecture.
  - [ ] Design doc: state distribution strategy (sharding by tenant vs. shared-nothing with replication)
  - [ ] Cluster membership discovery (DNS, gossip, or static config)
  - [ ] Event replication across nodes
  - [ ] Connection routing / rebalancing on node failure
  - [ ] Integration test: kill node, verify clients reconnect to surviving node
  - [ ] Integration test: events published on node A received by subscribers on node B

- [ ] **L-2: Mobile SDKs (Swift, Kotlin, Dart)**
  Blocks native mobile adoption.
  - [ ] Swift WebSocket client SDK with reconnect, presence, cursors
  - [ ] Kotlin WebSocket client SDK with reconnect, presence, cursors
  - [ ] Dart/Flutter WebSocket client SDK with reconnect, presence, cursors
  - [ ] E2EE support in each mobile SDK
  - [ ] Integration test per SDK: connect, publish, receive, reconnect catchup

- [ ] **L-3: PHP / .NET admin SDKs**
  Expands addressable market to Laravel/WordPress and enterprise .NET.
  - [ ] PHP admin SDK (zero-dependency, curl-based)
  - [ ] C#/.NET admin SDK (HttpClient-based)
  - [ ] Integration test per SDK: full tenant API coverage

- [ ] **L-4: At-least-once delivery**
  Requires ack tracking, redelivery queues, per-subscriber state. Redesign of fanout layer.
  - [ ] Design doc: delivery guarantee upgrade path
  - [ ] Per-subscriber ack tracking (seq-based)
  - [ ] Redelivery queue for unacked events
  - [ ] Client SDK: ack mechanism (opt-in, backwards compatible)
  - [ ] Integration test: kill subscriber mid-delivery, verify redelivery on reconnect
  - [ ] Integration test: verify no duplicate delivery under normal conditions

- [ ] **L-5: GDPR data deletion API**
  Required for EU compliance. No per-tenant or per-user data purge exists.
  - [ ] `DELETE /admin/tenants/{id}/data` — purge all tenant data from WAL
  - [ ] `DELETE /streams/{id}/events?user_id=X` — purge all events by user
  - [ ] Purge cascades to reactions, cursors, presence, blocks
  - [ ] Integration test: purge tenant, verify zero keys with tenant prefix remain
  - [ ] Integration test: purge user, verify their events/reactions/cursors removed

- [x] **L-6: WebSocket message size limit**
  HTTP has 1MB body limit but WS has no explicit cap.
  - [x] Add configurable max frame size to WS handler (default 1MB)
  - [x] Reject oversized frames with error code
  - [x] Integration test: send frame exceeding limit, verify rejection
  - [x] Integration test: send frame at limit, verify acceptance

- [ ] **L-7: SDK contract tests**
  No shared wire-protocol compliance tests. Risk of SDK drift.
  - [ ] Define contract test spec (JSON fixtures for each API endpoint + WS frame type)
  - [ ] TypeScript admin SDK passes contract tests
  - [ ] Go admin SDK passes contract tests
  - [ ] Python admin SDK passes contract tests
  - [ ] Ruby admin SDK passes contract tests
  - [ ] CI: contract tests run on SDK changes
