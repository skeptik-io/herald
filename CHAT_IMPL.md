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

`attach()` registers handlers for: `event`, `event.edited`, `event.deleted`, `reaction.changed`, `presence`, `cursor`, `typing`, `member.joined`, `member.left`, `connected`, `disconnected`.

**Gap (N-5):** `event.received` (ephemeral events) is not registered — silently dropped.

### Stream Management

```typescript
joinStream(streamId): Promise<SubscribedPayload>
```
Subscribes via `client.subscribe([streamId])`. Initializes cursor state (my cursor + latest seq), sets initial member list, creates ScrollState. Returns subscription payload.

```typescript
leaveStream(streamId): void
```
Unsubscribes and clears all state for the stream (messages, cursors, members, typing, scroll).

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

**Return type change (N-4):** `send()` will return `Promise<PendingMessage>` instead of `Promise<string>`. PendingMessage exposes `localId`, `status`, `retry()`, `cancel()` — wrapping the internal machinery that already exists.

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

**Slice naming:** `messages:{streamId}`, `typing:{streamId}`, `members:{streamId}`, `unread:{streamId}`, `unread:total`, `scroll:{streamId}`, `liveness`.

---

## Event Handlers

### Current Pipeline

```
HeraldClient event → ChatCore handler → store mutation → Notifier.notify(slice)
```

| SDK Event | Handler | Store Effect |
|---|---|---|
| `event` | `handleEvent` | `messages.appendEvent()` (binary search insert), `cursors.bumpLatestSeq()`, auto-mark-read if at edge |
| `event.edited` | `handleEdited` | `messages.applyEdit()` — updates body + editedAt |
| `event.deleted` | `handleDeleted` | `messages.applyDelete()` — sets deleted=true, clears body |
| `reaction.changed` | `handleReaction` | `messages.applyReaction()` — add/remove user from emoji set |
| `presence` | `handlePresence` | `members.updatePresence()` across all streams containing user |
| `cursor` | `handleCursor` | **No-op.** Comment: "could be used for last-read indicators" |
| `typing` | `handleTyping` | `typing.setTyping()` — skips own events, TTL 12s |
| `member.joined` | `handleMemberJoined` | `members.addMember()` — idempotent, presence=online |
| `member.left` | `handleMemberLeft` | `members.removeMember()` |
| `connected` | `handleConnected` | Documents that SDK re-subscribes automatically |
| `disconnected` | `handleDisconnected` | `typing.clearAll()` + restart expiry timer |
| `event.received` | **Not registered** | Silently dropped |

### Planned Pipeline (N-5)

```
HeraldClient event → middleware chain → next() → store mutation → Notifier.notify(slice)
```

**Middleware signature:** `(event: ChatEvent, next: () => void) => void`

- Code before `next()` runs before store update (enrichment, transformation, filtering)
- `next()` applies the store mutation synchronously
- Code after `next()` runs after state is updated (side-effects)
- Skipping `next()` drops the event entirely
- After-store reactions that don't need to intercept events use the existing `subscribe()` pattern

**Ephemeral events:** `event.received` handler registered. Forwarded through middleware then emitted via Notifier on a new `ephemeral:{streamId}` slice. Consumer subscribes to receive custom signaling (pins, session lifecycle, etc).

**Cursor handler wired:** `handleCursor` will track remote cursors in CursorStore and flip `Message.status` to `read` for self-sent messages with seq <= remote cursor.

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

**Issue (N-4):** `applyEdit` has no guard against `deleted: true` — will update body on a deleted message.

**Issue (N-4):** `prependBatch` uses `Array.sort` (O(n log n)) while `appendEvent` uses binary search insertion (O(log n)). Should use binary insertion for consistency.

### CursorStore

**Key:** `myCursor: Map<string, number>`, `latestSeq: Map<string, number>`.

**Semantics:** MAX-only — cursors never go backward. `updateMyCursor(streamId, seq)` ignores if seq <= current.

**Unread calculation:** `max(0, latestSeq - myCursor)`.

**Planned (N-5):** Add `remoteCursors: Map<string, Map<string, number>>` — tracks other users' cursor positions per stream. When a remote cursor advances past a self-sent message's seq, that message's status flips to `read`.

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

