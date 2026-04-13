# Changelog

All notable changes to Herald are documented in this file.

## [Unreleased]

## [5.0.1] - 2026-04-13

### Added

- **`ChatCore.seedReactions(streamId, messageId, reactions)`**: seed
  DB-sourced reactions onto a message already in the store. Idempotent
  via the underlying `applyReaction` Set semantics; silent no-op if the
  message is not present. Exists to let apps backed by a canonical
  database hydrate old messages' reaction state on initial load without
  round-tripping through the transport.

- **`ChatCore.seedRemoteCursor(streamId, userId, seq)`**: seed a remote
  user's read-cursor from an external source. Mirrors the envelope path:
  MAX-only advancement, self-skip, listen-only skip, and auto `markRead`
  on self-sent messages up to `seq`. Pairs with `seedReactions` to give
  apps a complete DB-seed story alongside `seedHistory`.

### Why

v5.0.0 collapsed chat state mutations onto the envelope-over-`event.new`
path and removed the dedicated reaction/cursor stores from the server.
That works end-to-end for live traffic, but left a gap for apps whose
DB is the canonical store: freshly seeded messages from `seedHistory`
start with an empty reactions Map and `"sent"` status, and there was no
public API to populate them from the app's canonical state. Apps were
forced to maintain a parallel reaction store in `meta`, defeating the
point of the v5 collapse. These two additive methods close the gap
without expanding the mutation surface.

## [5.0.0] - 2026-04-13

### Breaking Changes

- **Chat state mutations are now typed envelopes on `event.publish`, not
  dedicated WS frames.** Edits, deletes, reactions, and read-cursor
  advances ride the same `event.publish` → `event.new` path as messages,
  distinguished by a `kind` field in the event body. The chat engine
  on the server no longer has dedicated handlers for any of them, and
  the corresponding WS frame types (`event.edit`, `event.delete`,
  `reaction.add`, `reaction.remove`, `cursor.update`) and server-to-client
  frames (`event.edited`, `event.deleted`, `reaction.changed`,
  `cursor.moved`) have been removed. Chat-core's SDK handles the envelope
  dispatch end-to-end — see `herald-chat-core/src/envelope.ts`.

- **HTTP endpoints for chat-state mutations are removed.** The following
  were deleted in favor of publishing a typed envelope via
  `POST /streams/{id}/events`:
  - `PATCH /streams/{id}/events/{event_id}` (edit)
  - `DELETE /streams/{id}/events/{event_id}` (delete)
  - `GET /streams/{id}/events/{event_id}/reactions` (reaction list)
  - `GET /streams/{id}/cursors` (cursor list)

- **Server no longer enforces ownership for edit/delete.** Any
  member can publish an edit/delete envelope; apps are responsible for
  enforcing ownership in their webhook consumer when persisting to the
  canonical app database.

- **`store::reactions` module removed.** Reactions are no longer
  persisted in a dedicated namespace — they live in the event stream
  as envelopes and the app DB (via webhook mirror) is canonical across
  time.

- **`SubscribedPayload.cursor`** is now the ack-mode delivery
  high-water-mark, not the user-facing read cursor. Read cursors are
  now reconstructed client-side from `cursor` envelopes in the event
  stream (within the buffer window) or hydrated from the app DB.

### Why

Herald is a transport layer (Pusher/Ably/Centrifugo category), not a
chat server. The previous design had dedicated server-side handlers
and persistence for chat-specific mutations, which both contradicted
the "delivery layer, not a database" framing and created a buggy
parallel path (fire-and-forget frames with no ref/ack) alongside the
working `event.publish` path. Collapsing everything to the publish
path gives chat operations free ref/ack/optimistic/reconcile/replay
semantics and lets the server stay focused on transport + authz.

### Migration

Apps using `herald-chat-core` or `herald-chat-react` receive this
change transparently — the core's `addReaction` / `removeReaction` /
`edit` / `deleteEvent` methods now publish envelopes under the hood
with optimistic updates and rollback on failure. Apps using the raw
`herald-sdk` directly should publish envelopes of the form
`JSON.stringify({ kind: "reaction" | "edit" | "delete" | "cursor", ... })`
instead of calling the removed frame methods on `HeraldChatClient`.

Webhook consumers that switched on `event.event === "event.edited"` etc.
should instead switch on `body.kind` inside the received `event.new`
payload.

## [4.0.0] - 2026-04-11

### Breaking Changes

