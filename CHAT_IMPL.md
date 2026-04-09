# Herald Chat — Implementation Reference

Covers the current state of all three chat packages plus planned work (N-4, N-5).

---

## Package Structure

```
herald-chat-sdk-typescript/   WS client wrapper — chat-specific frame types
herald-chat-core/             Framework-agnostic state machine and stores
herald-chat-react/            React bindings (useSyncExternalStore + render-prop components)
```

**Dependency chain:** `herald-sdk` → `herald-chat-sdk` → `herald-chat-core` → `herald-chat-react`

All packages are ESM, peer-depend on each other, and target React 18+19.

---

## Architecture

```
Transport          State                   UI
─────────          ─────                   ──
HeraldClient  ──►  ChatCore  ──►  Notifier  ──►  useSyncExternalStore
     │               │                              │
HeraldChatClient     ├── MessageStore               ├── useMessages
  (frame types)      ├── CursorStore                ├── useTyping
                     ├── MemberStore                ├── useMembers
                     ├── TypingStore                ├── useUnreadCount
                     ├── ScrollState                ├── useLiveness
                     └── LivenessController         └── usePresence
```

Events flow one direction: transport → ChatCore handler → store mutation → Notifier.notify(slice) → React re-render. Reads go directly to stores via ChatCore getters.

---

## ChatCore

### Constructor

```typescript
interface ChatCoreOptions {
  client: HeraldClient;
  chat: HeraldChatClient;
  userId: string;
  liveness?: LivenessConfig;         // Browser activity tracking
  scrollIdleMs?: number;             // (N-4) Debounce delay for auto-mark-read. Default 1000ms
  loadMoreLimit?: number;            // (N-4) Pagination batch size. Default 50
  middleware?: Array<Middleware>;     // (N-5) Event interception chain
}
```

### Lifecycle

| Method | What it does |
|---|---|
| `attach()` | Registers 12 event handlers on HeraldClient. Starts typing expiry interval (1s). Attaches liveness controller. Idempotent. |
| `detach()` | Deregisters all handlers. Stops typing expiry. Detaches liveness. Idempotent. Does **not** clear stores — state persists for re-attach. |
| `destroy()` | Calls detach(), then clears all stores and notifier listeners. **One-way** — instance cannot be reused after destroy. |

**Session lifecycle:** The React Provider calls `attach()` on mount and `detach()` on unmount. It does **not** call `destroy()`, so stores persist across React re-renders and StrictMode double-mounts. For session changes (logout → login with different user), the app must call `destroy()` and create a new ChatCore instance. The Provider does not handle this automatically — the app should unmount the Provider, discard the client/chat objects, and remount with new credentials.

`attach()` registers handlers for: `event`, `event.edited`, `event.deleted`, `reaction.changed`, `presence`, `cursor`, `typing`, `member.joined`, `member.left`, `event.received`, `connected`, `disconnected`.

### Stream Management

```typescript
joinStream(streamId): Promise<SubscribedPayload>
```
Subscribes via `client.subscribe([streamId])`. Initializes cursor state (my cursor + latest seq), sets initial member list, creates ScrollState. Returns subscription payload.

```typescript
leaveStream(streamId): void
```
Unsubscribes and clears all state for the stream (messages, cursors, members, typing, scroll).

```typescript
listen(streamId): Promise<void>
```
Lightweight subscription — subscribes via transport, initializes cursor tracking (for unread counts), but skips MessageStore, MemberStore, TypingStore, and ScrollState. Events flow through middleware and Notifier slices. Useful for inbox/notification rooms where you need unread badges and event notifications without full chat state.

```typescript
unlisten(streamId): void
```
Unsubscribes a lightweight stream. Clears cursor and ephemeral state.

**What `listen()` provides:** Unread counts via `getUnreadCount(streamId)` and `getTotalUnreadCount()`. Raw event notifications via `event:{streamId}` and `ephemeral:{streamId}` slices. Middleware interception. **What it does not provide:** Message history, member lists, typing indicators, scroll state. For those, use `joinStream()`.

### Actions

