# @skeptik-io/herald-chat-sdk

WebSocket client wrapper that extends `@skeptik-io/herald-sdk` with chat-specific frame types.

> **Herald persists chat mutations for replay, not for durability.** Edits, deletes, and reactions are kept in Herald's WAL (default **7-day retention**) so a client that reconnects after a brief disconnect replays every delta it missed — including, say, a delete that happened while its wifi was down. This makes reconnects seamless without a page refresh. It does **not** make Herald a durable store: after the TTL, those mutations are gone. Mirror them into your app's database via the server-side webhook, and hydrate history from your DB on cold load / long absences.

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

// Edit and delete events
await chat.editEvent('general', 'event-id', 'corrected text');
await chat.deleteEvent('general', 'event-id');

// Typing
chat.startTyping('general');
chat.stopTyping('general');

// Cursors (read receipts)
chat.updateCursor('general', 42);

// Reactions
chat.addReaction('general', 'event-id', '🔥');
chat.removeReaction('general', 'event-id', '🔥');
```

## API

| Method | Frame | Response |
|--------|-------|----------|
| `editEvent(stream, id, body)` | `event.edit` | `Promise<EventAck>` |
| `deleteEvent(stream, id)` | `event.delete` | `Promise<EventAck>` |
| `updateCursor(stream, seq)` | `cursor.update` | fire-and-forget |
| `startTyping(stream)` | `typing.start` | fire-and-forget |
| `stopTyping(stream)` | `typing.stop` | fire-and-forget |
| `addReaction(stream, eventId, emoji)` | `reaction.add` | fire-and-forget |
| `removeReaction(stream, eventId, emoji)` | `reaction.remove` | fire-and-forget |

E2EE: `editEvent` encrypts the body automatically when `e2eeManager` is set on the underlying `HeraldClient`.
