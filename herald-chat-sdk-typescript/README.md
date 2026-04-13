# @skeptik-io/herald-chat-sdk

Thin WebSocket helpers for typing indicators on top of
`@skeptik-io/herald-sdk`.

All other chat-state operations (send, edit, delete, reactions, cursor
advances) ride the standard `event.publish` path as typed envelopes.
For apps, that shape is wrapped by `herald-chat-core` — prefer that as
the integration point. This package exists for consumers that want to
send typing frames over a raw `HeraldClient` without pulling in the
full chat state machine.

> **Herald persists chat mutations for replay, not for durability.** Envelopes (reactions, edits, deletes, cursor advances) ride through the WAL with a default 7-day retention so a client that reconnects after a brief disconnect replays every delta it missed — including, say, a delete that happened while its wifi was down. This makes reconnects seamless without a page refresh. It does **not** make Herald a durable store: after the TTL, those mutations are gone. Mirror them into your app's database via the server-side webhook, and hydrate history from your DB on cold load / long absences.

## Install

```bash
npm install @skeptik-io/herald-chat-sdk --registry=https://npm.pkg.github.com
```

Peer dependency: `@skeptik-io/herald-sdk ^2.0.0`

## Usage

```typescript
import { HeraldClient } from '@skeptik-io/herald-sdk';
import { HeraldChatClient } from '@skeptik-io/herald-chat-sdk';

const client = new HeraldClient({
  url: 'wss://herald.example.com/ws',
  token: jwt,
});

const chat = new HeraldChatClient(client);
await client.connect();

chat.startTyping('general');
chat.stopTyping('general');
```

## API

| Method | Frame |
|--------|-------|
| `startTyping(stream)` | `typing.start` |
| `stopTyping(stream)` | `typing.stop` |

## Chat-state operations

Messages, edits, deletes, reactions, and cursor advances are published
as enveloped events via `HeraldClient.publish`:

```ts
// Message
await client.publish(stream, JSON.stringify({ kind: 'message', text: 'hi' }));

// Reaction
await client.publish(stream, JSON.stringify({
  kind: 'reaction', targetId: eventId, op: 'add', emoji: '🔥',
}));

// Edit
await client.publish(stream, JSON.stringify({
  kind: 'edit', targetId: eventId, text: 'corrected',
}));

// Delete
await client.publish(stream, JSON.stringify({ kind: 'delete', targetId: eventId }));

// Read cursor
await client.publish(stream, JSON.stringify({ kind: 'cursor', seq: 42 }));
```

Or use `herald-chat-core`, which wraps all of the above with optimistic
state updates, rollback, and a subscribable store.
