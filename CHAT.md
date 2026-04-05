# Herald Chat — Product Split & Implementation Plan

Herald splits into two products: **Herald** (event transport) and **Herald Chat** (conversational layer). Herald becomes a clean Pusher/Ably competitor for any real-time use case. Herald Chat adds the semantics that only matter when humans are talking to each other.

---

## Protocol Frame Split

### Herald Core (transport)

Client → Server:
- `auth`, `auth.refresh`
- `subscribe`, `unsubscribe`
- `event.publish`
- `events.fetch`
- `event.trigger` (ephemeral, not persisted)
- `ping`

Server → Client:
- `auth_ok`, `auth_error`
- `subscribed`
- `event.new`, `event.ack`, `events.batch`
- `event.received` (ephemeral)
- `member.joined`, `member.left`
- `stream.updated`, `stream.deleted`
- `stream.subscriber_count`
- `system.token_expiring`
- `watchlist.online`, `watchlist.offline`
- `error`, `pong`

### Herald Chat (extension)

Client → Server:
- `event.edit`, `event.delete`
- `cursor.update`
- `presence.set`
- `typing.start`, `typing.stop`
- `reaction.add`, `reaction.remove`

Server → Client:
- `event.edited`, `event.deleted`
- `cursor.moved`
- `presence.changed`
- `typing`
- `reaction.changed`

### Why this cut

Core = publish, subscribe, receive. An IoT dashboard, sports ticker, or collaborative document uses these frames and nothing else. No presence, no typing, no reactions in the API surface.

Chat = conversational semantics. Edit/delete (you don't edit a sensor reading). Cursors (you don't track read position on a stock ticker). Typing (meaningless outside conversation). Reactions (same). Presence with linger (a device is connected or not — linger is a human UX concern).

### Edge case: `parent_id`

`parent_id` stays as an optional field on `event.publish` in core. From the transport's perspective it's just metadata on the event envelope. Thread-filtered queries, thread grouping, and thread UI are chat concerns. Core doesn't interpret it; chat does.

---

## Server Architecture

### Extension trait

Core provides a hook point for extensions:

```rust
pub trait HeraldExtension: Send + Sync {
    fn register_ws_handlers(&self, registry: &mut HandlerRegistry);
    fn register_http_routes(&self, router: Router<AppState>) -> Router<AppState>;
    fn register_background_tasks(&self, state: &AppState) -> Vec<JoinHandle<()>>;
}
```

Chat implements it:

```rust
impl HeraldExtension for ChatExtension {
    fn register_ws_handlers(&self, registry: &mut HandlerRegistry) {
        registry.add("event.edit", handle_edit);
        registry.add("event.delete", handle_delete);
        registry.add("cursor.update", handle_cursor_update);
        registry.add("presence.set", handle_presence_set);
        registry.add("typing.start", handle_typing_start);
        registry.add("typing.stop", handle_typing_stop);
        registry.add("reaction.add", handle_reaction_add);
        registry.add("reaction.remove", handle_reaction_remove);
    }

    fn register_http_routes(&self, router: Router<AppState>) -> Router<AppState> {
        router
            .route("/streams/:id/events/:eid", patch(edit_event).delete(delete_event))
            .route("/streams/:id/events/:eid/reactions", get(get_reactions))
            .route("/streams/:id/cursors", get(list_cursors))
            .route("/streams/:id/presence", get(stream_presence))
            .route("/presence/:user_id", get(user_presence))
            .route("/blocks", post(block_user).delete(unblock_user))
            .route("/blocks/:user_id", get(list_blocked))
    }

    fn register_background_tasks(&self, state: &AppState) -> Vec<JoinHandle<()>> {
        vec![tokio::spawn(typing_expiry_loop(state))]
    }
}
```

### Enabling chat

Cargo feature flag (on by default):

```toml
# herald-server/Cargo.toml
[features]
default = ["chat"]
chat = ["herald-chat"]
```

Config flag (runtime):

```toml
# herald.toml
[extensions]
chat = true
```

An IoT user compiles with `default-features = false` or sets `chat = false` and never sees presence, typing, cursors, or reactions in their API surface.

### Core server modules (keep)

