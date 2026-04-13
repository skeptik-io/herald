# @skeptik-io/herald-chat

Framework-agnostic chat state machine built on Herald. Manages messages, cursors, presence, typing, scroll position, and liveness detection. Designed for `useSyncExternalStore` but works with any UI framework.

> **Two authorities: Herald within the buffer window, your DB across time.** Herald is authoritative for *deltas* within its retention window (default 7 days) — new events, edits, deletes, reactions, cursor moves. A client that disconnects briefly replays every missed delta on reconnect, including mutations. Your app's database is authoritative for *state* across time: on cold load, long absence, or a new device, hydrate from your DB via `seedHistory()` / `loadMoreWith(fetcher)`, then subscribe to Herald for further deltas. Both paths converge if your webhook mirrors every Herald event into your DB. Do not treat Herald as a history API — events past the TTL are pruned silently.

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

## Persist-first Writes (recommended)

By default, `ChatCore.send()` publishes the message directly to Herald over the WebSocket and waits for its ack. That's fine for demos but it makes Herald's ack the commit point — if the WS publish fails (rate limit, connection blip, E2EE error, auth drift), the sender sees an optimistic message that never lands in your app database and disappears on the next refresh. This is the classic failure mode for media messages: the attachment uploads, the WS publish fails silently mid-flight, your webhook never fires, the row never exists in your DB.

The fix is to make the app DB the commit point and treat Herald as the fanout pipe — the pattern Slack, Discord, Matrix, and WhatsApp all use. `ChatCore` supports this via an optional `writer`:

```typescript
import { ChatCore, type ChatWriter } from '@skeptik-io/herald-chat';

const writer: ChatWriter = {
  async send(draft) {
    // draft = { localId, streamId, body, meta, parentId, senderId }
    //
    // 1. Upload any attachments first (app-managed storage). Media lives in
    //    your CDN/bucket, not Herald — Herald only carries references.
    if ((draft.meta as any)?.attachments) {
      (draft.meta as any).attachments = await uploadAll((draft.meta as any).attachments);
    }

    // 2. Commit to your own API. Use localId as the idempotency key so
    //    retries from the SDK don't double-publish. Your backend then
    //    writes to the DB and calls Herald's HTTP admin API
    //    (POST /streams/:id/events) to inject the event for fanout.
    const msg = await fetch('/api/messages', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Idempotency-Key': draft.localId,
      },
      body: JSON.stringify(draft),
    }).then((r) => r.json());

    // 3. Return the canonical MessageAck. id/seq/sent_at come from your
    //    DB (or from Herald's response to your inject call). Optional
    //    body/meta let you propagate server-side canonicalization
    //    (sanitization, expanded attachment URLs, resolved mentions)
    //    into the sender's UI — the optimistic entry is replaced.
    return {
      id: msg.id,
      seq: msg.seq,
      sent_at: msg.sent_at,
      body: msg.body,
      meta: msg.meta,
    };
  },

  async edit(streamId, eventId, body) {
    await fetch(`/api/messages/${eventId}`, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ body }),
    });
  },

  async delete(streamId, eventId) {
    await fetch(`/api/messages/${eventId}`, { method: 'DELETE' });
  },

  // Reactions and cursors are optional. Herald doesn't expose HTTP
  // endpoints for them today, so a persist-first writer still has to
  // trigger Herald fanout somewhere. The simplest shape: your API
  // commits to the app DB, and then your backend calls back out via
  // a server-side Herald client, OR you skip the writer for these and
  // let the default WS path handle fanout (reactions/cursors are
  // ephemeral UX signals — losing one for 7 days is rarely a big deal).
};

const core = new ChatCore({ client, chat, userId: 'alice', writer });
```

Each writer slot is independent — provide only the ones you want to own. Anything omitted falls back to the Herald WebSocket default, so you can adopt incrementally (start with `send`, keep everything else on the default).

On a cold load or after a long absence, hydrate history from your own DB via `seedHistory()` / `loadMoreWith(fetcher)`. Both paths converge on identical state as long as your app DB mirrors every Herald event (via the webhook) in addition to writes that originate in your API.

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
