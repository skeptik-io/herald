# Herald

> Persistent realtime event streams with built-in authorization. Multi-tenant, ShroudB Sentry ABAC. Event body is opaque — Herald is a transport+storage layer.

## Quick Context

- **Role**: Real-time event delivery. Browsers via WebSocket (:6200), backends via HTTP (:6201).
- **Not a ShroudB engine**: Standalone Rust project. Uses ShroudB crates for storage.
- **Storage**: ShroudB WAL engine (`shroudb-storage`). ~80us writes. No Postgres.
- **Multi-tenant**: `tenant` JWT claim. Per-tenant streams, members, JWT secrets. Admin API for tenant CRUD.
- **Opaque event body**: Herald stores and delivers event bodies as-is. Consumers handle their own encryption and search.
- **ShroudB Sentry**: ABAC authorization available as embedded (in-process) or remote (TCP with circuit breakers).

## Workspace Layout

```
herald/
├── herald-core/                     Domain types (no I/O)
├── herald-server/                   Rust binary — WS :6200 + HTTP :6201
├── herald-sdk-typescript/           Browser WebSocket client (npm: herald-sdk)
├── herald-admin-typescript/         Node.js HTTP admin  (npm: herald-admin)
├── herald-admin-go/                 Go HTTP admin
├── herald-admin-python/             Python HTTP admin
├── herald-admin-ruby/               Ruby HTTP admin
├── herald-admin-ui/                 Admin dashboard
├── ARCHITECTURE.md                  Full protocol specification
├── DOCS.md                          Configuration + API reference
└── CLAUDE.md                        Project rules
```

## Commands

```bash
cargo build --release
cargo test --workspace
cargo test --test bench_latency -- --nocapture --test-threads=1
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

TypeScript SDKs:

```bash
cd herald-sdk-typescript && npm run check       # tsc --noEmit
cd herald-admin-typescript && npm run check      # tsc --noEmit
```

---

## TypeScript SDK Guide

Two separate packages. **herald-sdk** is for browsers (WebSocket). **herald-admin** is for Node.js backends (HTTP). Never mix them up — browsers do not call the HTTP API directly, and backends do not open WebSocket connections via the SDK.

### herald-sdk-typescript (Browser WebSocket Client)

**Package**: `herald-sdk` v2.0.0
**Entry**: `herald-sdk-typescript/src/index.ts`
**Runtime**: Browser (uses native `WebSocket`)

#### Source Files

| File | Purpose |
|---|---|
| `src/client.ts` | `HeraldClient` — main class, all public methods |
| `src/connection.ts` | `Connection` — WebSocket lifecycle, reconnect, backoff |
| `src/types.ts` | All TypeScript interfaces and the `HeraldEventMap` |
| `src/errors.ts` | `HeraldError` class with `code` property |
| `src/e2ee.ts` | `E2EEManager` — per-stream encrypt/decrypt lifecycle |
| `src/crypto.ts` | WASM-backed E2EE primitives (lazy-loaded) |

#### Initialization

```typescript
import { HeraldClient } from "herald-sdk";

const client = new HeraldClient({
  url: "wss://herald.example.com/ws",   // or ws://localhost:6201/ws
  token: jwtFromBackend,                 // JWT with sub, tenant, streams, exp, iat, iss
  onTokenExpiring: async () => {         // called 60s before JWT exp
    return await fetchNewToken();
  },
  reconnect: { enabled: true, maxDelay: 30_000 },  // defaults
  e2ee: false,                           // opt-in E2EE
});

await client.connect();  // opens WebSocket + sends auth frame
```

**Constructor options** (`HeraldClientOptions` in `types.ts`):

| Field | Type | Default | Description |
|---|---|---|---|
| `url` | `string` | required | WebSocket URL |
| `token` | `string` | required | JWT from app backend |
| `onTokenExpiring` | `() => Promise<string>` | — | Return fresh JWT when server warns of expiry |
| `reconnect.enabled` | `boolean` | `true` | Auto-reconnect on disconnect |
| `reconnect.maxDelay` | `number` | `30000` | Max backoff delay (ms) |
| `e2ee` | `boolean` | `false` | Enable end-to-end encryption |

**Properties**:
- `client.connected: boolean` — true when WebSocket is open
- `client.connectionId: number | null` — server-assigned ID (available after auth)

#### Connection States

Defined in `connection.ts`:

```
initialized → connecting → connected → (disconnect) → connecting → connected
                                      → unavailable (after 4 failures, retries every 15s)
                                      → disconnected (intentional close)
                  → failed (WebSocket not supported)