```
herald-server/src/
├── ws/
│   ├── connection.rs       # WebSocket lifecycle, auth, reader/writer
│   ├── handler.rs          # core frame dispatch only
│   ├── fanout.rs           # broadcast to stream subscribers
│   └── upgrade.rs          # HTTP → WS upgrade
├── registry/
│   ├── connection.rs       # ConnectionRegistry (connections, users, watchlists)
│   └── stream.rs           # StreamRegistry (members, subscribers, sequences)
├── store/
│   ├── events.rs           # insert, list_before, list_after, list_since_time, latest_seq
│   ├── members.rs          # member CRUD
│   ├── streams.rs          # stream CRUD
│   └── tenants.rs          # tenant + API token CRUD
├── http/
│   ├── streams.rs          # stream CRUD endpoints
│   ├── events.rs           # inject + list endpoints only
│   ├── members.rs          # member CRUD endpoints
│   ├── admin.rs            # tenant/token management, stats, SSE
│   └── health.rs           # health, liveness, readiness, metrics
```

### Chat extension modules (move out)

```
herald-chat/src/
├── ws/
│   └── handler.rs          # edit, delete, cursor, presence, typing, reaction handlers
├── registry/
│   ├── presence.rs         # PresenceTracker (manual overrides, linger, TTL)
│   └── typing.rs           # TypingTracker (per-stream, 10s auto-expiry)
├── store/
│   ├── events_ext.rs       # edit_event, delete_event (soft delete)
│   ├── cursors.rs          # upsert (max semantics), get, list_by_stream
│   ├── reactions.rs        # add, remove, list_for_event (aggregated)
│   └── blocks.rs           # block, unblock, is_blocked, list_blocked
├── http/
│   ├── events_ext.rs       # PATCH/DELETE event, GET reactions
│   ├── presence.rs         # stream presence, user presence
│   ├── cursors.rs          # list cursors
│   └── blocks.rs           # block/unblock/list
```

### Storage namespace split

**Core:**
- `herald.tenants` — tenant config
- `herald.api_tokens` — per-tenant API tokens
- `herald.streams` — stream definitions (`{tenant}/{stream}`)
- `herald.members` — membership (`{tenant}/{stream}/{user}`)
- `herald.events` — persisted events (`{tenant}/{stream}/{seq:020}` + ID index)

**Chat:**
- `herald.cursors` — read positions (`{tenant}/{stream}/{user}`)
- `herald.reactions` — emoji markers (`{tenant}/{stream}/{event}/{emoji}/{user}`)
- `herald.blocks` — user blocks (`{tenant}/{blocker}/{blocked}`)

---

## SDK Split

### `@anthropic/herald-sdk` (transport)

Keeps:
- `Connection` class (reconnect, backoff, dedup, state machine)
- `connect()`, `disconnect()`
- `subscribe()`, `unsubscribe()`
- `publish()`, `fetch()`, `trigger()`
- `on()`, `off()`, `onAny()` for core events
- Token refresh via `onTokenExpiring`
- E2EE (orthogonal to chat — useful for any encrypted stream)

Loses (moved to chat SDK):
- `editEvent()`, `deleteEvent()`
- `updateCursor()`, `setPresence()`
- `startTyping()`, `stopTyping()`
- `addReaction()`, `removeReaction()`

### `@anthropic/herald-chat-sdk` (chat transport extension)

Thin wrapper over the core client. Adds chat frame methods:

```typescript
import { HeraldClient } from '@anthropic/herald-sdk'

class HeraldChatClient {
  constructor(private client: HeraldClient)

  editEvent(stream: string, id: string, body: string): Promise<EventAck>
  deleteEvent(stream: string, id: string): Promise<EventAck>
  updateCursor(stream: string, seq: number): void
  setPresence(status: 'online' | 'away' | 'dnd'): void
  startTyping(stream: string): void
  stopTyping(stream: string): void
  addReaction(stream: string, eventId: string, emoji: string): void
  removeReaction(stream: string, eventId: string, emoji: string): void
}
```

Also re-exports chat-specific event types: `EventEdited`, `EventDeleted`, `CursorMoved`, `PresenceChanged`, `TypingEvent`, `ReactionChanged`.

### `@anthropic/herald-chat-core` (state machine — framework-agnostic)

No DOM, no framework. Pure state management.