- **Engine architecture**: Herald now uses a Moat-style engine composition system. Domain features (chat, presence) are isolated engines behind feature flags, producing different server binaries from the same crate.
- **`setPresence` moved from chat SDK to presence SDK**: `HeraldChatClient.setPresence()` removed. Use `HeraldPresenceClient.setPresence()` from `herald-presence-sdk`.
- **Admin SDK namespace restructured**: `admin.chat.presence` moved to `admin.presence` (top-level).
- **`usePresence()` return type changed**: Returns `{ presence, setPresence, clearOverride }` instead of a plain string.
- **`PresenceIndicator` props changed**: Now provides `{ online, away, dnd, offline }` member arrays instead of just `{ members, onlineCount }`.

### Added

- **Chat engine** (`--features chat`): Event edit/delete, reactions, read cursors, typing indicators, user blocks, block-based fanout filtering.
- **Presence engine** (`--features presence`): Manual presence overrides (away/dnd/offline), per-override expiry with `until` field ("away until Monday 9am"), WAL-persisted overrides, `__presence` broadcast stream for global real-time presence, watchlist with override awareness, batch presence queries (`GET /presence?user_ids=...`), admin presence API (`POST /presence/{user_id}`).
- **`HeraldEngine` trait**: Extension point for engines with lifecycle hooks (`on_connect`, `on_disconnect`, `on_last_disconnect`), fanout filtering, HTTP route registration, and background timer tasks.
- **Docker multi-image builds**: `herald` (social/default), `herald-transport`, `herald-chat`, `herald-presence` — all from one Dockerfile via `HERALD_FEATURES` build arg.
- **`docker-bake.hcl`**: Multi-image build configuration for local builds.
- **`herald-presence-sdk-typescript`**: New package — `HeraldPresenceClient` with `setPresence(status, until?)` and `clearOverride()`.
- **`ChatCore.setPresence()`/`clearPresenceOverride()`/`getPresence()`**: Manual presence override API in chat-core.
- **`PresenceChanged.until`**: Optional ISO 8601 expiry field in presence events.
- **`PresenceStatus.Offline` as manual override**: Users can "appear offline" while connected.
- **Admin SDK `presence.getBulk()`/`presence.setOverride()`**: Batch presence queries and admin-initiated overrides — all 6 SDKs (TS, Go, Python, Ruby, PHP, C#).
- **CI feature-matrix checks**: Clippy runs for all 4 feature combinations (default, no-features, chat-only, presence-only).

### Changed

- Project README rewritten: transport-first positioning, engine architecture docs, corrected terminology (streams not rooms), Docker image matrix.
- Release workflow: Docker builds parallelized via matrix strategy.
- OpenAPI spec: includes all presence engine endpoints.
- Contract tests: cover `presence.getBulk` and `presence.setOverride` operations.

## [3.1.0] - 2026-04-11

### Added

- OpenAPI 3.1 spec generation from Rust handler annotations.
- Codegen pipeline for admin SDKs — generates TypeScript, Go, Python, Ruby, PHP, and C# from OpenAPI spec.
- PHP and C# admin SDKs.

### Fixed

- NuGet pack compatibility (PackageLicenseFile).
- Codegen `.ts` imports for tsx compat across Node versions.
- PHP transport compat with PHP 8.3.

## [3.0.7] - 2026-04-10

### Added

- OpenAPI 3.1 spec auto-generated from utoipa handler annotations.

## [3.0.6] - 2026-04-10

### Fixed

- WebSocket heartbeat: client ping + server idle timeout for connection keep-alive through proxies.

## [3.0.5] - 2026-04-10

### Changed

- Provider owns connection lifecycle with strict-mode safety in React SDK.

## [3.0.4] - 2026-04-10

### Added

- Basic auth (key+secret) support in all admin SDKs.

## [3.0.3] - 2026-04-10

### Fixed

- WASM dependency severed from barrel export — include WASM in package without requiring it at build time.

## [3.0.1] - 2026-04-09

### Added

- Auto-generate tenant ID on creation — only name required.

## [3.0.0] - 2026-04-09

### Breaking Changes

- **Auth redesign**: Replaced JWT with HMAC-SHA256 key+secret model. Tenants use `key` + `secret` pair. WebSocket auth via signed query params. Admin API uses bearer token.
- **Single port**: HTTP API and WebSocket serve on the same port (default 6200). Removed separate WebSocket port.
- **Auth on upgrade**: WebSocket authentication happens during HTTP upgrade, not as a post-connect frame.
- **Terminology**: `room` renamed to `stream`, `message` renamed to `event` across all APIs and SDKs.

### Added

- Self-service API (`/self/*`): tenants manage their own tokens, rotate secrets.
- Contract test suite: shared spec validated across TypeScript, Go, Python, Ruby SDKs.
- Delivery receipts (`event.delivered` frame).
- At-least-once delivery via opt-in ack mode.
- Cross-instance fanout via ShroudB store subscriptions (clustering).
- Per-tenant retention tiers with admin API.
- E2EE modules for Go, Python, Ruby admin SDKs.
- OpenTelemetry distributed tracing with W3C traceparent.
- GDPR data deletion API.
- Configurable WebSocket message size limit.
- ChatCore middleware system, ephemeral event storage, lightweight subscriptions.

## [1.1.7] - 2026-04-02

### Fixed

- Message display overflow and system message styling in admin UI.

## [1.1.6] - 2026-04-02

### Fixed

- Mobile-responsive layout in admin UI.

## [1.1.5] - 2026-04-02

### Added

- Per-tenant metrics and tenant-scoped stats endpoint.

## [1.1.4] - 2026-04-02

### Fixed

- Admin UI overview uses health/metrics directly.

## [1.1.3] - 2026-04-02

### Fixed

- Server-side proxy, revert CORS from Herald.

## [1.1.2] - 2026-04-02

### Added

- CORS headers on HTTP API.

## [1.1.1] - 2026-04-02

### Fixed

- Respect PORT env var in admin UI container.

## [1.1.0] - 2026-04-02

### Added

- Admin UI for tenant management, room monitoring, and debug console.

## [1.0.3] - 2026-04-02

### Fixed

- Hydrate all tenants on startup regardless of mode.

## [1.0.2] - 2026-04-02

### Changed

- `/ws` as primary WebSocket path, single-port Docker.

## [1.0.1] - 2026-04-02

### Added

- WebSocket on same port as HTTP via `/ws` path.

## [1.0.0] - 2026-04-02

### Added

- Multi-tenant WebSocket server with rooms, messages, presence, cursors, real-time fanout.
- ShroudB WAL storage (no external database).
- ABAC authorization via embedded ShroudB Sentry.
- TypeScript browser SDK and admin SDK.
- Go, Python, Ruby admin SDKs.
- Prometheus metrics at `/metrics`.
- Integration test suite.

## [0.1.0] - 2026-04-02

Initial release.

[4.0.0]: https://github.com/skeptik-io/herald/compare/v3.1.0...v4.0.0
[3.1.0]: https://github.com/skeptik-io/herald/compare/v3.0.7...v3.1.0
[3.0.7]: https://github.com/skeptik-io/herald/compare/v3.0.6...v3.0.7
[3.0.6]: https://github.com/skeptik-io/herald/compare/v3.0.5...v3.0.6
[3.0.5]: https://github.com/skeptik-io/herald/compare/v3.0.4...v3.0.5
[3.0.4]: https://github.com/skeptik-io/herald/compare/v3.0.3...v3.0.4
[3.0.3]: https://github.com/skeptik-io/herald/compare/v3.0.1...v3.0.3
[3.0.1]: https://github.com/skeptik-io/herald/compare/v3.0.0...v3.0.1
[3.0.0]: https://github.com/skeptik-io/herald/compare/v1.1.7...v3.0.0
[1.1.7]: https://github.com/skeptik-io/herald/compare/v1.1.6...v1.1.7
[1.1.6]: https://github.com/skeptik-io/herald/compare/v1.1.5...v1.1.6
[1.1.5]: https://github.com/skeptik-io/herald/compare/v1.1.4...v1.1.5
[1.1.4]: https://github.com/skeptik-io/herald/compare/v1.1.3...v1.1.4
[1.1.3]: https://github.com/skeptik-io/herald/compare/v1.1.2...v1.1.3
[1.1.2]: https://github.com/skeptik-io/herald/compare/v1.1.1...v1.1.2
[1.1.1]: https://github.com/skeptik-io/herald/compare/v1.1.0...v1.1.1
[1.1.0]: https://github.com/skeptik-io/herald/compare/v1.0.3...v1.1.0
[1.0.3]: https://github.com/skeptik-io/herald/compare/v1.0.2...v1.0.3
[1.0.2]: https://github.com/skeptik-io/herald/compare/v1.0.1...v1.0.2
[1.0.1]: https://github.com/skeptik-io/herald/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/skeptik-io/herald/compare/v0.1.7...v1.0.0
[0.1.0]: https://github.com/skeptik-io/herald/releases/tag/v0.1.0
