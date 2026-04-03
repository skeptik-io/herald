import { Connection, type ConnectionState } from "./connection.js";
import { HeraldError } from "./errors.js";
import type {
  CursorMoved,
  EventReceived,
  HeraldClientOptions,
  HeraldEvent,
  HeraldEventMap,
  MemberEvent,
  MessageAck,
  MessageDeleted,
  MessageEdited,
  MessageNew,
  MessagesBatch,
  PresenceChanged,
  ReactionChanged,
  RoomEvent,
  RoomSubscriberCount,
  ServerFrame,
  SubscribedPayload,
  TypingEvent,
  WatchlistEvent,
} from "./types.js";

type Handler<T> = (data: T) => void;

let refCounter = 0;
function nextRef(): string {
  return `r${++refCounter}`;
}

/**
 * Herald WebSocket client for browsers.
 *
 * Connects to a Herald server, authenticates with a JWT, and provides
 * methods for subscribing to rooms, sending messages, and receiving
 * real-time events.
 */
export class HeraldClient {
  private connection: Connection;
  private options: HeraldClientOptions;
  private token: string;
  private lastSeenAt: number | null = null;
  private seenMessageIds = new Set<string>();
  private _connectionId: number | null = null;

  // Pending request/response correlation
  private pending = new Map<
    string,
    { resolve: (v: unknown) => void; reject: (e: Error) => void }
  >();

  // Accumulate subscribed responses for batch subscribe
  private pendingSubscribes = new Map<
    string,
    {
      rooms: string[];
      results: SubscribedPayload[];
      resolve: (v: SubscribedPayload[]) => void;
    }
  >();

  // Event handlers
  private handlers = new Map<string, Set<Handler<unknown>>>();
  private globalHandlers = new Set<(event: string, data: unknown) => void>();

  constructor(options: HeraldClientOptions) {
    this.options = options;
    this.token = options.token;
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
    await this.connection.connect();
    await this.authenticate();
  }

  disconnect(): void {
    this.connection.close();
    this.pending.clear();
    this.pendingSubscribes.clear();
  }

  // ── Rooms ──────────────────────────────────────────────────────────

  async subscribe(rooms: string[]): Promise<SubscribedPayload[]> {
    const ref = nextRef();
    return new Promise((resolve, reject) => {
      this.pendingSubscribes.set(ref, { rooms: [...rooms], results: [], resolve });
      this.connection.send({
        type: "subscribe",
        ref,
        payload: { rooms },
      });
      // Timeout after 10 seconds
      setTimeout(() => {
        if (this.pendingSubscribes.has(ref)) {
          this.pendingSubscribes.delete(ref);
          reject(new HeraldError("TIMEOUT", "subscribe timed out"));
        }
      }, 10_000);
    });
  }

  unsubscribe(rooms: string[]): void {
    this.connection.send({
      type: "unsubscribe",
      payload: { rooms },
    });
  }

  // ── Messages ───────────────────────────────────────────────────────

  async send(room: string, body: string, options?: { meta?: unknown; parentId?: string }): Promise<MessageAck> {
    const ref = nextRef();
    return this.request(ref, {
      type: "message.send",
      ref,
      payload: { room, body, meta: options?.meta, parent_id: options?.parentId },
    }) as Promise<MessageAck>;
  }

  async editMessage(room: string, id: string, body: string): Promise<MessageAck> {
    const ref = nextRef();
    return this.request(ref, {
      type: "message.edit",
      ref,
      payload: { room, id, body },
    }) as Promise<MessageAck>;
  }

  addReaction(room: string, messageId: string, emoji: string): void {
    this.connection.send({
      type: "reaction.add",
      payload: { room, message_id: messageId, emoji },
    });
  }

  removeReaction(room: string, messageId: string, emoji: string): void {
    this.connection.send({
      type: "reaction.remove",
      payload: { room, message_id: messageId, emoji },
    });
  }

  async fetch(
    room: string,
    options?: { before?: number; limit?: number },
  ): Promise<MessagesBatch> {
    const ref = nextRef();
    return this.request(ref, {
      type: "messages.fetch",
      ref,
      payload: { room, ...options },
    }) as Promise<MessagesBatch>;
  }

  // ── Presence ───────────────────────────────────────────────────────

  setPresence(status: "online" | "away" | "dnd"): void {
    this.connection.send({
      type: "presence.set",
      payload: { status },
    });
  }

  // ── Cursors ────────────────────────────────────────────────────────

  updateCursor(room: string, seq: number): void {
    this.connection.send({
      type: "cursor.update",
      payload: { room, seq },
    });
  }

  // ── Typing ─────────────────────────────────────────────────────────

  startTyping(room: string): void {
    this.connection.send({
      type: "typing.start",
      payload: { room },
    });
  }

  stopTyping(room: string): void {
    this.connection.send({
      type: "typing.stop",
      payload: { room },
    });
  }

  // ── Ephemeral Events ────────────────────────────────────────────────

  /** Trigger an ephemeral event (not persisted). */
  trigger(room: string, event: string, data?: unknown): void {
    this.connection.send({
      type: "event.trigger",
      payload: { room, event, data },
    });
  }

  /** Delete a message. */
  async deleteMessage(room: string, id: string): Promise<MessageAck> {
    const ref = nextRef();
    return this.request(ref, {
      type: "message.delete",
      ref,
      payload: { room, id },
    }) as Promise<MessageAck>;
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
    if (this.lastSeenAt !== null) {
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
          if (batch.results.length >= batch.rooms.length) {
            batch.resolve(batch.results);
            this.pendingSubscribes.delete(ref);
          }
        }
        break;
      }

      case "message.new": {
        const msg = p as unknown as MessageNew;
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
        this.emit("message", msg);
        break;
      }

      case "message.ack":
        if (ref && this.pending.has(ref)) {
          this.pending.get(ref)!.resolve(p);
          this.pending.delete(ref);
        }
        break;

      case "messages.batch":
        if (ref && this.pending.has(ref)) {
          this.pending.get(ref)!.resolve(p);
          this.pending.delete(ref);
        }
        break;

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

      case "room.updated":
        this.emit("room.updated", p as unknown as RoomEvent);
        break;

      case "room.deleted":
        this.emit("room.deleted", p as unknown as RoomEvent);
        break;

      case "message.deleted":
        this.emit("message.deleted", p as unknown as MessageDeleted);
        break;

      case "message.edited":
        this.emit("message.edited", p as unknown as MessageEdited);
        break;

      case "reaction.changed":
        this.emit("reaction.changed", p as unknown as ReactionChanged);
        break;

      case "event.received":
        this.emit("event.received", p as unknown as EventReceived);
        break;

      case "room.subscriber_count":
        this.emit("room.subscriber_count", p as unknown as RoomSubscriberCount);
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
        this.emit("connected", undefined as never);
        break;
      case "disconnected":
        this._connectionId = null;
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
