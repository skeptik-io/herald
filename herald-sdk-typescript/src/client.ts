import { Connection, type ConnectionState } from "./connection.js";
import { HeraldError } from "./errors.js";
import { E2EEManager } from "./e2ee.js";
import { initE2EE } from "./crypto.js";
import type {
  CursorMoved,
  EventDelivered,
  EventReceived,
  HeraldClientOptions,
  HeraldEvent,
  HeraldEventMap,
  MemberEvent,
  EventAck,
  EventDeleted,
  EventEdited,
  EventNew,
  EventsBatch,
  PresenceChanged,
  ReactionChanged,
  StreamEvent,
  StreamSubscriberCount,
  ServerFrame,
  SubscribedPayload,
  TypingEvent,
  WatchlistEvent,
} from "./types.js";

type Handler<T> = (data: T) => void;

let refCounter = 0;
export function nextRef(): string {
  return `r${++refCounter}`;
}

/**
 * Herald WebSocket client for browsers.
 *
 * Connects to a Herald server, authenticates with a JWT, and provides
 * methods for subscribing to streams, publishing events, and receiving
 * real-time events.
 */
export class HeraldClient {
  private connection: Connection;
  private options: HeraldClientOptions;
  private token: string;
  private lastSeenAt: number | null = null;
  private seenMessageIds = new Set<string>();
  private _connectionId: number | null = null;
  private _initialConnectDone = false;

  // Track subscribed streams for re-subscribe on reconnect
  private subscribedStreams = new Set<string>();

  // Pending request/response correlation
  private pending = new Map<
    string,
    { resolve: (v: unknown) => void; reject: (e: Error) => void }
  >();

  // Accumulate subscribed responses for batch subscribe
  private pendingSubscribes = new Map<
    string,
    {
      streams: string[];
      results: SubscribedPayload[];
      resolve: (v: SubscribedPayload[]) => void;
    }
  >();

  // Event handlers
  private handlers = new Map<string, Set<Handler<unknown>>>();
  private globalHandlers = new Set<(event: string, data: unknown) => void>();

  // E2EE — null when e2ee option is not set (zero overhead)
  private e2ee: E2EEManager | null = null;

  // Ack mode — per-stream high-water-mark tracking
  private pendingAcks = new Map<string, number>();
  private ackTimer: ReturnType<typeof setTimeout> | null = null;
  private readonly ACK_DEBOUNCE_MS = 100;

  constructor(options: HeraldClientOptions) {
    this.options = options;
    this.token = options.token;
    if (options.e2ee) {
      this.e2ee = new E2EEManager();
    }
    this.connection = new Connection(
      options.url,
      options.reconnect?.enabled ?? true,
      options.reconnect?.maxDelay ?? 30_000,
      (frame) => this.handleFrame(frame),
      (state, previous) => this.handleStateChange(state, previous),
    );
  }

  get connected(): boolean {
    return this.connection.isConnected;
  }

  /** The connection ID assigned by the server. Available after auth. */
  get connectionId(): number | null {
    return this._connectionId;
  }

  async connect(): Promise<void> {
    if (this.e2ee) {
      await initE2EE();
    }
    await this.connection.connect();
    await this.authenticate();
    this._initialConnectDone = true;
  }

  disconnect(): void {
    this._initialConnectDone = false;
    this.subscribedStreams.clear();
    this.e2ee?.clear();
    this.flushAcks();
    this.connection.close();
    this.pending.clear();
    this.pendingSubscribes.clear();
  }

  // ── E2EE ──────────────────────────────────────────────────────────

  /** Set an E2EE session for a stream. Events to/from this stream will be encrypted. */
  setE2EESession(stream: string, session: import("./crypto.js").E2EESession): void {
    if (!this.e2ee) {
      throw new Error("E2EE not enabled. Set e2ee: true in HeraldClientOptions.");
    }
    this.e2ee.setSession(stream, session);
  }

  /** Remove an E2EE session for a stream. */
  removeE2EESession(stream: string): void {
    this.e2ee?.removeSession(stream);
  }

  // ── Streams ────────────────────────────────────────────────────────

  async subscribe(streams: string[]): Promise<SubscribedPayload[]> {
    const ref = nextRef();
    return new Promise((resolve, reject) => {
      this.pendingSubscribes.set(ref, { streams: [...streams], results: [], resolve });
      this.connection.send({
        type: "subscribe",
        ref,
        payload: { streams },
      });
      // Track for re-subscribe on reconnect
      for (const stream of streams) {
        this.subscribedStreams.add(stream);
      }
      // Timeout after 10 seconds
      setTimeout(() => {
        if (this.pendingSubscribes.has(ref)) {
          this.pendingSubscribes.delete(ref);
          reject(new HeraldError("TIMEOUT", "subscribe timed out"));
        }
      }, 10_000);
    });
  }

