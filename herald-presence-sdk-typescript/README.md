# @skeptik-io/herald-presence-sdk

WebSocket client wrapper that extends `@skeptik-io/herald-sdk` with presence frame types.

## Install

```bash
npm install @skeptik-io/herald-presence-sdk --registry=https://npm.pkg.github.com
```

Peer dependency: `@skeptik-io/herald-sdk ^2.0.0`

## Usage

```typescript
import { HeraldClient } from '@skeptik-io/herald-sdk';
import { HeraldPresenceClient } from '@skeptik-io/herald-presence-sdk';

const client = new HeraldClient({
  url: 'wss://herald.example.com/ws',
  token: hmacToken,
});

const presence = new HeraldPresenceClient(client);
await client.connect();

// Set presence status
presence.setPresence('dnd');

// Set presence with expiry (ISO 8601)
presence.setPresence('away', '2026-04-14T09:00:00Z');

// Clear override (reverts to connection-derived presence)
presence.clearOverride();
```

## API

| Method | Frame | Response |
|--------|-------|----------|
| `setPresence(status, until?)` | `presence.set` | fire-and-forget |
| `clearOverride()` | `presence.set` (status=online) | fire-and-forget |

### Types

```typescript
type PresenceStatus = 'online' | 'away' | 'dnd' | 'offline';
```

- `"online"` clears any manual override (reverts to connection-derived presence).
- `"away"`, `"dnd"`, `"offline"` set a manual override.
- `until` is an optional ISO 8601 datetime for when the override expires.