| Method | Behavior |
|---|---|
| `send(streamId, body, opts?)` | Optimistic insert (seq=0, status=sending) → publish → reconcile on ack. Returns server event ID. Stores in `pendingSends` for retry. |
| `retrySend(localId)` | Removes failed optimistic, creates new one, re-publishes. |
| `cancelSend(localId)` | Removes optimistic from store and pendingSends. |
| `edit(streamId, eventId, body)` | Delegates to `chat.editEvent()`. No optimistic update. |
| `deleteEvent(streamId, eventId)` | Delegates to `chat.deleteEvent()`. No optimistic update. |
| `addReaction(streamId, eventId, emoji)` | Fire-and-forget via `chat.addReaction()`. |
| `removeReaction(streamId, eventId, emoji)` | Fire-and-forget via `chat.removeReaction()`. |
| `startTyping(streamId)` / `stopTyping(streamId)` | Send typing frames. |

**Return type:** `send()` returns `Promise<PendingMessage>`. PendingMessage exposes `localId`, `status`, `retry()`, `cancel()` — wrapping the internal machinery.

### Reads

| Method | Returns |
|---|---|
| `getMessages(streamId)` | `Message[]` — sorted by seq ascending, optimistic at end |
| `getMembers(streamId)` | `Member[]` — with presence status |
| `getTypingUsers(streamId)` | `string[]` — user IDs currently typing |
| `getUnreadCount(streamId)` | `number` — `max(0, latestSeq - myCursor)` |
| `getTotalUnreadCount()` | `number` — sum across all streams |
| `getScrollState(streamId)` | `ScrollStateSnapshot` — `{atLiveEdge, pendingCount, isLoadingMore}` |
| `getLivenessState()` | `"active" \| "idle" \| "hidden"` |
| `getLastEphemeral(streamId, eventType?)` | `EventReceived \| undefined` — last ephemeral event on this stream, optionally filtered by event type |
| `getRemoteCursors(streamId)` | `Map<string, number>` — remote user cursor positions (userId → seq) |

All array-returning methods produce new references on mutation (required for `useSyncExternalStore`).

### Scroll Coordination

```typescript
setAtLiveEdge(streamId, atEdge): void
```
When `atEdge` becomes true, fires `autoMarkRead()`. Clears pending count.

```typescript
loadMore(streamId): Promise<boolean>
```
Returns false if already loading or no history. Fetches `{before: oldestSeq, limit: 50}` via `client.fetch()`. Prepends to message store. Returns `has_more`.

**Change (N-4):** Limit will read from `this.loadMoreLimit` (constructor option, default 50) instead of hardcoded.

### Subscription

```typescript
subscribe(slice: string, listener: () => void): () => void
```
Delegates to Notifier. Returns unsubscribe function.

**Slice naming:** `messages:{streamId}`, `typing:{streamId}`, `members:{streamId}`, `unread:{streamId}`, `unread:total`, `scroll:{streamId}`, `liveness`, `ephemeral:{streamId}`, `event:{streamId}`.

---

## Event Handlers

### Pipeline

```
HeraldClient event → middleware chain → next() → store mutation → Notifier.notify(slice)
```

| SDK Event | Handler | Store Effect |
|---|---|---|
| `event` | `handleEvent` | `messages.appendEvent()` (binary search insert), `cursors.bumpLatestSeq()`, auto-mark-read if at edge. Emits `event:{streamId}` |
| `event.edited` | `handleEdited` | `messages.applyEdit()` — updates body + editedAt. Emits `event:{streamId}` |
| `event.deleted` | `handleDeleted` | `messages.applyDelete()` — sets deleted=true, clears body. Emits `event:{streamId}` |
| `reaction.changed` | `handleReaction` | `messages.applyReaction()` — add/remove user from emoji set. Emits `event:{streamId}` |
| `presence` | `handlePresence` | `members.updatePresence()` across all streams containing user |
| `cursor` | `handleCursor` | `cursors.updateRemoteCursor()` — tracks remote cursor positions. Flips self-sent messages to `status: "read"` when remote cursor advances past their seq |
| `typing` | `handleTyping` | `typing.setTyping()` — skips own events, TTL 12s |
| `member.joined` | `handleMemberJoined` | `members.addMember()` — idempotent, presence=online |
| `member.left` | `handleMemberLeft` | `members.removeMember()` |
| `event.delivered` | `handleDelivered` | Flips self-sent messages from `sent` to `delivered` when a remote subscriber acks delivery (ack-mode streams only) |
| `event.received` | `handleEphemeral` | Stores per event type in `lastEphemeral` map, emits `ephemeral:{streamId}` |
| `connected` | `handleConnected` | Documents that SDK re-subscribes automatically |
| `disconnected` | `handleDisconnected` | `typing.clearAll()` + restart expiry timer |