```

Reconnect uses exponential backoff: 1s, 2s, 4s, 8s, 16s, max 30s, with random jitter (0.5x–1.5x). On reconnect, the client automatically re-authenticates and re-subscribes to all previously subscribed streams.

#### Subscribing to Streams

```typescript
const results = await client.subscribe(["stream_a", "stream_b"]);
// results: SubscribedPayload[] — one per stream

// SubscribedPayload:
// { stream, members: MemberPresence[], cursor: number, latest_seq: number }
// Unread count = latest_seq - cursor

client.unsubscribe(["stream_a"]);  // fire-and-forget, void return
```

`subscribe()` has a 10-second timeout. It waits for one `subscribed` server frame per stream. Throws `HeraldError("TIMEOUT", ...)` on timeout.

Only streams listed in the JWT `streams` claim are permitted. Server rejects others with `UNAUTHORIZED`.

#### Publishing Events

```typescript
const ack = await client.publish("stream_a", "Hello world", {
  meta: { reply_to: "msg_xyz", attachments: [] },
  parentId: "parent_msg_id",  // optional thread parent
});
// ack: { id: string, seq: number, sent_at: number }
```

`publish()` returns a promise that resolves with `EventAck` when the server confirms storage. 30-second timeout.

**Event body is opaque** — Herald stores and delivers it as-is. The SDK does not interpret, validate, or transform `body` or `meta`.

#### Editing and Deleting Events

```typescript
const ack = await client.editEvent("stream_a", "msg_abc", "Updated body");
const ack = await client.deleteEvent("stream_a", "msg_abc");
```

Both return `EventAck`. Edit broadcasts `event.edited` to subscribers. Delete broadcasts `event.deleted`.

#### Fetching History

```typescript
const batch = await client.fetch("stream_a", { before: 42, limit: 50 });
// batch: { stream, events: EventNew[], has_more: boolean }
```

Returns events with `seq < before`. Server caps `limit` at 100.

#### Reactions

```typescript
client.addReaction("stream_a", "msg_abc", "thumbsup");    // fire-and-forget
client.removeReaction("stream_a", "msg_abc", "thumbsup");  // fire-and-forget
```

All stream subscribers receive `reaction.changed` events.

#### Presence, Cursors, Typing

```typescript
client.setPresence("dnd");                    // "online" | "away" | "dnd"
client.updateCursor("stream_a", 42);          // fire-and-forget, no ack
client.startTyping("stream_a");               // auto-expires after 5s without renewal
client.stopTyping("stream_a");
```

#### Ephemeral Events

Not persisted. Not sequenced. Sender does NOT receive their own event.

```typescript
client.trigger("stream_a", "cursor_move", { x: 100, y: 200 });
```

Subscribers receive via `event.received` handler.

#### Event Listeners

Type-safe event system. All event types are in `HeraldEventMap` (`types.ts`).

```typescript
// Typed handlers
client.on("event", (msg: EventNew) => { /* new event */ });
client.on("event.edited", (msg: EventEdited) => { /* edited */ });
client.on("event.deleted", (msg: EventDeleted) => { /* deleted */ });
client.on("reaction.changed", (msg: ReactionChanged) => { /* reaction */ });
client.on("presence", (msg: PresenceChanged) => { /* presence change */ });
client.on("cursor", (msg: CursorMoved) => { /* cursor moved */ });
client.on("member.joined", (msg: MemberEvent) => { /* member added */ });
client.on("member.left", (msg: MemberEvent) => { /* member removed */ });
client.on("typing", (msg: TypingEvent) => { /* typing indicator */ });
client.on("stream.updated", (msg: StreamEvent) => { /* stream meta changed */ });
client.on("stream.deleted", (msg: StreamEvent) => { /* stream deleted */ });
client.on("stream.subscriber_count", (msg: StreamSubscriberCount) => { /* count */ });
client.on("event.received", (msg: EventReceived) => { /* ephemeral event */ });
client.on("watchlist.online", (msg: WatchlistEvent) => { /* users came online */ });
client.on("watchlist.offline", (msg: WatchlistEvent) => { /* users went offline */ });
client.on("connected", () => { /* WebSocket connected */ });
client.on("disconnected", () => { /* WebSocket disconnected */ });
client.on("reconnecting", () => { /* attempting reconnect */ });
client.on("error", (err: { code: string; message: string }) => { /* server error */ });

