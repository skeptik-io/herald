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

// Streams
const stream = await admin.streams.create('general', 'General Chat');
const fetched = await admin.streams.get('general');
await admin.streams.delete('general');

// Members
await admin.members.add('general', 'alice', 'owner');
const members = await admin.members.list('general');
await admin.members.remove('general', 'alice');

// Events
await admin.events.publish('general', 'system', 'Welcome!');
const history = await admin.events.list('general', { limit: 50 });

// Presence
const presence = await admin.presence.getUser('alice');
const streamPresence = await admin.presence.getStream('general');
const cursors = await admin.presence.getCursors('general');

// Health
const health = await admin.health();
```