**Planned (N-4):** `send()` returns `PendingMessage` instead of `string`:
```typescript
interface PendingMessage {
  localId: string;
  readonly status: MessageStatus;
  retry(): Promise<void>;
  cancel(): void;
}
```
Wrapper around existing `retrySend()`/`cancelSend()` — the internal machinery is already tested.

---

## Auto-Mark-Read

**Triggers:**
1. New event arrives AND at live edge AND not hidden
2. User scrolls to live edge (`setAtLiveEdge(true)`)

**Algorithm:**
1. Walk messages from end, find latest with seq > 0 (skip optimistic)
2. If latestSeq > myCursor: update cursor locally + send `cursor.update` frame
3. CursorStore uses MAX semantics — cursor never goes backward

**Current behavior:** Fires immediately on trigger.

**Planned (N-4):** Debounce by `scrollIdleMs` (default 1000ms). Prevents marking as read during fast scrolling. Timer cleared when user scrolls away from edge.

---

## Types

```typescript
type MessageStatus = "sending" | "sent" | "failed";
// Planned (N-5): add "read"

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

interface PendingMessage {          // (N-4) Returned by send()
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

// (N-5) Discriminated union for middleware
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

## Planned: N-4 (SDK Hardening)

Changes that fix incomplete features and bugs in the existing implementation.

### herald-chat-core

**Wire `scrollIdleMs` debounce.** Add a per-stream timer. `autoMarkRead()` calls in `setAtLiveEdge()` and `handleEvent()` schedule the cursor update after `scrollIdleMs` instead of firing immediately. Timer cleared when user scrolls away from edge. Prevents false "read" marks during fast scrolling.

**Wire `PendingMessage`.** Implement wrapper class delegating to existing `retrySend()`/`cancelSend()`. Change `send()` return from `Promise<string>` to `Promise<PendingMessage>`. The internal machinery (pendingSends map, reconcile, fail, retry, cancel) is already implemented and tested — this wires it to the public interface that was designed for it.

**Fix `prependBatch` insertion.** Replace `Array.sort` with binary insertion loop, consistent with `appendEvent`.

**Guard edit-after-delete.** `applyEdit()` should no-op if `message.deleted === true`.

**Resilient Notifier.** Wrap each listener call in try/catch so one throwing listener doesn't break the notification loop.

**Configurable `loadMore` limit.** Read from `this.loadMoreLimit` (constructor option, default 50) instead of hardcoded.

### herald-chat-react

**Update `useMessages` hook.** `send()` return type changes from `string` to `PendingMessage`.

**Error handling in hook actions.** `send()`, `edit()`, `deleteEvent()`, `loadMore()` must not produce unhandled rejections — catch and surface via return value or callback.

**Input validation.** Undefined `streamId` or null `scrollRef` should throw or no-op cleanly.

**Component tests.** HeraldChat scroll anchoring, MessageList, MessageInput, PresenceIndicator, TypingIndicator — currently untested.

### herald-admin-typescript

**Fix transport error handling.** Empty catch block masks JSON parse failures. Complete HTTP error code mapping (400, 403, 409, 429, 500, 503). Add timeout via AbortSignal.

**Type `Promise<unknown>` methods.** `connections()`, `adminEvents()`, `errors()`, `stats()`.

**Migrate deprecated `presence`/`blocks`.** Update test-integration to `chat.*`, then remove top-level properties.

---

## Planned: N-5 (ChatCore Completeness)

Features that any chat SDK is expected to provide but are currently missing.

### Ephemeral Event Support

Register handler for `event.received`. Forward through middleware, then emit on `ephemeral:{streamId}` Notifier slice. Any chat app needs custom signaling — typing-with-content, pin toggling, session lifecycle, custom indicators.

Ephemeral events flow for all ChatCore-managed streams — both full-state (`joinStream`) and lightweight (`listen`).

### Lightweight Stream Subscriptions

```typescript
listen(streamId): Promise<void>     // subscribe, events flow through middleware + Notifier, no stores
unlisten(streamId): void            // unsubscribe lightweight stream
```

`joinStream` initializes full chat state (MessageStore, CursorStore, MemberStore, TypingStore, ScrollState). `listen` subscribes to the same transport but only routes events through middleware and Notifier slices — no store overhead. Covers inbox/notification rooms (e.g. `user:{userId}`) without forcing consumers onto a separate raw `HeraldClient`.

Same client, same middleware chain, same Notifier. Consumer subscribes to `event:{streamId}` or `ephemeral:{streamId}` slices for routing. `leaveStream` remains for full-state streams, `unlisten` for lightweight ones.

**Implementation:** ChatCore tracks listen-only stream IDs in a `Set<string>`. Every existing handler (`handleEvent`, `handleTyping`, `handleCursor`, etc.) checks this set before accessing stores. For listen-only streams: route through middleware, emit on Notifier slices, skip store mutations entirely. For full-state streams: current behavior unchanged.

### Per-Message Read Status

Extend `MessageStatus`: `"sending" | "sent" | "failed" | "read"`.

Track remote cursors in CursorStore (`remoteCursors: Map<streamId, Map<userId, seq>>`). When `handleCursor` fires for a remote user and their seq advances past a self-sent message, flip that message's status to `"read"`.

**Group chat semantics:** `status = "read"` means "at least one remote user's cursor >= this message's seq." This is the useful signal for both 1:1 and group conversations — "has anyone seen this?" Consumers that need per-user granularity (e.g. a read receipts UI showing exactly who read what) derive it from the remote cursor data in CursorStore directly, not from the status enum. No `readBy: Set<string>` on Message — the same information is already available from `remoteCursors.get(streamId)`.

**No `"delivered"` state.** Herald has no delivery receipt primitive — `"sent"` (server ack) is the strongest delivery signal the transport provides. An explicit "client received event X" frame would be needed at the protocol level to support `delivered`. Herald's ack mode (N-2) is for at-least-once delivery guarantees, not for user-visible delivery receipts. Consumers with secondary persistence (DB write confirmation) handle that concept in their own layer via middleware.

### Event Middleware

```typescript
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