// Remove handler
client.off("event", handler);

// Global handler (all events)
client.onAny((event, data) => console.log(event, data));
client.offAny(handler);
```

#### Complete HeraldEventMap

```typescript
type HeraldEventMap = {
  "event":                    EventNew;
  "event.deleted":            EventDeleted;
  "event.edited":             EventEdited;
  "reaction.changed":         ReactionChanged;
  "event.received":           EventReceived;
  "presence":                 PresenceChanged;
  "cursor":                   CursorMoved;
  "member.joined":            MemberEvent;
  "member.left":              MemberEvent;
  "typing":                   TypingEvent;
  "stream.updated":           StreamEvent;
  "stream.deleted":           StreamEvent;
  "stream.subscriber_count":  StreamSubscriberCount;
  "watchlist.online":         WatchlistEvent;
  "watchlist.offline":        WatchlistEvent;
  "connected":                void;
  "disconnected":             void;
  "reconnecting":             void;
  "error":                    ErrorPayload;
};
```

#### Deduplication

The client deduplicates `event.new` frames by event `id`. It maintains a `Set<string>` capped at 10,000 entries (keeps most recent 5,000 when pruned). This handles reconnect catch-up duplicates. The `lastSeenAt` timestamp (from the most recent `event.new.sent_at`) is sent in the `auth` frame on reconnect so the server can replay missed events.

#### E2EE

Opt-in per-stream encryption using WASM (x25519 key exchange + AES-GCM cipher + blind tokens for searchable encryption).

```typescript
import { HeraldClient, initE2EE, generateKeyPair, deriveSharedSecret, createSession, restoreSession } from "herald-sdk";

const client = new HeraldClient({ url, token, e2ee: true });
await client.connect();  // lazily loads WASM

// Key exchange (app-level — Herald doesn't handle this)
const myKeys = generateKeyPair();
// Exchange myKeys.publicKey with peer out-of-band
const shared = deriveSharedSecret(myKeys.secretKey, peerPublicKey);
const session = createSession(shared);

// Assign session to a stream — all events encrypted/decrypted transparently
client.setE2EESession("stream_a", session);

// Persist keys for later
const { cipherKey, veilKey } = session.exportKeys();
// ... store in IndexedDB ...

// Restore on next page load
const restored = restoreSession(cipherKey, veilKey);
client.setE2EESession("stream_a", restored);

// Remove when done
client.removeE2EESession("stream_a");
```

E2EE is transparent — `publish()` auto-encrypts, `on("event", ...)` auto-decrypts. Blind tokens are stored in `meta.__blind` and stripped before reaching event handlers.

#### Error Handling

```typescript
import { HeraldError, ErrorCode } from "herald-sdk";

// ErrorCode constants:
// TOKEN_EXPIRED, TOKEN_INVALID, UNAUTHORIZED, NOT_SUBSCRIBED,
// STREAM_NOT_FOUND, RATE_LIMITED, BAD_REQUEST, INTERNAL

try {
  await client.publish("stream_a", "hello");
} catch (e) {
  if (e instanceof HeraldError) {
    console.log(e.code, e.message);  // e.g. "NOT_SUBSCRIBED: Not subscribed to stream_a"
  }
}
```

Pending requests (publish, fetch, subscribe) are rejected with `HeraldError("DISCONNECTED", ...)` when the connection drops.

#### Disconnect

```typescript
client.disconnect();
// Clears subscribed streams, E2EE sessions, pending requests.
// Connection state → "disconnected". No reconnect attempts.
```

---

### herald-admin-typescript (Node.js HTTP Admin Client)

**Package**: `herald-admin` v2.0.0
**Entry**: `herald-admin-typescript/src/index.ts`
**Runtime**: Node.js (uses native `fetch`)
**Dependencies**: Zero external dependencies

#### Source Files

| File | Purpose |
|---|---|
| `src/client.ts` | `HeraldAdmin` — main class with namespaced properties |
| `src/transport.ts` | `HttpTransport` — fetch wrapper with auth + error handling |
| `src/types.ts` | Shared TypeScript interfaces |
| `src/errors.ts` | `HeraldError` with `code` and `status` properties |
| `src/namespaces/rooms.ts` | `StreamNamespace` — stream CRUD |
| `src/namespaces/members.ts` | `MemberNamespace` — member management |
| `src/namespaces/messages.ts` | `EventNamespace` — event publish/list/delete/edit/reactions/trigger/search |
| `src/namespaces/presence.ts` | `PresenceNamespace` — presence + cursor queries |
| `src/namespaces/tenants.ts` | `TenantNamespace` — tenant CRUD + API token management |
| `src/namespaces/blocks.ts` | `BlockNamespace` — user blocking |

#### Initialization

```typescript
import { HeraldAdmin } from "herald-admin";