  unsubscribe(streams: string[]): void {
    for (const stream of streams) {
      this.subscribedStreams.delete(stream);
    }
    this.connection.send({
      type: "unsubscribe",
      payload: { streams },
    });
  }

  // ── Events ─────────────────────────────────────────────────────────

  async publish(stream: string, body: string, options?: { meta?: unknown; parentId?: string }): Promise<EventAck> {
    const ref = nextRef();
    const { body: finalBody, meta: finalMeta } = this.e2ee
      ? this.e2ee.encryptOutgoing(stream, body, options?.meta)
      : { body, meta: options?.meta };
    return this.request(ref, {
      type: "event.publish",
      ref,
      payload: { stream, body: finalBody, meta: finalMeta, parent_id: options?.parentId },
    }) as Promise<EventAck>;
  }

  async fetch(
    stream: string,
    options?: { before?: number; after?: number; limit?: number },
  ): Promise<EventsBatch> {
    const ref = nextRef();
    return this.request(ref, {
      type: "events.fetch",
      ref,
      payload: { stream, ...options },
    }) as Promise<EventsBatch>;
  }

  // ── Ephemeral Events ────────────────────────────────────────────────

  /** Trigger an ephemeral event (not persisted). */
  trigger(stream: string, event: string, data?: unknown): void {
    this.connection.send({
      type: "event.trigger",
      payload: { stream, event, data },
    });
  }

  // ── Frame primitives (for extension SDKs) ──────────────────────────

  /** Send a fire-and-forget frame. Used by extension SDKs (e.g. herald-chat-sdk). */
  sendFrame(frame: Record<string, unknown>): void {
    this.connection.send(frame);
  }

  /** Send a request frame and await a response. Used by extension SDKs. */
  requestFrame(ref: string, frame: Record<string, unknown>): Promise<unknown> {
    return this.request(ref, frame);
  }

  /** E2EE manager for extension SDKs that need to encrypt/decrypt. */
  get e2eeManager(): E2EEManager | null {
    return this.e2ee;
  }

  // ── Events ─────────────────────────────────────────────────────────

  on<E extends HeraldEvent>(event: E, handler: Handler<HeraldEventMap[E]>): void {
    if (!this.handlers.has(event)) {
      this.handlers.set(event, new Set());
    }
    this.handlers.get(event)!.add(handler as Handler<unknown>);
  }

  off<E extends HeraldEvent>(event: E, handler: Handler<HeraldEventMap[E]>): void {
    this.handlers.get(event)?.delete(handler as Handler<unknown>);
  }

  /** Listen to all events regardless of type (like Pusher's bind_global). */
  onAny(handler: (event: string, data: unknown) => void): void {
    this.globalHandlers.add(handler);
  }

  offAny(handler: (event: string, data: unknown) => void): void {
    this.globalHandlers.delete(handler);
  }

  // ── Internals ──────────────────────────────────────────────────────

  private emit<E extends HeraldEvent>(event: E, data: HeraldEventMap[E]): void {
    // Type-specific handlers
    const set = this.handlers.get(event);
    if (set) {
      for (const handler of set) {
        try {
          handler(data);
        } catch {
          // Don't let user handler errors break the event loop
        }
      }
    }
    // Global handlers
    for (const handler of this.globalHandlers) {
      try {
        handler(event, data);
      } catch { }
    }
  }

  private request(
    ref: string,
    frame: Record<string, unknown>,
  ): Promise<unknown> {
    return new Promise((resolve, reject) => {
      this.pending.set(ref, { resolve, reject });
      this.connection.send(frame);
      // Timeout after 30s
      setTimeout(() => {
        if (this.pending.has(ref)) {
          this.pending.delete(ref);
          reject(new HeraldError("TIMEOUT", "request timed out"));
        }
      }, 30_000);
    });
  }

  private async authenticate(): Promise<void> {
    const ref = nextRef();
    const payload: Record<string, unknown> = { token: this.token };
    if (this.options.ackMode) {
      payload.ack_mode = true;
      // In ack mode, do NOT send last_seen_at — the server uses cursor-based
      // catchup from the last acked sequence instead.
    } else if (this.lastSeenAt !== null) {
      payload.last_seen_at = this.lastSeenAt;
    }

    return new Promise((resolve, reject) => {
      this.pending.set(ref, {
        resolve: () => resolve(),
        reject: (e) => reject(e),
      });
      this.connection.send({ type: "auth", ref, payload });
    });
  }

