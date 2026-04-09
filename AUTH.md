# Herald Auth System — Audit & Redesign

## Current State

Herald has three independent auth mechanisms, two startup modes, and an open WebSocket endpoint. This document audits what exists, identifies what's wrong, and proposes a redesign.

## What exists today

### Three auth mechanisms

**1. JWT for WebSocket connections**
- Client connects to `/ws` (no auth on upgrade — connection accepted unconditionally).
- Client sends `{"type":"auth","payload":{"token":"<JWT>"}}` within 5 seconds.
- Server does a two-phase decode: insecure decode to extract `tenant` claim, then verified decode with that tenant's HMAC-SHA256 secret.
- JWT must contain: `sub` (user ID), `tenant`, `streams` (allowed streams), `exp`, `iat`, `iss`, `watchlist`.
- The tenant's backend is responsible for minting these JWTs.

**2. API tokens for tenant HTTP endpoints**
- Random UUID4s stored in `NS_API_TOKENS` namespace.
- `Authorization: Bearer <uuid>` on every request.
- Each token maps to a tenant ID and optional scope (`read-only`, `stream:<id>`).
- Validated by DB lookup on every request.
- Created/revoked only by the super admin.

**3. Static password for admin HTTP endpoints**
- `auth.super_admin_token` in config.
- `Authorization: Bearer <token>` checked via constant-time comparison.
- Gates all `/admin/*` endpoints.

These three systems share no concepts, no credentials, no middleware. A tenant has a `jwt_secret` for WebSocket auth and separate API tokens for HTTP auth. The admin password is a third thing entirely.

### Two startup modes

**Single-tenant mode** (`--single-tenant`, default):
- Config must provide `auth.jwt_secret` (min 16 chars).
- Config does NOT need `super_admin_token`.
- Auto-creates a `"default"` tenant at startup using the config jwt_secret.
- Auto-creates API tokens from `auth.api.tokens` config list.
- Admin API exists but is effectively inaccessible (no super_admin_token).

**Multi-tenant mode** (`--multi-tenant`):
- Config must provide `super_admin_token` (min 16 chars).
- Config does NOT need `jwt_secret`.
- No default tenant — admin creates tenants via `POST /admin/tenants`, supplying a `jwt_secret` themselves.

The only behavioral difference: which config field is validated at startup and whether a default tenant is bootstrapped. Same server, same code paths, same endpoints. The flag adds a branch in `main.rs` and a branch in `config.validate()`.

### Open WebSocket endpoint

`GET /ws` upgrades to WebSocket with zero pre-auth checks. On upgrade:

1. `ConnId` allocated (atomic counter)
2. Socket split into tx/rx halves
3. 256-slot MPSC channel created
4. Writer tokio task spawned

All of this happens before any auth. The server then waits 5 seconds for an auth message. If none arrives or auth fails, resources are cleaned up — but during that 5-second window, each connection holds ~3KB of resources with no limit on how many unauthenticated connections can exist simultaneously.

There is no per-IP limit, no global connection limit, no pre-auth connection cap. Only a per-tenant limit of 10,000 — checked after JWT validation succeeds. An attacker can open unlimited unauthenticated connections.

### Unprotected endpoints

`/health`, `/health/live`, `/health/ready`, and `/metrics` are completely public. `/health` exposes connection count, stream count, tenant count, storage health, and sentry integration status.

### Token refresh

The protocol defines `auth.refresh` and the server parses it. The handler immediately rejects it with `"already authenticated"`. Dead code.

### Cache TTL

`TENANT_CACHE_TTL_SECS = 300` is defined. `cached_at` is stored on every cache entry. Neither is ever checked. The cache only updates on explicit admin API calls or server restart.

### Error leakage

JWT validation failures send detailed error messages to unauthenticated WebSocket clients: `"unknown tenant: {name}"`, `"JWT validation failed: {details}"`. This leaks tenant names and validation internals.

### JWT secret management

- On `POST /admin/tenants`, the admin supplies `jwt_secret` as a request field. No strength validation.
- `jwt_secret` and `jwt_issuer` cannot be updated via any API endpoint — `PATCH /admin/tenants/{id}` updates name, plan, and config only.
- No rotation mechanism exists.
- If secret is changed directly in the DB, the cache keeps the old one until restart.

### What tenants can do

With an API token, tenants can manage their own streams, members, events, presence, cursors, blocks. They have a functional tenant-scoped HTTP API.

### What tenants cannot do

- Create or revoke their own API tokens
- Rotate their JWT secret
- See their own connections, errors, or audit log
- Any `/admin/*` operation

Every operational action requires the super admin.

---

## What's wrong

### 1. Artificial complexity with no security benefit

