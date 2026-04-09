# Herald Clustering — Cross-Instance Fanout

## Problem

Herald supports shared storage via remote ShroudB (`store.mode = "remote"`), enabling multiple instances to read/write the same data. However, **in-memory fanout is per-process**: a subscriber on instance A won't receive events published on instance B. This makes horizontal scaling impossible for real-time delivery.

Two gaps must be closed:

1. **Sequence counters** — currently `AtomicU64` per-process, so two instances can allocate the same sequence number, causing key collisions and data loss.
2. **Event fanout** — currently iterates local `ConnectionRegistry` only; remote subscribers are invisible.

## Design

### Backplane: ShroudB Store Subscriptions

The `Store` trait already exposes `subscribe(ns, filter) -> Subscription` with `recv() -> SubscriptionEvent`. Both `EmbeddedStore` and `RemoteStore` implement it. Herald wraps both in `BackendSubscription`. **This capability exists but is unused.**

Using ShroudB subscriptions as the backplane means:

- **Zero new dependencies** — no Redis, NATS, or additional infrastructure
- **Works in both store modes** — embedded (single-instance, subscription is local) and remote (multi-instance, subscription crosses instances via the shared ShroudB server)
- **Consistent with Herald's philosophy** — "no external database"

### Sequence Coordination

**Embedded mode**: No change. Single instance, `AtomicU64` is correct.

**Clustered mode** (remote store + `cluster.enabled`): Use `store.put()` return value (version number) as the sequence.

ShroudB guarantees per-key versions are monotonically increasing and unique. Each `put` to a counter key in `herald.sequences` namespace returns the next version: 1, 2, 3, ... Two concurrent puts from different instances get different versions — no collision possible.

```
allocate_seq(tenant, stream):
    key = "{tenant}/{stream}"
    version = store.put(NS_SEQUENCES, key, b"_", None)
    return version
```

**Migration**: When clustering is first enabled on a store with existing events, the counter key doesn't exist yet (version starts at 1), but events already have sequences up to N. On startup, Herald reads `latest_seq` from stored events and advances the counter key by doing N puts. This is a one-time operation per stream. For large streams, puts are batched.

### Cross-Instance Fanout

Each Herald instance spawns a **backplane consumer** task that:

1. Subscribes to `herald.events` namespace with `EventType::Put` filter
2. Receives `SubscriptionEvent` on each write (from any instance)
3. Parses the key to extract `tenant_id`, `stream_id`, `seq`
4. Skips ID-index keys (contain `/id/`)
5. Checks if the event was originated locally (via a bounded dedup set)
6. If remote: reads the event data from the shared store, constructs a `ServerMessage::EventNew`, fans out to local subscribers

**Local write tracking**: The publishing code path (WS `handle_publish` and HTTP `inject_event`) adds each event's primary key to a bounded concurrent set (`DashSet<Vec<u8>>`) on `AppState`. The backplane consumer checks this set before re-fanning out. Keys are removed after consumption, and the set is bounded (ring buffer semantics) to prevent memory growth.

### Message Flow (Clustered)

```
Client A (instance 1)
    │
    ▼
handle_publish
    │
    ├─ allocate_seq (store.put → version)
    ├─ store event (store.put to herald.events)
    ├─ record key in local_writes set
    ├─ local fanout (instance 1 subscribers)
    │
    └─ [ShroudB subscription fires on ALL instances]
         │
         ├─ Instance 1: key in local_writes → skip
         └─ Instance 2: key NOT in local_writes
              │
              ├─ read event from shared store
              └─ local fanout (instance 2 subscribers)
```

### Configuration

```toml
[cluster]
enabled = true          # default: false
instance_id = "node-1"  # optional, auto-generated UUID if omitted
```

Environment variables:
- `HERALD_CLUSTER_ENABLED=true`
- `HERALD_CLUSTER_INSTANCE_ID=node-1`

Clustering requires `store.mode = "remote"`. Validation fails if `cluster.enabled = true` with `store.mode = "embedded"`.

### What Changes

| Component | Before | After (clustered) |
|-----------|--------|-------------------|
| Sequence allocation | `AtomicU64::fetch_add` | `store.put(NS_SEQUENCES, key)` → version |
| Event fanout | Local `ConnectionRegistry` only | Local + backplane subscription consumer |
| Startup | Hydrate seq from `latest_seq` | + Initialize sequence counter keys |
| AppState | No cluster state | + `instance_id`, `local_writes: DashSet` |
| Config | No cluster section | + `[cluster]` section |
| Namespaces | 8 namespaces | + `herald.sequences` |

### What Doesn't Change

- **Connection management** — remains per-instance (each instance manages its own WebSocket connections)
- **Presence/typing** — remains per-instance ephemeral state (acceptable: presence queries hit local state, cross-instance presence would require separate protocol work)
- **Embedded mode** — completely unchanged, single-instance operation
- **Store layer** — no changes to how events are stored or queried
- **Client protocol** — no changes to the WebSocket or HTTP API