```typescript
interface ChatCoreOptions {
  client: HeraldClient
  chat: HeraldChatClient
  liveness?: {
    idleTimeout?: number        // ms before idle → away (default: 300_000)
    backgroundAway?: boolean    // visibilitychange:hidden → away (default: true)
  }
}

class ChatCore {
  constructor(options: ChatCoreOptions)

  // --- Stream state ---
  getMessages(stream: string): Message[]
  getMembers(stream: string): MemberWithPresence[]
  getTyping(stream: string): string[]
  getUnreadCount(stream: string): number

  // --- Actions (optimistic) ---
  send(stream: string, body: string, opts?: { meta?, parentId? }): PendingMessage
  edit(stream: string, id: string, body: string): void
  delete(stream: string, id: string): void
  react(stream: string, eventId: string, emoji: string): void
  unreact(stream: string, eventId: string, emoji: string): void
  markRead(stream: string): void

  // --- Pagination ---
  loadMore(stream: string): Promise<{ hasMore: boolean }>

  // --- Liveness ---
  attach(document: Document): void
  detach(): void
  readonly isActive: boolean
  readonly isVisible: boolean

  // --- Scroll coordination ---
  setAtLiveEdge(stream: string, value: boolean): void
  getAtLiveEdge(stream: string): boolean
  getPendingCount(stream: string): number
  clearPending(stream: string): void

  // --- Change notification ---
  onChange(stream: string, callback: () => void): () => void
  onStreamListChange(callback: () => void): () => void
}
```

#### Message state

```typescript
interface Message {
  id: string
  seq: number
  sender: string
  body: string
  meta?: unknown
  parentId?: string
  sentAt: number
  editedAt?: number
  reactions: Map<string, Set<string>>   // emoji → user_ids
  status: 'sending' | 'sent' | 'failed'
}

interface PendingMessage {
  localId: string
  status: 'sending' | 'sent' | 'failed'
  retry(): void
  cancel(): void
}
```

#### Optimistic sends

`send()` returns a `PendingMessage` with a local ID. It appears in `getMessages()` immediately with `status: 'sending'`. On `event.ack`, it flips to `status: 'sent'` and gets the real `id`/`seq`. On error, `status: 'failed'`. The view renders whatever `getMessages()` returns.

#### Message accumulation

`event.new`, `event.edited`, `event.deleted` are applied to the internal store automatically. `getMessages()` always returns current truth. Edits replace body in-place. Deletes remove or tombstone (configurable).

#### Liveness state machine

`attach(document)` binds three signals to derive presence:

```
visible + active     →  online
visible + idle       →  away   (walked away from desk)
hidden               →  away   (switched tabs, backgrounded mobile browser)
websocket closed     →  offline (Herald core handles this already)
```

Input signals:
- `visibilitychange` → immediate `away` on hidden, resume check on visible
- `mousemove`, `keydown`, `touchstart`, `scroll` → throttled to 1/sec, resets idle timer
- Idle timer (default 5 min) → `setPresence('away')`

On activity resume or visibility return → `setPresence('online')`.
Manual `setPresence('dnd')` is respected — activity detection does not override it.

Fixes:
- **Laptop open, shell closed**: idle timer fires → `setPresence('away')`
- **Mobile backgrounded**: `visibilitychange` to hidden → `setPresence('away')` immediately

#### Scroll coordination

The framework shell owns the IntersectionObserver and calls `setAtLiveEdge()`. ChatCore uses this to:
- Auto-update cursor (mark as read) when at live edge + visible + active
- Increment `pendingCount` when not at live edge
- The shell reads `getPendingCount()` to render the "N new messages" pill

#### Change notification

`onChange(stream, callback)` is the bridge to framework reactivity. React hooks use `useSyncExternalStore` with it. Vue composables wrap it in a `watchEffect`. Fires when any state for that stream changes: new message, edit, delete, presence, typing, cursor.

### `@anthropic/herald-chat-react` (React bindings)

#### Provider

```tsx
<HeraldProvider
  client={client}
  chat={chatClient}
  liveness={{ idleTimeout: 300_000 }}
>
  {children}
</HeraldProvider>
```

Holds `ChatCore` instance. Handles `attach(document)` on mount, `detach()` on unmount.

#### Hooks

```typescript
useMessages(streamId: string)
  → { messages: Message[], send, edit, delete, loadMore, hasMore, loading }

useMembers(streamId: string)
  → MemberWithPresence[]

useTyping(streamId: string)
  → { typing: string[], sendTyping: () => void }

useUnreadCount(streamId: string)
  → number

usePresence(userId: string)
  → 'online' | 'away' | 'dnd' | 'offline'

useLiveness()
  → { isActive: boolean, isVisible: boolean }
```