Three auth mechanisms that could be one. Two startup modes that differ by an if-statement. A "super admin token" that's just a password with a fancy name. Config fields that only matter depending on a CLI flag. None of this adds security — it adds surface area for misconfiguration.

Redis has `requirepass`. Postgres has roles and passwords. Herald has `jwt_secret`, `jwt_issuer`, `super_admin_token`, `api.tokens`, `--single-tenant`, and `--multi-tenant`.

### 2. The WebSocket endpoint is unprotected

Any client on the network can:
- Open unlimited TCP connections to `/ws`
- Each connection allocates a task, a channel, and socket resources
- Hold them for 5 seconds with no auth
- Repeat indefinitely

The per-tenant connection limit only kicks in after successful JWT validation. There is no defense against unauthenticated connection flooding.

### 3. JWT secret supplied by the admin

The admin creates a tenant and pastes in a secret they generated. If they use `"password123"`, Herald stores it. No minimum length, no entropy check on the tenant creation endpoint (config-level validation exists for single-tenant mode but not for the admin API path). Every comparable platform generates the secret and returns it once.

### 4. No secret rotation

There is no API endpoint to rotate a tenant's JWT secret. The `PATCH` endpoint doesn't accept `jwt_secret`. The only path is direct DB modification, which the cache won't reflect. This is not a missing feature — it's a design gap that makes secret compromise unrecoverable without server restart.

### 5. Token refresh is dead code

`AuthRefresh` is defined in the protocol, parsed by the server, and immediately rejected. Clients with expiring JWTs must disconnect and reconnect. JWT expiry is only checked at initial auth time — once connected, the JWT could expire and the connection lives on indefinitely.

### 6. Tenants are locked out of their own operations

A tenant running on a shared Herald deployment can't create their own API tokens, can't rotate their own secret, can't see their own connections or errors. Everything goes through the super admin. This turns every operational task into a support ticket.

### 7. Health endpoints leak operational data

`/health` returns connection count, stream count, tenant count, and integration status to anyone who asks. `/metrics` returns full Prometheus data. These should be behind auth or at minimum configurable.

---

## Redesign

### Principles

1. **One auth system.** A server password and tenant credentials. Not three mechanisms with separate middleware stacks.
2. **Credentials are generated, not supplied.** The server generates secrets and returns them once. The admin doesn't need to know what a good HMAC key looks like.
3. **No modes.** Start Herald, it works. Create tenants if you want more than one. The default tenant is just a tenant.
4. **Protect the connection.** Auth before resource allocation, not after.
5. **Tenants manage themselves.** If you have credentials for a tenant, you can operate that tenant.

### Server password

One config field: `password`. Required. Min 16 characters. This is the admin password for the server.

```toml
[auth]
password = "my-herald-password"
```

Admin API uses `Authorization: Bearer <password>`. Same constant-time comparison as today, just a sane name.

No `jwt_secret`, no `jwt_issuer`, no `super_admin_token`, no `api.tokens`, no `--single-tenant`, no `--multi-tenant`.

### Tenant credentials

On `POST /admin/tenants`, the server generates a `key` (public identifier) and `secret` (signing key). Returned once in the response. The admin provides only `id` and `name`.

```
POST /admin/tenants
{"id": "myapp", "name": "My App"}

201 Created
{"id": "myapp", "name": "myapp", "key": "hk_a1b2c3d4e5f6", "secret": "hs_...64chars..."}
```

The key identifies the tenant. The secret signs tokens and authenticates API requests. One credential pair, used everywhere.

### Startup

Herald starts, creates a `default` tenant with auto-generated key+secret, logs them once at startup. If the default tenant already exists (persistent storage), it skips creation and logs the existing key (not the secret).

No flags. No config-level JWT secret. First startup shows you the credentials you need. Subsequent startups just run.

If you want more tenants, use the admin API. If you only ever need one, use the default.

### WebSocket auth

Move auth before resource allocation.

**Option A — Auth on upgrade (query parameter):**
Client connects to `/ws?key=hk_...&token=...&user_id=alice`. The upgrade handler validates before accepting the WebSocket. If auth fails, return 401 — no upgrade, no resources allocated.

The `token` is an HMAC-SHA256 signature over a canonical payload, generated by the tenant's backend:

```
HMAC-SHA256(secret, "user_id:sorted_streams:watchlist")
```

Streams and watchlist are sorted, comma-joined. No timestamp — the token proves the tenant's server authorized this user for these streams. Expiry is handled by the server tracking the connection, not by embedding claims in the token.

Pros: Zero resource allocation before auth. Standard HTTP auth flow.
Cons: Credentials in URL (logged by proxies, visible in browser history). Mitigated by using short-lived tokens (the token itself isn't the secret — it's derived from the secret).