**Middleware signature:** `(event: ChatEvent, next: () => void) => void`

- Code before `next()` runs before store update (enrichment, transformation, filtering)
- `next()` applies the store mutation synchronously
- Code after `next()` runs after state is updated (side-effects)
- Skipping `next()` drops the event entirely
- `connected`/`disconnected` are internal lifecycle events — not routed through middleware
- After-store reactions that don't need to intercept events use the existing `subscribe()` pattern

**Middleware is synchronous by design.** `next()` applies the store mutation synchronously, and React's `useSyncExternalStore` requires that store updates are synchronous within a single microtask. Async middleware (fetch display names, decrypt, call external services) would break the contract: the store update would be deferred, causing tearing between subscribe and getSnapshot. For async enrichment, do the async work outside the middleware chain (e.g. in a `subscribe()` callback that updates a separate store) or pre-process data before it enters ChatCore.

**`event:{streamId}` slice:** Fires on every event-type handler (`event`, `event.edited`, `event.deleted`, `reaction.changed`) for both full-state and listen-only streams. Not consumed by any built-in hook. Intended for consumers that need a raw "something happened on this stream" signal — e.g. updating a sidebar preview, triggering a notification toast, or driving middleware after-effects via the Notifier rather than inside the middleware chain itself.

**Listen-only streams:** For streams opened via `listen(streamId)`, all handlers skip store mutations but still run middleware and emit Notifier slices (`event:{streamId}`, `typing:{streamId}`, `members:{streamId}`, `ephemeral:{streamId}`). Consumer subscribes to slices directly for routing.

---

## Stores

### MessageStore

**Key:** `Map<string, Message[]>` — per-stream, sorted ascending by seq.

**Indices:** `byId` (server event ID → Message), `byLocalId` (local ID → Message).

**Ordering:** Real messages sorted by seq via binary search insertion. Optimistic messages (seq=0) always at end in insertion order.

**Mutations:**

| Method | Behavior |
|---|---|
| `appendEvent(event)` | Binary search insert by seq. Dedup via `byId`. Returns false if duplicate. |
| `prependBatch(streamId, events, hasMore)` | Appends + re-sorts. Sets `_hasMore` flag. |
| `addOptimistic(...)` | Creates Message with seq=0, status=sending. Appends to end. |
| `reconcile(localId, ack)` | Updates optimistic with server data (id, seq, sentAt, status=sent). Re-sorts. Handles race: if event.new already arrived, removes optimistic. |
| `failOptimistic(localId)` | Sets status=failed. Message stays in list for retry. |
| `removeOptimistic(localId)` | Removes from list entirely. |
| `applyEdit(edit)` | Updates body + editedAt on existing message. |
| `applyDelete(del)` | Sets deleted=true, clears body. Message stays as tombstone. |
| `applyReaction(reaction)` | Adds/removes userId from emoji Set on the message's reactions Map. |
| `markDelivered(streamId, upToSeq, sender)` | Walks messages, flips self-sent messages with `status === "sent"` and seq <= upToSeq to `delivered`. |
| `markRead(streamId, upToSeq, sender)` | Walks messages, flips self-sent messages with `status === "sent"` or `"delivered"` and seq <= upToSeq to `read`. |

### CursorStore

**Key:** `myCursor: Map<string, number>`, `latestSeq: Map<string, number>`, `remoteCursors: Map<string, Map<string, number>>`.

**Semantics:** MAX-only — cursors never go backward. `updateMyCursor(streamId, seq)` and `updateRemoteCursor(streamId, userId, seq)` ignore if seq <= current.

