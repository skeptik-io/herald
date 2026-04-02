# @skeptik-io/herald-admin

Node.js HTTP admin client for Herald.

## Install

```bash
npm install @skeptik-io/herald-admin --registry=https://npm.pkg.github.com
```

## Usage

```typescript
import { HeraldAdmin } from '@skeptik-io/herald-admin';

const admin = new HeraldAdmin({
  url: 'https://herald.example.com',
  token: 'your-api-token',
});

// Rooms
const room = await admin.rooms.create('general', 'General Chat', {
  encryption_mode: 'server_encrypted',
});
const rooms = await admin.rooms.get('general');
await admin.rooms.delete('general');

// Members
await admin.members.add('general', 'alice', 'owner');
const members = await admin.members.list('general');
await admin.members.remove('general', 'alice');

// Messages
await admin.messages.send('general', 'system', 'Welcome!');
const history = await admin.messages.list('general', { limit: 50 });

// Presence
const presence = await admin.presence.getUser('alice');
const roomPresence = await admin.presence.getRoom('general');
const cursors = await admin.presence.getCursors('general');

// Health
const health = await admin.health();
```