const admin = new HeraldAdmin({
  url: "http://localhost:6201",    // Herald HTTP API
  token: "api-token-from-config",  // must match auth.api.tokens in herald.toml
});
```

#### Namespaces

Access all functionality through namespaced properties:

```typescript
admin.streams   // StreamNamespace
admin.members   // MemberNamespace
admin.events    // EventNamespace
admin.presence  // PresenceNamespace
admin.tenants   // TenantNamespace  (requires super_admin_token for admin routes)
admin.blocks    // BlockNamespace
```

#### Streams (`admin.streams`)

```typescript
// Create
const stream = await admin.streams.create("stream_abc", "General Chat", {
  meta: { type: "group" },
  public: false,
});
// Returns: { id, name, meta, created_at, public?, archived? }

// List all
const streams = await admin.streams.list();

// Get one
const stream = await admin.streams.get("stream_abc");

// Update (name, meta, archived)
await admin.streams.update("stream_abc", { name: "New Name", archived: true });

// Delete (broadcasts stream.deleted, terminates subscriptions)
await admin.streams.delete("stream_abc");
```

#### Members (`admin.members`)

```typescript
// Add member (broadcasts member.joined)
const member = await admin.members.add("stream_abc", "user_3", "member");
// Returns: { stream_id, user_id, role, joined_at }

// List members
const members = await admin.members.list("stream_abc");

// Update role (broadcasts member.updated)
await admin.members.update("stream_abc", "user_3", "admin");

// Remove (broadcasts member.left, terminates subscriptions)
await admin.members.remove("stream_abc", "user_3");
```

Roles: `"owner"`, `"admin"`, `"member"`. Roles are informational — Herald does not enforce role-based permissions beyond stream membership.

#### Events (`admin.events`)

```typescript
// Inject event from backend (triggers full pipeline: store, fan-out, webhook)
const result = await admin.events.publish("stream_abc", "system", "Welcome!", {
  meta: { system: true },
  parentId: "parent_msg_id",
  excludeConnection: "12345",  // skip this WS connection in fan-out
});
// Returns: { id, seq, sent_at }

// List events (pagination via before/after/limit)
const list = await admin.events.list("stream_abc", {
  before: 100,
  after: 50,
  limit: 25,
  thread: "parent_msg_id",  // filter by thread
});
// Returns: { events: Event[], has_more: boolean }

// Edit event body
await admin.events.edit("stream_abc", "msg_abc", "Updated body");

// Delete/redact event (soft-delete: clears body, sets meta.deleted=true)
await admin.events.delete("stream_abc", "msg_abc");

// Get reactions for an event
const reactions = await admin.events.getReactions("stream_abc", "msg_abc");
// Returns: [{ emoji, count, users }]

// Trigger ephemeral event (not persisted, sender is _server)
await admin.events.trigger("stream_abc", "custom_event", { key: "value" }, excludeConnId);

// Search events
const results = await admin.events.search("stream_abc", "search query", 20);
```

#### Presence (`admin.presence`)

```typescript
// Get user presence
const presence = await admin.presence.getUser("user_1");
// Returns: { user_id, status, connections }

// Get all members' presence in a stream
const members = await admin.presence.getStream("stream_abc");
// Returns: [{ user_id, status }]

// Get read cursors for a stream
const cursors = await admin.presence.getCursors("stream_abc");
// Returns: [{ user_id, seq }]
```

#### Tenants (`admin.tenants`)

Requires `super_admin_token` (multi-tenant mode).

```typescript
// Create tenant
const tenant = await admin.tenants.create({
  id: "acme",
  name: "Acme Corp",
  jwt_secret: "tenant-hmac-secret",
  jwt_issuer: "acme-backend",
  plan: "pro",
});

