# @skeptik-io/herald-sdk

Browser WebSocket client for Herald.

## Install

```bash
npm install @skeptik-io/herald-sdk --registry=https://npm.pkg.github.com
```

## Usage

```typescript
import { HeraldClient } from '@skeptik-io/herald-sdk';

const client = new HeraldClient({
  url: 'wss://herald.example.com',
  token: jwt, // must include tenant claim
  onTokenExpiring: async () => {
    const resp = await fetch('/api/refresh-token');
    return resp.text();
  },
});

await client.connect();
await client.subscribe(['general', 'notifications']);

client.on('message', (msg) => {
  console.log(`${msg.sender}: ${msg.body}`);
  console.log('meta:', msg.meta);
});

client.on('presence', (p) => {
  console.log(`${p.user_id} is now ${p.presence}`);
});

await client.send('general', 'hello!', { custom: true });
client.updateCursor('general', 42);
client.setPresence('dnd');
client.startTyping('general');
```

## JWT Format

Your backend mints JWTs with:

```json
{
  "sub": "user_id",
  "tenant": "your-tenant-id",
  "rooms": ["general", "notifications"],
  "exp": 1712003600,
  "iat": 1712000000,
  "iss": "your-app"
}
```

## Events

| Event | Payload |
|-------|---------|
| `message` | `{ room, id, seq, sender, body, meta, sent_at }` |
| `presence` | `{ user_id, presence }` |
| `cursor` | `{ room, user_id, seq }` |
| `member.joined` / `member.left` | `{ room, user_id, role }` |
| `typing` | `{ room, user_id, active }` |
| `connected` / `disconnected` / `reconnecting` | — |
