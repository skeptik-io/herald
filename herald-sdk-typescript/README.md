# @skeptik-io/herald-sdk

Browser WebSocket client for Herald.

> **Herald is a transport layer, not a database.** Events are retained for a short catch-up window (default 7 days) so reconnecting clients can resume without gaps. Anything your app needs to keep past that window — long-term history, search, audit — must live in your own database. This SDK delivers live events and resumes on reconnect; it does not replace an application message store.

## Install

```bash
npm install @skeptik-io/herald-sdk --registry=https://npm.pkg.github.com
```

## Usage

```typescript
import { HeraldClient } from '@skeptik-io/herald-sdk';

const client = new HeraldClient({
  url: 'wss://herald.example.com/ws',
  key: 'your-tenant-key',
  token: hmacToken, // HMAC-SHA256 signed token using tenant secret
  userId: 'alice',
  streams: ['general', 'notifications'],
});

await client.connect();

client.on('event', (evt) => {
  console.log(`${evt.sender}: ${evt.body}`);
  console.log('meta:', evt.meta);
});

client.on('presence', (p) => {
  console.log(`${p.user_id} is now ${p.presence}`);
});

await client.publish('general', 'hello!', { meta: { custom: true } });
client.updateCursor('general', 42);
client.setPresence('dnd');
client.startTyping('general');
```

## E2EE

Herald supports optional client-side end-to-end encryption. When enabled, event bodies are encrypted with AES-256-GCM and blind search tokens are generated via HMAC-SHA256 — all client-side. The server never sees plaintext.

### Setup

```typescript
import { HeraldClient } from '@skeptik-io/herald-sdk';
import { initE2EE, generateKeyPair, deriveSharedSecret, createSession } from '@skeptik-io/herald-sdk/e2ee';

const client = new HeraldClient({
  url: 'wss://herald.example.com/ws',
  key: 'your-tenant-key',
  token: hmacToken,
  userId: 'alice',
  streams: ['private-stream'],
  e2ee: true,
});

// Load the WASM crypto module before connecting
await initE2EE();
await client.connect();
```

### Key Exchange

```typescript
// Generate your keypair (x25519)
const myKeys = generateKeyPair();

// Exchange public keys with the other party (via your app's API)
const theirPublicKey = await exchangePublicKeys(myKeys.publicKey);

// Derive shared secret
const sharedSecret = deriveSharedSecret(myKeys.secretKey, theirPublicKey);

// Create a session (derives both cipher and search keys)
const session = createSession(sharedSecret);

// Assign to a stream
client.setE2EESession('private-stream', session);
```

### Transparent Encrypt/Decrypt

Once a session is set for a stream, `publish()` and `on('event')` handle encryption transparently:

```typescript
// Encrypts automatically — server stores ciphertext
await client.publish('private-stream', 'sensitive message');

// Decrypts automatically — handler receives plaintext
client.on('event', (evt) => {
  console.log(evt.body); // "sensitive message"
});

// Fetch history — each event decrypted on arrival
const history = await client.fetch('private-stream', { limit: 50 });
```

### Key Persistence

Export keys to store in the client keychain (e.g. IndexedDB):

```typescript
const { cipherKey, veilKey } = session.exportKeys();
// Store cipherKey and veilKey securely

// Later, restore the session
import { restoreSession } from '@skeptik-io/herald-sdk/e2ee';
const restored = restoreSession(savedCipherKey, savedVeilKey);
client.setE2EESession('private-stream', restored);
```

### How It Works

- **Encryption**: AES-256-GCM via `shroudb-cipher-blind` (WASM). Stream ID is used as AAD, binding ciphertext to the stream.
- **Blind tokens**: HMAC-SHA256 via `shroudb-veil-blind` (WASM). Stored in `meta.__blind` for searchable encryption without exposing plaintext.
- **Key derivation**: x25519 key exchange, then HKDF-SHA256 with separate domain separators for cipher (`herald-cipher-v1`) and search (`herald-veil-v1`) keys.
- **Zero overhead**: WASM module is only loaded when `e2ee: true` is set. Non-E2EE usage has no additional dependencies.

## Auth

Herald uses HMAC-SHA256 signed tokens (key+secret model). Your backend generates tokens using the tenant secret:

```typescript
// Server-side token generation
import { createHmac } from 'crypto';

const payload = `${userId}:${streams.join(',')}:${timestamp}`;
const token = createHmac('sha256', tenantSecret).update(payload).digest('hex');
```

The client passes `key`, `token`, `userId`, and `streams` when constructing `HeraldClient`. Auth happens on WebSocket upgrade via query params.

## Events

| Event | Payload |
|-------|---------|
| `event` | `{ stream, id, seq, sender, body, meta, sent_at }` |
| `event.deleted` | `{ stream, id, seq }` |
| `event.edited` | `{ stream, id, seq, body, edited_at }` |
| `reaction.changed` | `{ stream, event_id, emoji, user_id, action }` |
| `event.received` | `{ stream, event, sender, data }` |
| `presence` | `{ user_id, presence, until? }` |
| `cursor` | `{ stream, user_id, seq }` |
| `member.joined` / `member.left` | `{ stream, user_id, role }` |
| `typing` | `{ stream, user_id, active }` |
| `stream.updated` / `stream.deleted` | `{ stream }` |
| `stream.subscriber_count` | `{ stream, count }` |
| `watchlist.online` / `watchlist.offline` | `{ user_ids }` |
| `connected` / `disconnected` / `reconnecting` | — |