// List / get / update / delete
const tenants = await admin.tenants.list();
const tenant = await admin.tenants.get("acme");
await admin.tenants.update("acme", { name: "Acme Inc", plan: "enterprise" });
await admin.tenants.delete("acme");

// API token management
const token = await admin.tenants.createToken("acme", "read-only");  // scope optional
const tokens = await admin.tenants.listTokens("acme");
await admin.tenants.deleteToken("acme", token.token);

// List streams for a tenant (admin view)
const streams = await admin.tenants.listTenantStreams("acme");
```

#### Blocks (`admin.blocks`)

```typescript
await admin.blocks.block("user_a", "user_b");     // A blocks B
await admin.blocks.unblock("user_a", "user_b");
const blocked = await admin.blocks.list("user_a"); // returns string[]
```

Blocking is directional and per-tenant. Herald does not enforce blocks at the delivery level — it provides the data for client-side filtering.

#### Health and Admin Endpoints

```typescript
const health = await admin.health();
// Returns: { status, connections, streams, uptime_secs }

const connections = await admin.connections();  // active WebSocket connections
const events = await admin.adminEvents({ limit: 50 });  // admin event log
const errors = await admin.errors({ limit: 20, category: "auth" });
const stats = await admin.stats();  // platform-wide stats
```

#### Error Handling

```typescript
import { HeraldError } from "herald-admin";

try {
  await admin.streams.get("nonexistent");
} catch (e) {
  if (e instanceof HeraldError) {
    console.log(e.code);     // "NOT_FOUND"
    console.log(e.status);   // 404
    console.log(e.message);  // "NOT_FOUND: Stream not found"
  }
}
```

The admin `HeraldError` has a `status: number` property (HTTP status code) that the browser SDK's `HeraldError` does not.

---

## Typical Integration Pattern

```
App Backend (Node.js)                    Browser
─────────────────────                    ───────
1. Create stream                         
   admin.streams.create(...)             
2. Add members                           
   admin.members.add(...)                
3. Mint JWT with streams claim           
   jwt.sign({ sub, tenant, streams })    
4. Return JWT to browser           →     5. Connect
                                          client = new HeraldClient({ url, token })
                                          await client.connect()
                                         6. Subscribe
                                          await client.subscribe(["stream_a"])
                                         7. Send/receive events
                                          client.on("event", handler)
                                          await client.publish("stream_a", "Hello")
8. Receive webhook                       
   POST /webhooks/herald                 
   Persist event in app DB               
```

## JWT Claims

Minted by the app backend, validated by Herald.

| Claim | Type | Required | Description |
|---|---|---|---|
| `sub` | string | yes | User ID |
| `tenant` | string | yes | Tenant ID |
| `streams` | string[] | yes | Stream IDs the user may subscribe to |
| `exp` | number | yes | Expiration (Unix seconds) |
| `iat` | number | yes | Issued-at |
| `iss` | string | no | Issuer (validated against tenant config) |
| `watchlist` | string[] | no | User IDs to track online/offline status |

## Event Pipeline

```
Client sends event.publish
  → Rate limit check (per-connection sliding window)
  → JWT tenant + streams claim check
  → Optional Sentry EVALUATE (embedded or remote)
  → WAL store (dual key: seq + ID index)        [event_store histogram]
  → Metrics counter increment
  → event.ack to sender
  → Fan-out event.new to stream subscribers      [event_fanout histogram]
  → Webhook POST (signed, async)
  → Chronicle INGEST audit event (async)
  → Courier NOTIFY offline members (async)
```

## Key Design Decisions

- **ShroudB WAL replaces Postgres** — ~80us writes. Single binary, no external database.
- **Event body is opaque** — Herald stores and delivers bodies as-is. Consumers handle their own encryption and search.
- **Multi-tenant via composite keys** — `{tenant_id}/{stream_id}` in every namespace.
- **Circuit breakers on all remote calls** — 5 failures -> open, 30s cooldown. Sentry fail-open.
- **Two SDKs, strict separation** — Browser SDK (WebSocket) never calls HTTP API. Admin SDK (HTTP) never opens WebSocket.
- **Zero external dependencies** in admin SDK — uses native `fetch`.
- **E2EE is opt-in and lazy** — WASM module loaded only when `e2ee: true`. Zero overhead otherwise.
- **Dedup is client-side** — Server may send duplicates on reconnect catch-up. Client deduplicates by event `id`.