**Unread calculation:** `max(0, latestSeq - myCursor)`.

**Remote cursors:** Tracks other users' cursor positions per stream. When a remote cursor advances past a self-sent message's seq, that message's status flips to `read`.

**Status progression:** `sending → sent → delivered → read`. The `delivered` state means "at least one remote subscriber's ack-mode client has received this event" — triggered by `event.delivered` frames from the server. Only fires on ack-mode streams; non-ack streams skip directly to `read` on cursor advance. **Read semantics:** `status = "read"` means "at least one remote user's cursor >= this message's seq" (WhatsApp-style). This is intentional — it answers "has anyone seen this?" which is the useful signal for both 1:1 and group conversations. The `Message.status` field is a convenience enum, not a per-user tracker. For per-user granularity (e.g. "read by 3 of 5 members" or a read-receipts UI showing who read what), consumers derive it from `getRemoteCursors(streamId)` at render time — no `readBy: Set<string>` on Message because the same information is already in the cursor map.

**Delivery receipts** are powered by Herald's ack mode (N-2). When a subscriber's ack-mode client receives events, the server broadcasts `event.delivered { stream, user_id, seq }` to other subscribers. ChatCore flips self-sent messages from `sent` → `delivered`. On non-ack streams, no delivery receipts are generated — status progresses directly from `sent` to `read` on cursor advance.

### MemberStore

**Key:** `Map<string, Member[]>` — per-stream.

**Member:** `{userId, role, presence}`. Presence is `"online" | "away" | "dnd"`.

`streamsForUser(userId)` returns all streams containing a user — used by presence handler to fan out presence changes across streams. O(streams × members) — fine for typical usage, would need an index if a user is in hundreds of streams.

### TypingStore

**Key:** `Map<string, Map<string, number>>` — per-stream map of userId → timestamp.

