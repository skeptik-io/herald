# Herald Production Audit Tracker

> Full audit completed 2026-04-03. All 39 items must be implemented **and** covered by integration tests against a live server before being marked complete. No stubs, no placeholders, no unwired code.

**Completion rule:** An item is only `[x]` when:
1. The fix/feature is implemented in code
2. Integration test(s) exist that exercise it against a running Herald instance
3. The test(s) verify the behavior end-to-end (not mocked)

---

## Phase 1: Security & Correctness (blocks deployment)

- [x] **1. Constant-time admin token comparison**
  - Use `subtle::ConstantTimeEq` for super_admin_token check in `http/mod.rs:139-142`
  - Test: send incorrect tokens of varying prefix-match lengths, verify uniform rejection timing and 401 response

- [x] **2. Add HTTP body size limits**
  - Add `DefaultBodyLimit` middleware to HTTP router in `http/mod.rs`
  - Test: POST a 10MB JSON body to `/rooms`, `/rooms/:id/messages`, `/admin/tenants` — verify 413 Payload Too Large

- [x] **3. Add input validation (ID format, body size, meta size)**
  - Validate tenant/room/user IDs: alphanumeric + `-` + `_`, max 255 chars
  - Validate message body: max configurable size (default 64KB)
  - Validate meta field: max configurable size (default 16KB)
  - Reject path traversal characters, null bytes, control characters
  - Test: submit IDs with `../`, null bytes, 1MB strings, oversized body/meta — verify 400 Bad Request for each

- [x] **4. Implement HTTP API rate limiting**
  - Wire up the existing `api_rate_limit` config (currently dead code)
  - Per-tenant rate limiting on all authenticated HTTP endpoints
  - Test: send `api_rate_limit + 1` requests in under a minute, verify the last one returns 429 Too Many Requests

- [x] **5. Fix sliding-window rate limiting on WebSocket**
  - Replace fixed-window counter in `ws/connection.rs:183-188` with sliding window
  - Test: send `max_messages_per_sec` messages at the boundary of two windows, verify the burst is correctly limited (not 2x)

- [x] **6. Fix backpressure: increment metrics, notify client**
  - `registry/connection.rs:103` — increment `messages_dropped` metric when `try_send()` fails
  - Send error notification to slow client when messages are being dropped
  - Test: fill a client's channel buffer (256), send another message via fanout, verify `messages_dropped` metric increments and client receives a backpressure warning

- [x] **7. Fix presence linger race condition**
  - `ws/connection.rs:253-272` — TOCTOU between disconnect check and linger task
  - Fix: use atomic state or cancellation token so reconnect during linger cancels the offline broadcast
  - Test: connect user, disconnect, immediately reconnect within linger window — verify presence never broadcasts offline

- [x] **8. Fix circuit breaker half-open to allow exactly one probe**
  - `circuit_breaker.rs:70-73` — HalfOpen state allows all calls through instead of one
  - Implement permit counter: one call in HalfOpen, reject subsequent until probe completes
  - Test: trigger circuit open, wait for half-open, send 5 concurrent requests — verify only 1 passes through, 4 are rejected

- [x] **9. Add CORS middleware**
  - Wire up `CorsLayer` from tower-http (already a dependency) in `http/mod.rs`
  - Make allowed origins configurable
  - Test: send OPTIONS preflight request, verify correct `Access-Control-*` headers. Send request with disallowed origin, verify rejection.

- [x] **10. Sanitize error responses**
  - All HTTP endpoints: return generic error messages to clients, log detailed errors server-side
  - Affected files: `http/rooms.rs:58,81,101,121,150,178`, `http/members.rs:52,96,146`, `http/messages.rs:67,86,195`
  - Test: trigger a store error (e.g., create duplicate room), verify response body does NOT contain internal error details (no "WAL", stack traces)

- [x] **11. Add config validation at startup**
  - Validate: numeric values > 0, required fields non-empty, JWT secret minimum length, TLS paths exist if configured, webhook secret non-empty if webhook URL set
  - Test: start Herald with `max_messages_per_sec=0`, empty `jwt_secret`, empty TLS key path — verify startup fails with descriptive error for each

---

## Phase 2: Production Hardening

- [x] **12. Add pagination to all list endpoints**
  - `list_rooms`, `list_members`, `list_tenants`, `list_api_tokens`, `list_connections` — all currently return full result sets
  - Add `?limit=&offset=` (or cursor-based) with defaults (50) and max (500)
  - Test: create 100 rooms, list with `limit=10` — verify 10 returned with pagination metadata. Verify default limit applies when no param given.

- [x] **13. Add request ID tracing middleware**
  - Generate UUID per HTTP request, include in all log entries and response headers (`X-Request-Id`)
  - Propagate through WebSocket connection context
  - Test: send HTTP request, verify `X-Request-Id` header in response. Check server logs contain matching request ID.

- [x] **14. Structured JSON logging**
  - Replace default `tracing_subscriber::fmt()` with JSON output layer
  - Include: timestamp, level, request_id, tenant_id, user_id, message
  - Test: start Herald, send a request, capture stdout — verify output is valid JSON with expected fields