  private handleFrame(frame: ServerFrame): void {
    const p = frame.payload as Record<string, unknown> | undefined;
    const ref = frame.ref;

    switch (frame.type) {
      case "auth_ok":
        this._connectionId = (p as any)?.connection_id ?? null;
        if (ref && this.pending.has(ref)) {
          this.pending.get(ref)!.resolve(p);
          this.pending.delete(ref);
        }
        break;

      case "auth_error":
        if (ref && this.pending.has(ref)) {
          const code = (p?.code as string) ?? "AUTH_ERROR";
          const msg = (p?.message as string) ?? "authentication failed";
          this.pending.get(ref)!.reject(new HeraldError(code, msg));
          this.pending.delete(ref);
        }
        break;

      case "subscribed": {
        const payload = p as unknown as SubscribedPayload;
        // Find the pending subscribe batch by ref
        if (ref && this.pendingSubscribes.has(ref)) {
          const batch = this.pendingSubscribes.get(ref)!;
          batch.results.push(payload);
          if (batch.results.length >= batch.streams.length) {
            batch.resolve(batch.results);
            this.pendingSubscribes.delete(ref);
          }
        }
        break;
      }

      case "event.new": {
        const msg = p as unknown as EventNew;
        // Track for reconnect catch-up dedup
        if (msg.sent_at && msg.sent_at > (this.lastSeenAt ?? 0)) {
          this.lastSeenAt = msg.sent_at;
        }
        // Dedup
        if (this.seenMessageIds.has(msg.id)) break;
        this.seenMessageIds.add(msg.id);
        // Cap dedup set size — keep the most recent 5000
        if (this.seenMessageIds.size > 10_000) {
          const ids = Array.from(this.seenMessageIds);
          this.seenMessageIds = new Set(ids.slice(-5_000));
        }
        // E2EE decrypt
        if (this.e2ee) {
          const result = this.e2ee.decryptIncoming(msg.stream, msg.body, msg.meta);
          msg.body = result.body;
          msg.meta = result.meta;
        }
        this.emit("event", msg);
        // At-least-once: ack the event
        if (this.options.ackMode) {
          this.scheduleAck(msg.stream, msg.seq);
        }
        break;
      }

      case "event.ack":
        if (ref && this.pending.has(ref)) {
          this.pending.get(ref)!.resolve(p);
          this.pending.delete(ref);
        }
        break;

      case "events.batch": {
        const batch = p as unknown as EventsBatch;
        // E2EE decrypt
        if (this.e2ee && batch) {
          for (const evt of batch.events) {
            const result = this.e2ee.decryptIncoming(batch.stream, evt.body, evt.meta);
            evt.body = result.body;
            evt.meta = result.meta;
          }
        }
        if (ref && this.pending.has(ref)) {
          // Response to explicit fetch() call
          this.pending.get(ref)!.resolve(p);
          this.pending.delete(ref);
        } else if (batch) {
          // Server-pushed catchup batch (reconnect) — emit each event
          let highestSeq = 0;
          for (const evt of batch.events) {
            if (this.seenMessageIds.has(evt.id)) continue;
            this.seenMessageIds.add(evt.id);
            if (evt.sent_at && evt.sent_at > (this.lastSeenAt ?? 0)) {
              this.lastSeenAt = evt.sent_at;
            }
            if (evt.seq > highestSeq) highestSeq = evt.seq;
            this.emit("event", evt as unknown as EventNew);
          }
          // At-least-once: ack the highest seq in the batch
          if (this.options.ackMode && highestSeq > 0) {
            this.scheduleAck(batch.stream, highestSeq);
          }
          // Auto-paginate: if server has more events, fetch the next page
          if (batch.has_more && batch.events.length > 0) {
            const lastSeq = batch.events[batch.events.length - 1].seq;
            this.fetch(batch.stream, { after: lastSeq }).then((next) => {
              // Re-emit as server-pushed batch by synthesizing a frame
              this.handleFrame({
                type: "events.batch",
                payload: next,
              });
            }).catch(() => {
              // Pagination failure is non-fatal — client has partial catchup
            });
          }
        }
        break;
      }

      case "presence.changed":
        this.emit("presence", p as unknown as PresenceChanged);
        break;

      case "cursor.moved":
        this.emit("cursor", p as unknown as CursorMoved);
        break;

      case "member.joined":
        this.emit("member.joined", p as unknown as MemberEvent);
        break;

      case "member.left":
        this.emit("member.left", p as unknown as MemberEvent);
        break;

      case "typing":
        this.emit("typing", p as unknown as TypingEvent);
        break;

      case "stream.updated":
        this.emit("stream.updated", p as unknown as StreamEvent);
        break;

      case "stream.deleted":
        this.emit("stream.deleted", p as unknown as StreamEvent);
        break;

      case "event.deleted":
        this.emit("event.deleted", p as unknown as EventDeleted);
        break;

      case "event.edited": {
        const edited = p as unknown as EventEdited;
        if (this.e2ee) {
          const result = this.e2ee.decryptIncoming(edited.stream, edited.body);
          edited.body = result.body;
        }
        this.emit("event.edited", edited);
        break;
      }

      case "reaction.changed":
        this.emit("reaction.changed", p as unknown as ReactionChanged);
        break;

      case "event.received":
        this.emit("event.received", p as unknown as EventReceived);
        break;

      case "event.delivered":
        this.emit("event.delivered", p as unknown as EventDelivered);
        break;

      case "stream.subscriber_count":
        this.emit("stream.subscriber_count", p as unknown as StreamSubscriberCount);
        break;

      case "watchlist.online":
        this.emit("watchlist.online", p as unknown as WatchlistEvent);
        break;

      case "watchlist.offline":
        this.emit("watchlist.offline", p as unknown as WatchlistEvent);
        break;

      case "system.token_expiring":
        this.handleTokenExpiring();
        break;

      case "error":
        if (ref && this.pending.has(ref)) {
          const code = (p?.code as string) ?? "INTERNAL";
          const msg = (p?.message as string) ?? "unknown error";
          this.pending.get(ref)!.reject(new HeraldError(code, msg));
          this.pending.delete(ref);
        }
        this.emit("error", p as unknown as { code: string; message: string });
        break;

      case "pong":
        // No-op
        break;
    }
  }

