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
| `attach()` | Registers 11 event handlers on HeraldClient. Starts typing expiry interval (2s). Attaches liveness controller. Idempotent. |
| `detach()` | Deregisters all handlers. Stops typing expiry. Detaches liveness. Idempotent. |
| `destroy()` | Calls detach(), then clears all stores and notifier listeners. |

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
Lightweight subscription — subscribes via transport, events flow through middleware and Notifier slices, but no stores are initialized (no MessageStore, CursorStore, ScrollState overhead). Useful for inbox/notification rooms.

```typescript
unlisten(streamId): void
```
Unsubscribes a lightweight stream. Clears ephemeral state.

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
| `markRead(streamId, upToSeq, sender)` | Walks messages, flips self-sent (msg.sender === sender) messages with seq <= upToSeq from `sent` to `read`. |

### CursorStore

**Key:** `myCursor: Map<string, number>`, `latestSeq: Map<string, number>`, `remoteCursors: Map<string, Map<string, number>>`.

**Semantics:** MAX-only — cursors never go backward. `updateMyCursor(streamId, seq)` and `updateRemoteCursor(streamId, userId, seq)` ignore if seq <= current.

**Unread calculation:** `max(0, latestSeq - myCursor)`.

**Remote cursors:** Tracks other users' cursor positions per stream. When a remote cursor advances past a self-sent message's seq, that message's status flips to `read`. Consumers needing per-user granularity access `getRemoteCursors(streamId)` directly.

### MemberStore

**Key:** `Map<string, Member[]>` — per-stream.

**Member:** `{userId, role, presence}`. Presence is `"online" | "away" | "dnd"`.

`streamsForUser(userId)` returns all streams containing a user — used by presence handler to fan out presence changes across streams.

### TypingStore

**Key:** `Map<string, Map<string, number>>` — per-stream map of userId → timestamp.

**TTL:** 12s client-side (slightly > server's 10s). Expiry check runs every 2s via `setInterval`.

**Optimization:** Only notifies on actual list changes, not timestamp refreshes. Compares old vs new snapshot array before firing.

### ScrollState

Per-stream scroll coordination: `{atLiveEdge, pendingCount, isLoadingMore}`.

- `pendingCount` increments when events arrive while scrolled up (not at live edge)
- `setAtLiveEdge(true)` resets pending count
- `setLoadingMore` gates concurrent `loadMore()` calls

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
type MessageStatus = "sending" | "sent" | "failed" | "read";

interface Message {
  id: string;
  localId?: string;
  seq: number;
  stream: string;
  sender: string;
  body: string;
  meta?: unknown;
  parentId?: string;
  sentAt: number;
  editedAt?: number;
  deleted: boolean;
  status: MessageStatus;
  reactions: Map<string, Set<string>>;   // emoji → set of userIds
}

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