All hooks are thin wrappers: `useSyncExternalStore` + the corresponding `ChatCore` getter + `onChange`.

#### Chat shell (headless, render-prop)

```tsx
<ChatShell streamId={streamId}>
  {({
    messages,           // Message[]
    pendingCount,       // number — new messages while scrolled up
    atLiveEdge,         // boolean
    scrollToBottom,     // () => void
    sentinelRef,        // ref — attach to top element for load-more trigger
    liveEdgeRef,        // ref — attach to bottom element for live-edge detection
  }) => (
    // Your views. You own every pixel.
    // ChatShell handles:
    //   - Scroll anchoring on history prepend (no jump)
    //   - IntersectionObserver on sentinelRef for load-more
    //   - IntersectionObserver on liveEdgeRef for at-bottom detection
    //   - pendingCount tracking when scrolled up
    //   - scrollToBottom with smooth scroll
  )}
</ChatShell>
```

#### Notification shell (headless)

```tsx
<NotificationShell streamIds={streamIds}>
  {({
    totalUnread,        // number — sum of unread across all streams
    streams,            // { streamId, unreadCount, lastMessage, lastActivity }[]
    onlineContacts,     // string[] — from watchlist
  }) => (
    // Your notification badge, stream list, contact indicators.
    // NotificationShell handles:
    //   - Unread aggregation across streams
    //   - Stream sorting by last activity
    //   - Watchlist presence tracking
  )}
</NotificationShell>
```

---

## Package Dependency Graph

```
@anthropic/herald-sdk                  (transport)
        │
        ├── @anthropic/herald-chat-sdk (chat frames)
        │         │
        │         ├── @anthropic/herald-chat-core   (state machine)
        │         │         │
        │         │         ├── @anthropic/herald-chat-react   (hooks + shells)
        │         │         ├── @anthropic/herald-chat-vue     (composables, future)
        │         │         └── @anthropic/herald-chat-svelte  (stores, future)
        │         │
        │         └── @anthropic/herald-admin        (HTTP admin, unchanged)
        │
        └── (standalone use for IoT, dashboards, etc.)
```

---

## Order of Work

### Phase 1: Fix the immediate pain

Build `herald-chat-core` and `herald-chat-react` on top of the **existing** SDK. No server changes. The current `herald-sdk-typescript` already has all the methods (`editEvent`, `setPresence`, etc.) — chat-core wraps them with state management, liveness, and scroll coordination.

Deliverables:
- `herald-chat-core` — message store, optimistic sends, liveness state machine, scroll coordination, unread counts
- `herald-chat-react` — provider, hooks, ChatShell, NotificationShell

After this phase: adding chat to a new app = drop in provider + shells, write views only. Liveness, scroll anchoring, and tab visibility are solved once.

### Phase 2: Server extension trait

Refactor `herald-server` to make the handler dispatch pluggable. Extract chat handlers, presence tracker, typing tracker, and chat storage into `herald-chat` crate. Core compiles and runs without chat.

Deliverables:
- `HeraldExtension` trait in core
- `herald-chat` server crate implementing the trait
- `chat` feature flag (default on)
- Clean API surface for non-chat users

### Phase 3: SDK split

Move chat frame methods out of `herald-sdk-typescript` into `herald-chat-sdk-typescript`. Core SDK becomes a pure transport client. Chat SDK wraps it.

Deliverables:
- `@anthropic/herald-sdk` — transport only
- `@anthropic/herald-chat-sdk` — chat frames
- Update `herald-chat-core` to depend on both

### Phase 4: Admin SDK alignment

Update admin SDKs (TypeScript, Go, Python, Ruby) to reflect the split. Chat-specific endpoints (presence queries, cursor queries, blocking) move to separate admin modules or namespaces.

---

## What the split solves

**For non-chat users:** Herald becomes a focused event transport. No chat concepts in the API surface. Clear Pusher/Ably competitor positioning.

**For chat users:** Herald Chat becomes a full conversational layer with framework bindings. Liveness, scroll, tab visibility, optimistic updates, unread counts — solved once, never reinvented.

**For you specifically:** Adding chat to a new bespoke app = provider + shells + your views. The three hard behavioral problems (scroll anchoring, tab visibility, liveness-driven presence) live in `herald-chat-core` and `herald-chat-react`. They're written once and shared across every app.
