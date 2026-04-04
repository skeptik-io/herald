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
  url: 'wss://herald.example.com/ws',
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

await client.send('general', 'hello!', { meta: { custom: true } });
client.updateCursor('general', 42);
client.setPresence('dnd');
client.startTyping('general');
```

## E2EE

Herald supports optional client-side end-to-end encryption. When enabled, message bodies are encrypted with AES-256-GCM and blind search tokens are generated via HMAC-SHA256 — all client-side. The server never sees plaintext.

### Setup

```typescript
import { HeraldClient, initE2EE, generateKeyPair, deriveSharedSecret, createSession } from '@skeptik-io/herald-sdk';

const client = new HeraldClient({
  url: 'wss://herald.example.com/ws',
  token: jwt,
  e2ee: true, // enables WASM crypto module
});

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

// Assign to a room
client.setE2EESession('private-room', session);
```

### Transparent Encrypt/Decrypt

Once a session is set for a room, `send()` and `on('message')` handle encryption transparently:

```typescript
// Encrypts automatically — server stores ciphertext
await client.send('private-room', 'sensitive message');

// Decrypts automatically — handler receives plaintext
client.on('message', (msg) => {
  console.log(msg.body); // "sensitive message"
});

// Fetch history — each message decrypted on arrival
const history = await client.fetch('private-room', { limit: 50 });
```

### Key Persistence

Export keys to store in the client keychain (e.g. IndexedDB):

```typescript
const { cipherKey, veilKey } = session.exportKeys();
// Store cipherKey and veilKey securely

// Later, restore the session
import { restoreSession } from '@skeptik-io/herald-sdk';
const restored = restoreSession(savedCipherKey, savedVeilKey);
client.setE2EESession('private-room', restored);
```

### How It Works

- **Encryption**: AES-256-GCM via `shroudb-cipher-blind` (WASM). Room ID is used as AAD, binding ciphertext to the room.
- **Blind tokens**: HMAC-SHA256 via `shroudb-veil-blind` (WASM). Stored in `meta.__blind` for searchable encryption without exposing plaintext.
- **Key derivation**: x25519 key exchange, then HKDF-SHA256 with separate domain separators for cipher (`herald-cipher-v1`) and search (`herald-veil-v1`) keys.
- **Zero overhead**: WASM module is only loaded when `e2ee: true` is set. Non-E2EE usage has no additional dependencies.

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
| `message.deleted` | `{ room, id, seq }` |
| `message.edited` | `{ room, id, seq, body, edited_at }` |
| `reaction.changed` | `{ room, message_id, emoji, user_id, action }` |
| `event.received` | `{ room, event, sender, data }` |
| `presence` | `{ user_id, presence }` |
| `cursor` | `{ room, user_id, seq }` |
| `member.joined` / `member.left` | `{ room, user_id, role }` |
| `typing` | `{ room, user_id, active }` |
| `room.updated` / `room.deleted` | `{ room }` |
| `room.subscriber_count` | `{ room, count }` |
| `watchlist.online` / `watchlist.offline` | `{ user_ids }` |
| `connected` / `disconnected` / `reconnecting` | — |