  private handleStateChange(state: ConnectionState, previous?: ConnectionState): void {
    // Emit specific state events
    switch (state) {
      case "connected":
        // On reconnect (not initial connect), re-authenticate and re-subscribe
        if (this._initialConnectDone) {
          this.handleReconnect();
        }
        this.emit("connected", undefined as never);
        break;
      case "disconnected":
        this._connectionId = null;
        // Clear pending requests — they'll never resolve on a dead connection
        for (const [, p] of this.pending) {
          p.reject(new HeraldError("DISCONNECTED", "connection lost"));
        }
        this.pending.clear();
        this.pendingSubscribes.clear();
        this.emit("disconnected", undefined as never);
        break;
      case "connecting":
      case "unavailable":
        this.emit("reconnecting", undefined as never);
        break;
    }
    // Emit generic state_change via global handlers
    for (const handler of this.globalHandlers) {
      try {
        handler("state_change", { previous, current: state });
      } catch { }
    }
  }

  private async handleReconnect(): Promise<void> {
    try {
      // Re-authenticate with the current token
      await this.authenticate();

      // Re-subscribe to all previously subscribed streams
      if (this.subscribedStreams.size > 0) {
        const streams = Array.from(this.subscribedStreams);
        // Fire-and-forget subscribe — don't block on it, just send the frame.
        // The subscribed responses will come through handleFrame normally.
        this.connection.send({
          type: "subscribe",
          payload: { streams },
        });
      }
    } catch {
      // Auth failed on reconnect — the server will close the connection,
      // triggering another reconnect attempt. Don't emit error here to
      // avoid confusing the consumer; the connection state machine handles it.
    }
  }

  /** Buffer an ack for a stream and schedule a debounced flush. */
  private scheduleAck(stream: string, seq: number): void {
    const current = this.pendingAcks.get(stream) ?? 0;
    if (seq > current) {
      this.pendingAcks.set(stream, seq);
    }
    if (this.ackTimer === null) {
      this.ackTimer = setTimeout(() => {
        this.ackTimer = null;
        this.flushAcks();
      }, this.ACK_DEBOUNCE_MS);
    }
  }

  /** Send all buffered acks immediately. */
  private flushAcks(): void {
    if (this.ackTimer !== null) {
      clearTimeout(this.ackTimer);
      this.ackTimer = null;
    }
    for (const [stream, seq] of this.pendingAcks) {
      this.connection.send({
        type: "event.ack",
        payload: { stream, seq },
      });
    }
    this.pendingAcks.clear();
  }

  private async handleTokenExpiring(): Promise<void> {
    if (this.options.onTokenExpiring) {
      try {
        const newToken = await this.options.onTokenExpiring();
        this.token = newToken;
        this.connection.send({
          type: "auth.refresh",
          payload: { token: newToken },
        });
      } catch {
        // Token refresh failed — connection will be closed by server
      }
    }
  }
}
