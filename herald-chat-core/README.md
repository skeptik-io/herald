# @skeptik-io/herald-chat

Framework-agnostic chat state machine built on Herald. Manages messages, cursors, presence, typing, scroll position, and liveness detection. Designed for `useSyncExternalStore` but works with any UI framework.

## Install

```bash
npm install @skeptik-io/herald-chat --registry=https://npm.pkg.github.com
```

Peer dependencies: `@skeptik-io/herald-sdk ^2.0.0`, `@skeptik-io/herald-chat-sdk ^0.1.0`

Optional peer dependency: `@skeptik-io/herald-presence-sdk ^0.1.0` (for manual presence overrides)

## Usage

```typescript
import { HeraldClient } from '@skeptik-io/herald-sdk';
import { HeraldChatClient } from '@skeptik-io/herald-chat-sdk';
import { ChatCore } from '@skeptik-io/herald-chat';

const client = new HeraldClient({ url: 'wss://...', token: jwt });
const chat = new HeraldChatClient(client);
await client.connect();

const core = new ChatCore({
  client,
  chat,
  userId: 'alice',
  presence: presenceClient, // optional — HeraldPresenceClient for manual overrides
  scrollIdleMs: 1000,       // debounce auto-mark-read (default 1000ms)
  loadMoreLimit: 50,        // events per loadMore() call (default 50)
  middleware: [              // optional — runs before/after store mutations
    (event, next) => {
      console.log(event.type);
      next(); // call next() to proceed; code after next() runs post-mutation
    },
  ],
});

core.attach(); // registers event handlers on the client

// Join a stream (full state: messages, cursors, members, scroll)
await core.joinStream('general');

// Send a message (optimistic insert, reconciles on ack)
const pending = await core.send('general', 'hello!', { meta: { custom: true } });
console.log(pending.status); // "sending" → "sent" → "read"

// Read state
core.getMessages('general');     // Message[] sorted by seq
core.getMembers('general');      // Member[] with presence
core.getTypingUsers('general');  // string[] of user IDs
core.getUnreadCount('general');  // number
core.getRemoteCursors('general'); // Map<userId, seq>

// Manual presence overrides (requires presence client)
core.setPresence('dnd', '2026-04-14T09:00:00Z');
core.clearPresenceOverride();
core.getPresence();               // PresenceStatus

// Subscribe to changes (useSyncExternalStore contract)
const unsub = core.subscribe('messages:general', () => {
  const msgs = core.getMessages('general');
});
```

## Stream Modes

**Full state** (`joinStream` / `leaveStream`): Initializes all stores (MessageStore, CursorStore, MemberStore, TypingStore, ScrollState). Full chat functionality.

**Lightweight** (`listen` / `unlisten`): Subscribes to events via the same transport, routes through middleware and Notifier, but no stores are initialized. Useful for inbox or notification streams where you need events but not chat state.

```typescript
await core.listen('user:alice');
core.subscribe('event:user:alice', () => { /* new event */ });
core.subscribe('ephemeral:user:alice', () => { /* ephemeral signal */ });
core.unlisten('user:alice');
```

## Optimistic Sends

`send()` returns a `PendingMessage` immediately. The message appears in `getMessages()` with `status: "sending"` before the server acknowledges.

```typescript
const pending = await core.send('general', 'hello');

pending.localId;  // "local:..." (stable identifier)
pending.status;   // "sending" → "sent" (lazy getter, reads current state)

// On failure:
pending.status;   // "failed"
await pending.retry();
pending.cancel();
```

## Per-Message Read Status

Message status progresses: `sending` → `sent` → `read`.

When a remote user's cursor advances past a self-sent message's sequence number, that message's status flips to `"read"`. In group chats, `"read"` means "at least one remote user has read this." For per-user granularity, use `getRemoteCursors(streamId)`.

## Event Middleware

Middleware intercepts events in the ChatCore pipeline. The chain is synchronous — code before `next()` runs before the store mutation, code after `next()` runs after.

```typescript
const core = new ChatCore({
  // ...
  middleware: [
    // Filter: block messages from ignored users
    (event, next) => {
      if (event.type === 'event' && ignored.has(event.data.sender)) return;
      next();
    },
    // Enrich: add client-side timestamp
    (event, next) => {
      if (event.type === 'event') event.data.meta = { ...event.data.meta, receivedAt: Date.now() };
      next();
    },
    // Before/after: run logic on both sides of the store mutation
    (event, next) => {
      console.log('before store:', event.type);
      next();
      console.log('after store:', event.type);
    },
  ],
});
```

Omitting the `next()` call swallows the event — the store will not be updated and downstream middleware will not run.

All 11 event types flow through middleware: `event`, `event.edited`, `event.deleted`, `reaction.changed`, `presence`, `cursor`, `typing`, `member.joined`, `member.left`, `event.delivered`, `ephemeral`.

## Ephemeral Events

Ephemeral events (`event.received`) are not persisted by the server. Use them for custom signaling: typing-with-content, pin toggling, session lifecycle.

```typescript
core.subscribe('ephemeral:general', () => {
  const last = core.getLastEphemeral('general', 'custom.ping');
  // filter by event type, or omit for most recent
});
```

## Notifier Slices

| Slice | Fires when |
|-------|------------|
| `messages:{streamId}` | Message list changes |
| `typing:{streamId}` | Typing indicators change |
| `members:{streamId}` | Member list changes |
| `unread:{streamId}` | Unread count for stream changes |
| `unread:total` | Total unread across all streams changes |
| `scroll:{streamId}` | Scroll state changes |
| `liveness` | Browser activity state changes |
| `presence` | Presence state changes |
| `ephemeral:{streamId}` | Ephemeral event received |
| `event:{streamId}` | Any event received (both full-state and listen-only) |

## Liveness Detection

Optional browser activity tracking that automatically syncs presence.

```typescript
const core = new ChatCore({
  // ...
  liveness: {
    idleTimeoutMs: 120_000, // default: 2 minutes
    throttleMs: 5_000,      // default: 5 seconds
  },
});
```

State machine: `active` ↔ `idle` ↔ `hidden`. Maps to presence: `active` → `online`, `idle`/`hidden` → `away`.
