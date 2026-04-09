# At-Least-Once Delivery — Design Document

## Problem

Herald currently provides **at-most-once** delivery semantics. Events are fanned out via bounded in-memory channels (256-message capacity). If a connection's channel is full when fanout fires, the event is silently dropped. On reconnect, the client sends a `last_seen_at` timestamp and the server replays events with `sent_at > last_seen_at`, but:

1. The timestamp is coarse — clock skew can cause missed events.
2. The server has no knowledge of what the client actually processed.
3. If a connection drops between receiving an event and the client updating `last_seen_at`, that event is lost.

## Solution: Opt-In Ack Mode

Add an opt-in **ack mode** where the client explicitly acknowledges processed events using a per-stream sequence high-water-mark. On reconnect, the server replays from the last acked sequence instead of a timestamp.

### Why High-Water-Mark (Not Per-Event Acks)

Per-event acking would impose unacceptable overhead for realtime fanout to thousands of subscribers. A high-water-mark approach — "I have processed all events up to seq 42 on stream S" — matches Kafka consumer offset semantics and composes naturally with Herald's existing monotonic sequence numbers and cursor system.

### Why Reconnect-Only Redelivery (Not Timer-Based)

Timer-based redelivery during active connections adds complexity (redelivery state machines, exponential backoff per subscriber) for marginal benefit. The bounded channel already provides backpressure. The real gap is: events sent between the last ack and a connection drop are lost. Fixing this with seq-based reconnect catchup is sufficient. The ack high-water-mark makes the catchup window precise.

## Protocol Changes

### New Client → Server Frame: `event.ack`

```json
{
  "type": "event.ack",
  "payload": {
    "stream": "stream_a",
    "seq": 42
  }
}
```

Meaning: "I have processed all events up to and including seq 42 on stream_a."

No server response. Fire-and-forget, like `cursor.update`.

### Auth Frame Enhancement

```json
{
  "type": "auth",
  "payload": {
    "token": "JWT...",
    "ack_mode": true
  }
}
```

Connections that set `ack_mode: true` get seq-based reconnect catchup. Connections without this flag (or `false`) get the existing timestamp-based catchup — full backwards compatibility.

When `ack_mode` is true, the client should NOT send `last_seen_at`. The server uses the stored cursor (last acked seq) per stream instead.

## Architecture

### What Changes

1. **Connection state**: Each `ConnectionHandle` gains an `ack_enabled: bool` flag and an `acked_seqs: HashMap<String, u64>` (stream_id → last_acked_seq). Updated on each `event.ack` frame with MAX semantics.

2. **Reconnect catchup**: For ack-enabled connections, `reconnect_catchup()` reads the cursor (acked seq) from the store per stream and uses `list_after(acked_seq)` instead of `list_since_time(timestamp)`.

3. **Disconnect flush**: On disconnect, the in-memory acked seqs are flushed to the cursor store (`NS_CURSORS`) so they survive connection death and are available on reconnect — including to different instances in clustered mode.

4. **Client SDK**: New `ackMode` option. When enabled, the client sends `event.ack` frames with debouncing (100ms) to batch acks. On reconnect, sends `ack_mode: true` instead of `last_seen_at`.

### What Stays The Same

- **Fanout path**: `fanout_to_stream()` → `send_to_conn()` → bounded channel → WebSocket writer. Still fire-and-forget. Dropped events are caught by the ack gap on reconnect.
- **Event storage**: No changes. Events already have unique IDs, monotonic sequences, and TTL-based retention.
- **Backplane**: No changes. Cross-instance fanout uses the same fanout path. Ack state is per-connection (in-memory) and flushed to the shared cursor store on disconnect.
- **Client dedup**: The `seenMessageIds` set is retained as defense-in-depth against duplicate delivery during catchup overlap.

### Delivery Guarantee

**At-least-once**: If an event is stored, it will eventually be delivered to every ack-mode subscriber (within the event's TTL window). Events may be delivered more than once — the client is responsible for idempotency (the existing dedup set handles this).

**Not exactly-once**: Between the last ack and a connection drop, events will be redelivered on reconnect. This is by design.

### Edge Cases

- **No cursor exists**: If a client connects with `ack_mode` but has never acked anything for a stream, no catchup is sent for that stream. The client is treated as a fresh subscriber.
- **Server crash**: In-memory acked seqs are lost. On reconnect, the last persisted cursor (from a previous disconnect flush) is used. The redelivery window widens to include events since the last clean disconnect.
- **Clustered mode**: Cursors are stored in the shared store (ShroudB). A client that reconnects to a different instance gets correct catchup because the cursor was flushed on disconnect from the previous instance.

## Metrics

- `herald_events_acked_total` — Counter of event.ack frames processed.
- `herald_events_redelivered_total` — Counter of events sent during ack-mode reconnect catchup.