- [x] **15. Tenant cache TTL/invalidation**
  - Add TTL to tenant cache entries in `state.rs` (e.g., 5 minutes)
  - Invalidate cache entry on tenant update/delete via admin API
  - Test: create tenant, cache it via JWT auth, update tenant's JWT secret via admin API, verify old JWT is rejected within TTL window (not accepted until restart)

- [x] **16. Separate liveness/readiness health probes**
  - Add `GET /health/live` (always 200 if process is running) and `GET /health/ready` (checks storage, optional engine health)
  - Test: start Herald, verify `/health/live` returns 200. Verify `/health/ready` returns 200 when storage is healthy and 503 when degraded.

- [x] **17. Typing indicator TTL + cleanup on disconnect**
  - Add server-side typing state with 10s TTL auto-expiry
  - On disconnect, broadcast `typing.stop` for all rooms the user was typing in
  - Test: connect user, send `typing.start`, disconnect without `typing.stop` — verify other clients receive `typing.stop` within TTL

- [x] **18. Reconnect catchup gap detection**
  - When catchup returns `CATCHUP_LIMIT` messages, set `has_more: true` in the batch response
  - Client can then fetch remaining via `messages.fetch`
  - Test: send 250 messages to a room, reconnect with old `last_seen_at` — verify `messages.batch` includes `has_more: true` and only 200 messages

- [x] **19. Security headers middleware**
  - Add `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`, `Referrer-Policy: no-referrer`
  - Add `Strict-Transport-Security` when TLS is configured
  - Test: send HTTP request, verify all security headers present in response

- [x] **20. Graceful shutdown improvements**
  - Drain period: stop accepting new connections, let in-flight requests complete
  - Complete in-flight webhook deliveries before exit
  - Send close frame to active WebSocket connections
  - Test: start Herald, establish WS connection, send SIGTERM — verify WS client receives close frame, in-flight HTTP request completes

---

## Phase 3: Feature Completeness (deferred — scoping to Herald vs Herald Chat TBD)

> Items 21-26 are chat-specific features that may belong in a separate Herald Chat product layer (similar to Pusher Channels vs Beams). Items 27-29 are transport-level concerns that stay in Herald. All deferred pending product scoping decision.

- [ ] **21. Message editing** *(chat-specific — likely Herald Chat)*
  - WS: `message.edit` client→server, `message.edited` server→client fan-out
  - HTTP: `PATCH /rooms/:id/messages/:msg_id`
  - Store edit history (original preserved)
  - Only sender or admin/owner can edit
  - Test: send message, edit it, verify other clients receive `message.edited` with new body. Verify non-sender gets 403. Verify edited message returned correctly in history fetch.

- [x] **22. Message deletion/redaction** *(transport: compliance/moderation)*
  - WS: `message.delete` client→server, `message.deleted` server→client fan-out
  - HTTP: `DELETE /rooms/:id/messages/:msg_id`
  - Soft delete (mark as deleted, remove body)
  - Only sender, admin, or owner can delete
  - Test: send message, delete it, verify other clients receive `message.deleted`. Verify deleted message body is `null` in history. Verify non-sender non-admin gets 403.

- [ ] **23. Thread/reply support** *(chat-specific — likely Herald Chat)*
  - Add optional `parent_id` field to messages
  - `messages.fetch` supports `?thread=<parent_id>` filter
  - Fan-out includes `parent_id` in `message.new`
  - Test: send message, reply to it with `parent_id`, verify reply appears in thread fetch. Verify `message.new` fan-out includes `parent_id`.

- [ ] **24. Reactions** *(chat-specific — likely Herald Chat)*
  - WS: `reaction.add` / `reaction.remove` client→server, `reaction.changed` server→client
  - HTTP: `POST /rooms/:id/messages/:msg_id/reactions`, `DELETE /rooms/:id/messages/:msg_id/reactions/:emoji`
  - Per-message reaction counts + user lists
  - Test: add reaction, verify other clients receive `reaction.changed`. Remove reaction, verify update. Verify reaction data included in message history.

- [ ] **25. File/media attachments** *(chat-specific — likely Herald Chat)*
  - Attachment URL model: messages carry `attachments` array in meta with `{url, content_type, size, name}`
  - HTTP: `POST /rooms/:id/upload` returns signed URL or stores reference
  - Validate content type and size limits
  - Test: send message with attachment metadata, verify it's stored and returned in history. Verify size limit enforcement.

- [ ] **26. User blocking/muting** *(chat-specific — likely Herald Chat)*
  - Per-user block list scoped to tenant
  - Blocked users' messages filtered from fan-out to blocker
  - HTTP: `POST /users/:uid/block`, `DELETE /users/:uid/block/:blocked_uid`
  - Test: user A blocks user B, B sends message — verify A does NOT receive it but C does. Unblock, verify messages resume.