**TTL:** 8s client-side (expires before server's 10s to avoid stale indicators). Expiry check runs every 1s via `setInterval`.

**Optimization:** Only notifies on actual list changes, not timestamp refreshes. Compares old vs new snapshot array before firing.

### ScrollState

Per-stream scroll coordination: `{atLiveEdge, pendingCount, isLoadingMore}`.

- `pendingCount` increments when events arrive while scrolled up (not at live edge). This is a presentation convenience — the canonical message data lives in MessageStore, and `pendingCount` saves every consumer from independently tracking "how many arrived since I scrolled away." UIs that want different behavior (auto-scroll, batch notifications) can ignore it and derive from MessageStore directly.
- `setAtLiveEdge(true)` resets pending count
- `setLoadingMore` gates concurrent `loadMore()` calls

**`loadMore` dual signals:** `loadMore()` returns `boolean` (has_more) as an imperative result and also sets `isLoadingMore` on ScrollState reactively. Use the return value for awaiting in imperative code; use the reactive state for rendering loading indicators via `useSyncExternalStore`.

---

## Liveness System

Browser activity tracking → automatic presence.

**State machine:**
```
active ←─(idle timeout)──► idle
  │                          │
  └──(tab hidden)──► hidden ◄┘
         │
         └──(tab visible)──► active
```

**Activity events:** mousemove, mousedown, keydown, touchstart, scroll (all passive).

**Config:** `idleTimeoutMs` (default 120s), `throttleMs` (default 5s).

**Output:** Calls `syncPresence(state)` on transition. Maps: active → `online`, idle/hidden → `away`. Sends `presence.set` frame.

**Abstraction:** `LivenessEnvironment` interface wraps DOM APIs — fully testable without a browser.

---

## Reconnection and Gap-Fill

Herald SDK handles reconnection at the transport layer. On disconnect, the SDK reconnects automatically and re-subscribes to all streams. The server replays events missed during the gap using cursor-based catchup (implemented in M-1): the client sends its last known seq per stream, and the server delivers all events since that point with pagination.

ChatCore's role during reconnection:
- `handleDisconnected()` clears typing indicators (ephemeral state is stale after disconnect)
- `handleConnected()` is a no-op — the SDK handles re-subscription and catchup
- `MessageStore.appendEvent()` deduplicates via `byId` (event ID), so replayed events that already exist are safely ignored
- `CursorStore` uses MAX semantics, so replayed cursor positions cannot regress

**Ordering:** Events may arrive out of order during catchup replay. `appendEvent` uses binary search insertion by seq, so messages are always stored in correct order regardless of arrival order. The `byId` dedup guard prevents double-insertion.

**Idempotency:** All event handlers are idempotent. `appendEvent` returns false for duplicates. `applyEdit`, `applyDelete`, `applyReaction` operate on existing messages by ID — applying the same edit twice is harmless. Cursor MAX semantics prevent regression.

**Presence on reconnect:** Presence is ephemeral state, not event-sourced — it is not included in cursor-based catchup. When the SDK re-subscribes after a reconnect, the server's `subscribed` payload includes the current member list with each member's current presence status. ChatCore's `joinStream` calls `members.setMembers()` with this payload, which overwrites the stale local state. For listen-only streams (`listen()`), member state is not tracked, so presence changes during disconnection are not visible. If the app needs authoritative presence after a gap, it should re-join the stream or query presence via the admin API.

---

## Cross-Stream Ordering

Sequence numbers (`seq`) are per-stream — there is no global ordering across streams. Consumers that need a unified view (e.g. "latest message across all my rooms, sorted by time") must use `sentAt` timestamps. During optimistic sends, `sentAt` is client-estimated (`Date.now()`); after reconciliation, it carries the server-assigned timestamp.

The `listen()` feature provides per-stream unread counts but no cross-stream ordering primitive. A unified inbox with time-sorted previews is an app-level concern — derive it from `getMessages(streamId)` across streams, sorting by `sentAt`.

---

## Tombstones

`applyDelete` keeps deleted messages as tombstones (`deleted: true`, `body: ""`). This allows the UI to render "this message was deleted" placeholders. Tombstones are not pruned by ChatCore — they are bounded by the server's event TTL (per-tenant retention, default 7 days). When a client fetches history via `loadMore()`, expired events are already pruned server-side, so tombstones naturally age out of the client's view.

---

## HeraldChatClient (SDK Wrapper)

Bridges HeraldClient to chat-specific WS frame types.

| Method | Frame | Response |
|---|---|---|
| `editEvent(stream, id, body)` | `event.edit` | `EventAck` |
| `deleteEvent(stream, id)` | `event.delete` | `EventAck` |
| `updateCursor(stream, seq)` | `cursor.update` | fire-and-forget |
| `setPresence(status)` | `presence.set` | fire-and-forget |
| `startTyping(stream)` | `typing.start` | fire-and-forget |
| `stopTyping(stream)` | `typing.stop` | fire-and-forget |
| `addReaction(stream, eventId, emoji)` | `reaction.add` | fire-and-forget |
| `removeReaction(stream, eventId, emoji)` | `reaction.remove` | fire-and-forget |

E2EE: `editEvent` encrypts body if `e2eeManager` is set on the underlying client.

---

## React Bindings

### Provider

```tsx
<HeraldChatProvider
  client={heraldClient}
  chat={heraldChatClient}
  userId="alice"
  liveness={{ idleTimeoutMs: 120000 }}
  scrollIdleMs={1000}
>
  {children}
</HeraldChatProvider>
```

Lazy-initializes ChatCore in `useRef`. Calls `attach()` on mount, `detach()` on unmount. Does not call `destroy()` — stores persist across re-renders.

### Hooks

All hooks use `useSyncExternalStore` subscribing to Notifier slices.

| Hook | Slice | Returns |
|---|---|---|
| `useMessages(streamId)` | `messages:{streamId}` | `{messages, send, edit, deleteEvent, loadMore}` |
| `useTyping(streamId)` | `typing:{streamId}` | `{typing, sendTyping}` |
| `useMembers(streamId)` | `members:{streamId}` | `Member[]` |
| `useUnreadCount(streamId)` | `unread:{streamId}` | `number` |
| `useTotalUnreadCount()` | `unread:total` | `number` |
| `useLiveness()` | `liveness` | `"active" \| "idle" \| "hidden"` |
| `usePresence()` | `liveness` | `"online" \| "away"` (derived) |
| `useEphemeral(streamId, eventType?)` | `ephemeral:{streamId}` | `EventReceived \| undefined` |

### Components (Headless, Render-Prop)

All components accept `children: (state) => ReactNode`. Zero styling.

**HeraldChat** — Scroll coordination. Attaches scroll listener to `scrollRef`. Detects live edge (bottom threshold), triggers `loadMore` (top threshold), anchors scroll position when history is prepended.

```typescript
interface HeraldChatProps {
  streamId: string;
  scrollRef: RefObject<HTMLElement>;
  edgeThreshold?: number;       // default 50px
  loadMoreThreshold?: number;   // default 100px
  children: (state: { pendingCount, atLiveEdge, isLoadingMore, scrollToBottom }) => ReactNode;
}
```

**MessageList** — Composes `useMessages`. Passes `{messages, send, edit, deleteEvent, loadMore}`.

**MessageInput** — Composes `useMessages` + `useTyping`. Passes `{send, sendTyping}`.

**PresenceIndicator** — Composes `useMembers`. Derives `onlineCount`. Passes `{members, onlineCount}`.

**TypingIndicator** — Composes `useTyping`. Derives `isAnyoneTyping`. Passes `{typing, isAnyoneTyping}`.

---

## Optimistic Send Pipeline

```
send(streamId, body)
  │
  ├─► addOptimistic(localId, ...)        Message{seq:0, status:"sending"} appended
  │     └─► notify("messages:stream")    UI renders immediately
  │
  ├─► client.publish(streamId, body)     Network request
  │
  ├─► on success: reconcile(localId, ack)
  │     ├─► if event.new already in byId: remove optimistic (dedup)
  │     └─► else: update msg (id, seq, sentAt, status:"sent"), re-sort, move to byId
  │           └─► notify("messages:stream")
  │
  └─► on failure: failOptimistic(localId)
        └─► status = "failed", stays in list
              └─► notify("messages:stream")

retrySend(localId)
  └─► removes failed optimistic, creates new one, re-publishes

cancelSend(localId)
  └─► removes from list and pendingSends entirely
```

**Race condition:** If `event` (from server broadcast) arrives before the publish ack, `appendEvent` inserts the server event. When `reconcile` runs, it finds the event ID already in `byId` and simply removes the optimistic duplicate.

**E2EE:** Encryption and decryption are handled transparently at the Herald SDK transport layer, not in ChatCore. When `e2eeManager` is set on `HeraldClient`, `publish()` encrypts the body before sending and incoming `event` payloads are decrypted before the event handler fires. ChatCore's `handleEvent` always sees plaintext. `HeraldChatClient.editEvent()` also encrypts via the same path. ChatCore has no E2EE awareness — it operates on plaintext bodies throughout.

`send()` returns `PendingMessage`:
```typescript
interface PendingMessage {
  localId: string;
  readonly status: MessageStatus;
  retry(): Promise<void>;
  cancel(): void;
}
```
Wrapper around `retrySend()`/`cancelSend()`.

**Reactive vs. imperative:** Most UIs observe message status reactively via `useMessages()` — the optimistic message appears in the list with `status: "sending"` and updates to `"sent"` or `"failed"` automatically via Notifier. `PendingMessage` is for imperative flows: fire-and-forget sends where the caller needs `retry()` or `cancel()` handles without searching the message list. The `status` getter reads current state lazily from MessageStore — it is not a snapshot, so it reflects the latest status at read time.

---

## Auto-Mark-Read

**Triggers:**
1. New event arrives AND at live edge AND not hidden
2. User scrolls to live edge (`setAtLiveEdge(true)`)

**Algorithm:**
1. Walk messages from end, find latest with seq > 0 (skip optimistic)
2. If latestSeq > myCursor: update cursor locally + send `cursor.update` frame
3. CursorStore uses MAX semantics — cursor never goes backward

**Debounce:** Fires after `scrollIdleMs` (default 1000ms) of idle time at the live edge. Prevents marking as read during fast scrolling. Timer cleared when user scrolls away from edge.

---

## Types

```typescript
type MessageStatus = "sending" | "sent" | "delivered" | "failed" | "read";

interface Message {
  id: string;
  localId?: string;
  seq: number;
  stream: string;
  sender: string;
  body: string;
  meta?: unknown;              // app-specific payload (see note below)
  parentId?: string;           // threading parent (see note below)
  sentAt: number;
  editedAt?: number;
  deleted: boolean;
  status: MessageStatus;
  reactions: Map<string, Set<string>>;   // emoji → set of userIds
}
```

**`meta?: unknown`** — Opaque app-specific payload. Herald stores and delivers it as-is (Herald is a transport layer — event body and meta are opaque). Consumers use this for attachments, embeds, structured content, link previews, or any domain-specific data. Typed as `unknown` because ChatCore imposes no schema — consumers cast to their app's shape. If you want type safety across your app, define your own `MyMeta` interface and cast at the boundary (`msg.meta as MyMeta`).

**`parentId?: string`** — Threading parent event ID. Passed through to the server on `publish()` and stored on the message, but ChatCore provides no threading behavior: no "get replies to X" query, no threaded message grouping, no reply-count aggregation. It is a data field for consumers to build threading on top of — filter `getMessages()` by `parentId`, group in the UI, etc. ChatCore treats it as opaque metadata.

```typescript

interface PendingMessage {          // Returned by send()
  localId: string;
  readonly status: MessageStatus;
  retry(): Promise<void>;
  cancel(): void;
}

interface Member {
  userId: string;
  role: string;
  presence: string;                 // "online" | "away" | "dnd"
}

interface ScrollStateSnapshot {
  atLiveEdge: boolean;
  pendingCount: number;
  isLoadingMore: boolean;
}

type LivenessState = "active" | "idle" | "hidden";

interface LivenessConfig {
  idleTimeoutMs?: number;           // default 120000
  throttleMs?: number;              // default 5000
}

// Discriminated union for middleware
type ChatEvent =
  | { type: "event"; data: EventNew }
  | { type: "event.edited"; data: EventEdited }
  | { type: "event.deleted"; data: EventDeleted }
  | { type: "reaction.changed"; data: ReactionChanged }
  | { type: "presence"; data: PresenceChanged }
  | { type: "cursor"; data: CursorMoved }
  | { type: "typing"; data: TypingEvent }
  | { type: "member.joined"; data: MemberEvent }
  | { type: "member.left"; data: MemberEvent }
  | { type: "event.delivered"; data: EventDelivered }
  | { type: "ephemeral"; data: EventReceived };

type Middleware = (event: ChatEvent, next: () => void) => void;
```

---

## Testing Coverage

### What's Tested

| Area | File | Coverage |
|---|---|---|
| Notifier | `test/notifier.test.ts` | Comprehensive — subscribe, notify, isolation, edge cases |
| MessageStore | `test/message-store.test.ts` | Comprehensive — ordering, dedup, optimistic, edit/delete/reactions, markRead, stress |
| CursorStore | `test/cursor-store.test.ts` | Good — init, MAX semantics, unread calc, remote cursors, totals |
| LivenessController | `test/liveness.test.ts` | Comprehensive — state machine, throttle, visibility, re-attach |
| ChatCore lifecycle | `test/chat-core.test.ts` | Good — attach/detach/destroy, join/leave, all event handlers |
| ChatCore middleware | `test/chat-core.test.ts` | Good — passthrough, skip, enrich, ordering, all event types |
| ChatCore ephemeral | `test/chat-core.test.ts` | Good — forwarding, notifier, cleanup on leave/destroy |
| ChatCore read status | `test/chat-core.test.ts` | Good — sent→read on remote cursor, own cursor ignored, other users' msgs unaffected |
| ChatCore listen/unlisten | `test/chat-core.test.ts` | Good — no stores, notifier slices, cleanup |
| Hook contracts | `test/hooks.test.ts` | Good — documents slice subscriptions, mock-based |

### What's Not Tested

- **Scroll anchoring** — HeraldChat's useLayoutEffect delta calculation
- **React components** — all five render-prop components have zero tests
- **Provider lifecycle** — mount/unmount, missing context error