**Option B — Auth on first message (current pattern, hardened):**
Keep the current "connect then auth" pattern but add pre-auth defenses:
- Global max unauthenticated connections (configurable, e.g., 1000)
- Per-IP unauthenticated connection limit (configurable, e.g., 10)
- Reduce auth timeout from 5s to 2s
- First byte must arrive within 1s or connection drops
- Auth message must be the first message (current behavior, but enforce strictly)

Pros: No credentials in URL. Familiar pattern (Centrifugo, Phoenix Channels do this).
Cons: Still allocates resources before auth. More config knobs.

**Recommendation:** Option A for the default path. It's simpler, more secure, and doesn't require inventing connection-level rate limiting. Option B can exist as a fallback for environments where URL auth is unacceptable, but it should be opt-in and hardened.

### HTTP auth

Tenant API requests use the same key+secret:

```
Authorization: Basic base64(key:secret)
```

Or continue using API tokens (Bearer) for scoped access. API tokens remain UUID4s tied to a tenant — but tenants can now create and revoke their own tokens since they authenticate with their key+secret.

The middleware tries Basic first (key+secret lookup), then Bearer (API token lookup). One middleware, not two.

### Tenant self-service

If you authenticate as a tenant (via key+secret), you can:
- Rotate your own secret (`POST /self/secret/rotate` — returns new secret once)
- Create/list/revoke your own API tokens
- See your own connections, errors, events, audit log

These are tenant-scoped views of existing admin data. The admin sees everything across all tenants. The tenant sees only their own.

### Connection token model

The tenant's backend generates connection tokens for their users:

```python
import hmac, hashlib

def connection_token(secret, user_id, streams, watchlist=None):
    streams_str = ",".join(sorted(streams))
    watchlist_str = ",".join(sorted(watchlist or []))
    payload = f"{user_id}:{streams_str}:{watchlist_str}"
    return hmac.new(secret.encode(), payload.encode(), hashlib.sha256).hexdigest()
```

The client sends this to Herald along with the key, user_id, streams, and watchlist in plaintext. Herald reconstructs the payload, computes the HMAC with the tenant's secret, and compares. If it matches, the tenant's server authorized this user for these streams.

No JWT library. No claims schema. No expiry management. No issuer validation. The token is a signature, not a container.

**Token expiry**: The connection itself has a configurable max lifetime (server-side, per-tenant or global). When it expires, the server sends a `connection.expiring` message and the client reconnects with a fresh token from their backend. This replaces JWT expiry and the broken `auth.refresh` mechanism.

### Cache TTL

Enforce it. When `validate_signed_token` looks up a tenant from cache and `cached_at` is older than `TENANT_CACHE_TTL_SECS`, refresh from DB. This makes secret rotation propagate within the TTL window without restart.

### Error messages

Never send internal details to unauthenticated clients. Auth failures return a generic error:

```json
{"type": "auth_error", "payload": {"code": "AUTH_FAILED", "message": "authentication failed"}}
```

Log the detailed reason server-side. Don't tell the client whether the tenant doesn't exist, the signature is wrong, or the token is malformed.

### Health endpoints

Add an `expose_health` config option (default: `true` for `/health/live` and `/health/ready`, `false` for `/health` and `/metrics`). When disabled, these endpoints require the admin password.

---

## Migration

Existing tenants have `jwt_secret` and `jwt_issuer` in their stored records. On upgrade:

1. Server reads existing tenants from DB.
2. For each tenant without a `key`/`secret`: generates them, writes back, logs the new key (not secret) at startup.
3. JWT auth continues working for a configurable grace period (e.g., 30 days) to allow migration. After that, only signed tokens are accepted.
4. `jwt_secret` field remains in the DB schema but is only used during the grace period.

This is the only place where backwards compatibility matters — existing deployments need time to update their client code.

---

## Summary of changes

| Current | Proposed |
|---------|----------|
| `super_admin_token` config | `password` config |
| `jwt_secret` per tenant (admin-supplied) | `key` + `secret` per tenant (server-generated) |
| `api.tokens` in config | Removed (default tenant gets auto-generated credentials) |
| `--single-tenant` / `--multi-tenant` flags | Removed (default tenant always exists) |
| JWT for WebSocket | HMAC signed token (no JWT library) |
| API tokens for HTTP (admin-created only) | key+secret Basic auth + optional API tokens (self-service) |
| Open WebSocket upgrade | Auth on upgrade (query param) or hardened post-upgrade |
| Cache TTL defined but unenforced | Enforced with DB refresh |
| Token refresh rejected | Connection lifetime + reconnect |
| Error details sent to clients | Generic auth errors, details logged server-side |
| Tenants locked out of self-service | Tenant self-service endpoints |
| Health/metrics public | Configurable auth |