- [x] **27. Room archival/readonly state** *(transport)*
  - `PATCH /rooms/:id` with `{archived: true}` → room becomes read-only
  - Reject `message.send` to archived rooms with clear error
  - Allow unarchiving
  - Test: archive room, attempt to send message — verify error. Verify history still readable. Unarchive, verify sends work again.

- [x] **28. Webhook event filtering** *(transport)*
  - Configure per-webhook which event types to receive
  - Config: `webhook.events = ["message.new", "member.joined"]` (default: all)
  - Test: configure webhook for `message.new` only, trigger `member.joined` — verify webhook NOT fired. Send message — verify webhook fires.

- [x] **29. API key scoping** *(transport)*
  - API tokens with optional scope: `read-only`, `room:<room_id>`, `admin`
  - Enforce scope on every HTTP endpoint
  - Test: create read-only token, attempt POST — verify 403. Create room-scoped token, access different room — verify 403.

---

## Phase 4: SDK & Testing

- [x] **30. Fix WS SDK dedup memory leak**
  - `herald-sdk-typescript/src/client.ts:282-287` — `seenMessageIds` Set truncation is broken (calls `iter.next()` but never removes)
  - Fix: proper Set truncation (convert to array, slice, rebuild)
  - Test: send 15k messages through SDK, verify Set size stays bounded at ~10k

- [x] **31. Fix WS SDK subscribe timeout**
  - Subscribe promise hangs forever if one room silently fails
  - Add per-room timeout (e.g., 10s) with rejection on timeout
  - Test: subscribe to existing + non-existent room, verify promise rejects within timeout instead of hanging

- [x] **32. Add `rooms.list()` to Go, Python, Ruby admin SDKs**
  - Implement `GET /rooms` in each SDK's room namespace
  - Test: create 3 rooms via admin SDK, call `rooms.list()`, verify all 3 returned

- [x] **33. Add tenant management to all admin SDKs**
  - Implement in all 4 admin SDKs (TS, Go, Python, Ruby):
    - `tenants.create()`, `tenants.list()`, `tenants.get()`, `tenants.update()`, `tenants.delete()`
    - `tenants.createToken()`, `tenants.listTokens()`, `tenants.deleteToken()`
  - Test: full CRUD lifecycle through each SDK against live Herald in multi-tenant mode

- [x] **34. Add observability endpoints to admin SDKs**
  - Implement in all 4 admin SDKs:
    - `admin.connections()`, `admin.events()`, `admin.errors()`, `admin.stats()`
  - Test: call each method, verify response structure matches server output

- [x] **35. Fix Python timeout and Ruby SSL verification**
  - Python: add `timeout=30` parameter to `urllib.request.urlopen()`
  - Ruby: add configurable SSL verification with `verify_mode = OpenSSL::SSL::VERIFY_PEER` default
  - Test: Python — connect to non-routable address, verify timeout raises within 30s. Ruby — connect to self-signed cert server, verify SSL error raised.

- [ ] **36. Write SDK test suites**
  - Each SDK gets an integration test suite running against a live Herald instance
  - Cover: auth, room CRUD, member CRUD, message send/list, presence, cursors, error handling
  - Minimum: 15 tests per admin SDK, 25 tests for WS SDK
  - Test: CI runs SDK tests against Herald started in test mode

- [x] **37. Add server-side security and authorization tests**
  - JWT: expired tokens, tampered signatures, `alg:none`, missing required claims
  - AuthZ: cross-room access, privilege escalation, non-member sends
  - Input: oversized payloads, malformed JSON, special characters
  - Error paths: store failures, connection loss, partial writes
  - Minimum: 30 new integration tests
  - Test: all run against live Herald with real WAL storage

- [x] **38. Add store layer unit tests and `cargo audit` to CI**
  - Unit tests for `store/cursors.rs`, `store/members.rs`, `store/messages.rs`, `store/rooms.rs`, `store/tenants.rs`
  - Add `cargo audit` step to `.github/workflows/ci.yml`
  - Add `cargo test --lib` to CI (currently only runs integration tests)
  - Add test coverage reporting
  - Minimum: 20 store unit tests
  - Test: CI pipeline passes with all new checks

- [x] **39. Update DOCS.md with all implemented endpoints**
  - Document all undocumented endpoints:
    - `DELETE /admin/tenants/{id}/tokens/{token}`
    - `GET /admin/tenants/{id}/rooms`
    - `GET /admin/connections`
    - `GET /admin/events`
    - `GET /admin/events/stream`
    - `GET /admin/errors`
    - `GET /admin/stats`
    - `GET /stats`
  - Document new features from Phase 3 as they land
  - Test: verify every documented endpoint returns expected response shape against live server

---

## Progress Summary

| Phase | Items | Completed | Remaining |
|-------|-------|-----------|-----------|
| 1. Security & Correctness | 11 | 11 | 0 |
| 2. Production Hardening | 9 | 9 | 0 |
| 3. Feature Completeness | 9 | 4 | 5 (deferred) |
| 4. SDK & Testing | 10 | 9 | 1 (item 36: full SDK test suites) |
| **Total** | **39** | **33** | **6** (5 deferred to Herald Chat, 1 SDK test suites) |