Optional middleware chain in constructor. `next()` applies the store mutation synchronously. Consumer does enrichment before `next()`, side-effects after `next()`. Skipping `next()` drops the event.

Middleware sees the raw event, not computed effects. For example: when a cursor event triggers per-message read status flips, middleware sees the cursor event itself — not "message X was just marked read." Consumers that want to react to computed state changes (like a message's status changing to `read`) use a Notifier `subscribe()` on `messages:{streamId}` for that. Different tools: middleware intercepts the pipeline, subscriptions react to state.

After-store reactions that don't need to intercept events use the existing `subscribe()` pattern — middleware is only for cases where the consumer must transform or gate events before they hit state.

---

## Testing Coverage

### What's Tested

| Area | File | Coverage |
|---|---|---|
| Notifier | `test/notifier.test.ts` | Comprehensive — subscribe, notify, isolation, edge cases |
| MessageStore | `test/message-store.test.ts` | Comprehensive — ordering, dedup, optimistic, edit/delete/reactions, stress |
| CursorStore | `test/cursor-store.test.ts` | Good — init, MAX semantics, unread calc, totals |
| LivenessController | `test/liveness.test.ts` | Comprehensive — state machine, throttle, visibility, re-attach |
| ChatCore lifecycle | `test/chat-core.test.ts` | Partial — attach/detach/destroy, join/leave, event handling |
| Hook contracts | `test/hooks.test.ts` | Partial — documents slice subscriptions, mock-based |

### What's Not Tested

- **Send pipeline end-to-end** — reconcile, retry, cancel, fail paths through ChatCore (store-level tested, integration not)
- **Race conditions** — event.new arriving before ack, concurrent sends, concurrent edits
- **Scroll anchoring** — HeraldChat's useLayoutEffect delta calculation
- **React components** — all five render-prop components have zero tests
- **Provider lifecycle** — mount/unmount, missing context error
- **Multi-stream** — presence fanout, total unread aggregation across streams
- **Disconnect/reconnect** — typing clear, state recovery
