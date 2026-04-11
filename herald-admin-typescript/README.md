# @skeptik-io/herald-admin

Node.js HTTP admin client for Herald.

## Install

```bash
npm install @skeptik-io/herald-admin --registry=https://npm.pkg.github.com
```

## Usage

```typescript
import { HeraldAdmin } from '@skeptik-io/herald-admin';

// With key + secret (Basic auth)
const admin = new HeraldAdmin({
  url: 'https://herald.example.com',
  key: 'your-tenant-key',
  secret: 'your-tenant-secret',
});

// Or with an API token (Bearer auth)
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

// Presence (top-level namespace)
const presence = await admin.presence.getUser('alice');
const bulk = await admin.presence.getBulk(['alice', 'bob', 'carol']);
const streamPresence = await admin.presence.getStream('general');
const cursors = await admin.presence.getCursors('general');
await admin.presence.setOverride('alice', { status: 'dnd', until: '2026-04-14T09:00:00Z' });

// Health
const health = await admin.health();
```
