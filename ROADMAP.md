# Herald — Roadmap

Features planned for Herald, ordered by priority. Each builds on Herald's existing primitives (streams, events, members, presence) without changing its core identity as a protocol-agnostic real-time transport layer.

---

## 1. Batch Publish

**Status:** Planned

Publish events to multiple streams in a single HTTP request. Reduces round-trips for high-fanout scenarios (e.g., notifying hundreds of users after a batch computation).

```
POST /events/batch
Authorization: Bearer <token>

[
  {"stream": "user_abc", "body": "...", "meta": {"type": "cluster.updated"}},
  {"stream": "user_def", "body": "...", "meta": {"type": "cluster.updated"}}
]
```

Response: array of `{id, stream, seq, sent_at}` in request order. Partial failures return per-item errors.

**Scope:**
- New HTTP endpoint on the admin API (port 6201)
- Single WAL write per batch where possible
- Max batch size configurable (default 100)
- Each item validated independently (stream exists, body size, rate limits)

---

## 2. Server-Side Event Filtering

**Status:** Planned

Allow subscribers to filter events by `meta` fields so they only receive events they care about. Without this, consumers must either create a stream per concern (combinatorial explosion) or filter client-side (wasted bandwidth).

```json
{
  "type": "subscribe",
  "payload": {
    "streams": ["user_abc"],
    "filter": {
      "meta.type": ["match.new", "feed.updated"]
    }
  }
}
```

**Scope:**
- Optional `filter` field on `subscribe` — backward-compatible, omitting it delivers all events
- Top-level `meta` key equality only (no nested paths, no regex, no range queries)
- Filter evaluated in fan-out before serialization — filtered events are never sent over the wire
- Filters are per-subscription, not persisted — resubscribe resets them
- HTTP replay (`GET /streams/:id/events?after=`) gets an optional `filter` query param

**Non-goals:** Full query language, nested field matching, server-side projections.

---

## 3. SSE Stream Tailing

**Status:** Planned

Server-Sent Events endpoint for tailing a stream over plain HTTP. Intended for backend services (dashboards, analytics pipelines) that want real-time events without a WebSocket client library.

```
GET /streams/:id/events/live
Authorization: Bearer <token>
Accept: text/event-stream
```

Each SSE event:
```
id: <seq>
event: event
data: {"id":"evt_...","seq":42,"sender":"user_x","body":"...","meta":{...},"sent_at":1712000000000}
```

**Scope:**
- Reuses existing fan-out subscription machinery — SSE client is just another subscriber
- `Last-Event-ID` header for reconnect catch-up (maps to sequence number)
- Auth via bearer token query param (`?token=`) or `Authorization` header
- Read-only — no publishing, no presence, no cursors
- Admin API port (6201), not the WebSocket port

**Non-goals:** Replacing WebSocket for interactive clients. SSE is unidirectional — clients that need presence, typing indicators, or publishing use WebSocket.

---

## 4. Claim-Gated Streams

**Status:** Planned

Allow streams to grant subscribe access based on JWT claims without explicit membership. Public streams already auto-add members on subscribe, but still require the stream ID in the JWT `streams` claim. Claim-gated streams go further: any JWT matching a claim predicate can subscribe.

```json
// Stream creation
POST /streams
{
  "id": "announcements",
  "name": "Company Announcements",
  "public": true,
  "claim_gate": {
    "tenant": "acme"
  }
}
```

Any JWT with `"tenant": "acme"` can subscribe to this stream without it appearing in the token's `streams` claim.

**Scope:**
- New optional `claim_gate` field on stream creation (top-level claim equality only)
- Subscribe checks: (1) stream in JWT `streams` claim, OR (2) JWT claims satisfy `claim_gate`
- Claim-gated streams are always public (auto-join on subscribe)
- Gate is set at creation, updatable via `PATCH /streams/:id`
- No wildcard patterns, no nested claim matching

**Non-goals:** Dynamic group membership, pattern-based stream subscribe (`client:*:ops`), role-based access control beyond what Sentry provides.
