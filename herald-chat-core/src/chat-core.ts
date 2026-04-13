import type { HeraldClient, EventNew, EventEdited, EventDeleted, EventAck, EventsBatch,
  ReactionChanged, PresenceChanged, CursorMoved, MemberEvent, TypingEvent,
  EventReceived, EventDelivered, SubscribedPayload } from "herald-sdk";
import type { HeraldChatClient } from "herald-chat-sdk";
import type { HeraldPresenceClient, PresenceStatus } from "herald-presence-sdk";
import type { ChatCoreOptions, ChatWriter, Message, MessageAck, PendingMessage, Member, ScrollStateSnapshot, LivenessState, ChatEvent, Middleware } from "./types.js";
import { Notifier } from "./notifier.js";
import { MessageStore } from "./stores/message-store.js";
import { CursorStore } from "./stores/cursor-store.js";
import { MemberStore } from "./stores/member-store.js";
import { TypingStore } from "./stores/typing-store.js";
import { LivenessController, browserEnvironment } from "./liveness/liveness.js";
import { ScrollState } from "./scroll/scroll-state.js";
import {
  encodeEnvelope,
  parseEnvelope,
  type ChatEnvelope,
  type ReactionEnvelope,
  type EditEnvelope,
  type DeleteEnvelope,
  type CursorEnvelope,
} from "./envelope.js";

type Handler<T> = (data: T) => void;

export class ChatCore {
  private client: HeraldClient;
  private chat: HeraldChatClient;
  private presenceClient: HeraldPresenceClient | null = null;
  private userId: string;
  private manualPresenceOverride: PresenceStatus | null = null;
  private notifier: Notifier;
  private messages: MessageStore;
  private cursors: CursorStore;
  private members: MemberStore;
  private typing: TypingStore;
  private scrollStates = new Map<string, ScrollState>();
  private liveness: LivenessController | null = null;
  private livenessState: LivenessState = "active";
  private scrollIdleMs: number;
  private loadMoreLimit: number;
  private middleware: Middleware[];
  private writer: ChatWriter | null = null;
  private readTimers = new Map<string, ReturnType<typeof setTimeout>>();
  private attached = false;
  private listenOnly = new Set<string>();
  private lastEphemeral = new Map<string, Map<string, EventReceived>>();

  // Track pending optimistic sends for retry/cancel
  private pendingSends = new Map<string, { streamId: string; body: string; meta?: unknown; parentId?: string }>();

  // Typing throttle: at most one frame per TYPING_THROTTLE_MS per stream
  private lastTypingSent = new Map<string, number>();
  private static readonly TYPING_THROTTLE_MS = 2000;

  // Bound handlers for clean detach
  private _onEvent: Handler<EventNew>;
  private _onEdited: Handler<EventEdited>;
  private _onDeleted: Handler<EventDeleted>;
  private _onReaction: Handler<ReactionChanged>;
  private _onPresence: Handler<PresenceChanged>;
  private _onCursor: Handler<CursorMoved>;
  private _onTyping: Handler<TypingEvent>;
  private _onMemberJoined: Handler<MemberEvent>;
  private _onMemberLeft: Handler<MemberEvent>;
  private _onEphemeral: Handler<EventReceived>;
  private _onDelivered: Handler<EventDelivered>;
  private _onConnected: Handler<void>;
  private _onDisconnected: Handler<void>;

  constructor(options: ChatCoreOptions) {
    this.client = options.client;
    this.chat = options.chat;
    this.presenceClient = options.presence ?? null;
    this.userId = options.userId;
    this.scrollIdleMs = options.scrollIdleMs ?? 1000;
    this.loadMoreLimit = options.loadMoreLimit ?? 50;
    this.middleware = options.middleware ?? [];
    this.writer = options.writer ?? null;

    this.notifier = new Notifier();
    this.messages = new MessageStore(this.notifier);
    this.cursors = new CursorStore(this.notifier);
    this.members = new MemberStore(this.notifier);
    this.typing = new TypingStore(this.notifier);

    // Liveness (optional, browser-only)
    if (options.liveness) {
      this.liveness = new LivenessController(
        browserEnvironment(),
        options.liveness,
        (state) => {
          this.livenessState = state;
          this.notifier.notify("liveness");
          this.syncPresence(state);
        },
      );
    }

    // Bind handlers
    this._onEvent = (e) => this.handleEvent(e);
    this._onEdited = (e) => this.handleEdited(e);
    this._onDeleted = (e) => this.handleDeleted(e);
    this._onReaction = (e) => this.handleReaction(e);
    this._onPresence = (e) => this.handlePresence(e);
    this._onCursor = (e) => this.handleCursor(e);
    this._onTyping = (e) => this.handleTyping(e);
    this._onMemberJoined = (e) => this.handleMemberJoined(e);
    this._onMemberLeft = (e) => this.handleMemberLeft(e);
    this._onEphemeral = (e) => this.handleEphemeral(e);
    this._onDelivered = (e) => this.handleDelivered(e);
    this._onConnected = () => this.handleConnected();
    this._onDisconnected = () => this.handleDisconnected();
  }

  // ── Lifecycle ────────────────────────────────────────────────────

  attach(): void {
    if (this.attached) return;
    this.attached = true;

    this.client.on("event", this._onEvent);
    this.client.on("event.edited", this._onEdited);
    this.client.on("event.deleted", this._onDeleted);
    this.client.on("reaction.changed", this._onReaction);
    this.client.on("presence", this._onPresence);
    this.client.on("cursor", this._onCursor);
    this.client.on("typing", this._onTyping);
    this.client.on("member.joined", this._onMemberJoined);
    this.client.on("member.left", this._onMemberLeft);
    this.client.on("event.received", this._onEphemeral);
    this.client.on("event.delivered", this._onDelivered);
    this.client.on("connected", this._onConnected);
    this.client.on("disconnected", this._onDisconnected);

    this.typing.startExpiry();
    this.liveness?.attach();
  }

  detach(): void {
    if (!this.attached) return;
    this.attached = false;

    this.client.off("event", this._onEvent);
    this.client.off("event.edited", this._onEdited);
    this.client.off("event.deleted", this._onDeleted);
    this.client.off("reaction.changed", this._onReaction);
    this.client.off("presence", this._onPresence);
    this.client.off("cursor", this._onCursor);
    this.client.off("typing", this._onTyping);
    this.client.off("member.joined", this._onMemberJoined);
    this.client.off("member.left", this._onMemberLeft);
    this.client.off("event.received", this._onEphemeral);
    this.client.off("event.delivered", this._onDelivered);
    this.client.off("connected", this._onConnected);
    this.client.off("disconnected", this._onDisconnected);

    this.typing.stopExpiry();
    this.liveness?.detach();
  }

  destroy(): void {
    this.detach();
    for (const timer of this.readTimers.values()) clearTimeout(timer);
    this.readTimers.clear();
    this.messages.clearAll();
    this.cursors.clearAll();
    this.members.clearAll();
    this.typing.clearAll();
    this.scrollStates.clear();
    this.listenOnly.clear();
    this.lastEphemeral.clear();
    this.notifier.clear();
  }

  // ── Stream management ────────────────────────────────────────────

  async joinStream(streamId: string): Promise<SubscribedPayload> {
    const [payload] = await this.client.subscribe([streamId]);
    this.cursors.initStream(streamId, payload.cursor, payload.latest_seq);
    this.members.setMembers(streamId, payload.members);
    if (!this.scrollStates.has(streamId)) {
      this.scrollStates.set(streamId, new ScrollState(streamId, this.notifier));
    }
    return payload;
  }

  leaveStream(streamId: string): void {
    this.client.unsubscribe([streamId]);
    this.cancelAutoMarkRead(streamId);
    this.messages.clear(streamId);
    this.cursors.clear(streamId);
    this.members.clear(streamId);
    this.typing.clear(streamId);
    this.scrollStates.delete(streamId);
    this.lastEphemeral.delete(streamId);
    this.lastTypingSent.delete(streamId);
  }

  async listen(streamId: string): Promise<void> {
    this.listenOnly.add(streamId);
    const [payload] = await this.client.subscribe([streamId]);
    // Initialize cursors so unread counts work for listen-only streams
    this.cursors.initStream(streamId, payload.cursor, payload.latest_seq);
  }

  unlisten(streamId: string): void {
    this.listenOnly.delete(streamId);
    this.lastEphemeral.delete(streamId);
    this.cursors.clear(streamId);
    this.client.unsubscribe([streamId]);
  }

  // ── Actions ──────────────────────────────────────────────────────

  async send(
    streamId: string,
    body: string,
    opts?: { meta?: unknown; parentId?: string },
  ): Promise<PendingMessage> {
    const localId = `local:${crypto.randomUUID()}`;
    this.messages.addOptimistic(streamId, localId, this.userId, body, opts?.meta, opts?.parentId);
    this.pendingSends.set(localId, { streamId, body, ...opts });

    const pending = this.createPendingMessage(localId);

    try {
      const ack = await this.dispatchSend(localId, streamId, body, opts);
      this.messages.reconcile(localId, ack);
      this.pendingSends.delete(localId);
      return pending;
    } catch {
      this.messages.failOptimistic(localId);
      throw new Error("Send failed");
    }
  }

  async retrySend(localId: string): Promise<string> {
    const pending = this.pendingSends.get(localId);
    if (!pending) throw new Error("No pending send for localId");

    // Remove the failed optimistic and re-add as sending
    this.messages.removeOptimistic(localId);
    const newLocalId = `local:${crypto.randomUUID()}`;
    this.messages.addOptimistic(pending.streamId, newLocalId, this.userId, pending.body, pending.meta, pending.parentId);
    this.pendingSends.delete(localId);
    this.pendingSends.set(newLocalId, pending);

    try {
      const ack = await this.dispatchSend(newLocalId, pending.streamId, pending.body, {
        meta: pending.meta,
        parentId: pending.parentId,
      });
      this.messages.reconcile(newLocalId, ack);
      this.pendingSends.delete(newLocalId);
      return ack.id;
    } catch {
      this.messages.failOptimistic(newLocalId);
      throw new Error("Retry failed");
    }
  }

  cancelSend(localId: string): void {
    this.messages.removeOptimistic(localId);
    this.pendingSends.delete(localId);
  }

  async edit(streamId: string, eventId: string, body: string): Promise<void> {
    if (this.writer?.edit) {
      await this.writer.edit(streamId, eventId, body);
    } else {
      await this.chat.editEvent(streamId, eventId, body);
    }
  }

  async deleteEvent(streamId: string, eventId: string): Promise<void> {
    if (this.writer?.delete) {
      await this.writer.delete(streamId, eventId);
    } else {
      await this.chat.deleteEvent(streamId, eventId);
    }
  }

  async addReaction(streamId: string, eventId: string, emoji: string): Promise<void> {
    // Optimistic: apply locally so the UI updates immediately. The server's
    // echo (envelope or writer-driven fanout) will re-apply idempotently
    // (Set add for the same user is a no-op).
    this.messages.applyReaction({
      stream: streamId, event_id: eventId, user_id: this.userId, emoji, action: "add",
    });
    try {
      if (this.writer?.addReaction) {
        await this.writer.addReaction(streamId, eventId, emoji);
      } else {
        const env = encodeEnvelope({ kind: "reaction", targetId: eventId, op: "add", emoji });
        await this.client.publish(streamId, env);
      }
    } catch (err) {
      // Rollback: inverse op restores the pre-optimistic state. Idempotent
      // if the user already had no such reaction (rare race).
      this.messages.applyReaction({
        stream: streamId, event_id: eventId, user_id: this.userId, emoji, action: "remove",
      });
      throw err;
    }
  }

  async removeReaction(streamId: string, eventId: string, emoji: string): Promise<void> {
    this.messages.applyReaction({
      stream: streamId, event_id: eventId, user_id: this.userId, emoji, action: "remove",
    });
    try {
      if (this.writer?.removeReaction) {
        await this.writer.removeReaction(streamId, eventId, emoji);
      } else {
        const env = encodeEnvelope({ kind: "reaction", targetId: eventId, op: "remove", emoji });
        await this.client.publish(streamId, env);
      }
    } catch (err) {
      this.messages.applyReaction({
        stream: streamId, event_id: eventId, user_id: this.userId, emoji, action: "add",
      });
      throw err;
    }
  }

  /** Route a send through the app writer if provided, otherwise the Herald
   *  WebSocket. Both paths resolve to the same reconcile shape. */
  private async dispatchSend(
    localId: string,
    streamId: string,
    body: string,
    opts?: { meta?: unknown; parentId?: string },
  ): Promise<MessageAck> {
    if (this.writer?.send) {
      return this.writer.send({
        localId,
        streamId,
        body,
        meta: opts?.meta,
        parentId: opts?.parentId,
        senderId: this.userId,
      });
    }
    return this.client.publish(streamId, body, {
      meta: opts?.meta,
      parentId: opts?.parentId,
    });
  }

  startTyping(streamId: string): void {
    const now = Date.now();
    const last = this.lastTypingSent.get(streamId) ?? 0;
    if (now - last < ChatCore.TYPING_THROTTLE_MS) return;
    this.lastTypingSent.set(streamId, now);
    this.chat.startTyping(streamId);
  }

  stopTyping(streamId: string): void {
    this.chat.stopTyping(streamId);
  }

  /**
   * Update the `meta` field of a message in the store and notify subscribers.
   * Useful for application-specific metadata (e.g. pinned state) that isn't
   * covered by Herald's built-in edit/delete operations.
   */
  updateMessageMeta(streamId: string, messageId: string, meta: unknown): void {
    this.messages.updateMeta(streamId, messageId, meta);
  }

  // ── Read state ───────────────────────────────────────────────────

  getMessages(streamId: string): Message[] {
    return this.messages.getMessages(streamId);
  }

  getMembers(streamId: string): Member[] {
    return this.members.getMembers(streamId);
  }

  getTypingUsers(streamId: string): string[] {
    return this.typing.getTypingUsers(streamId);
  }

  getUnreadCount(streamId: string): number {
    return this.cursors.getUnreadCount(streamId);
  }

  getTotalUnreadCount(): number {
    return this.cursors.getTotalUnreadCount();
  }

  getScrollState(streamId: string): ScrollStateSnapshot {
    return this.scrollStates.get(streamId)?.getSnapshot() ?? DEFAULT_SCROLL;
  }

  getLivenessState(): LivenessState {
    return this.livenessState;
  }

  getLastEphemeral(streamId: string, eventType?: string): EventReceived | undefined {
    const byType = this.lastEphemeral.get(streamId);
    if (!byType) return undefined;
    if (eventType) return byType.get(eventType);
    // No type filter — return the most recently stored (last entry in insertion-order Map)
    let last: EventReceived | undefined;
    for (const ev of byType.values()) last = ev;
    return last;
  }

  getRemoteCursors(streamId: string): Map<string, number> {
    return this.cursors.getRemoteCursors(streamId);
  }

  // ── Scroll coordination ──────────────────────────────────────────

  setAtLiveEdge(streamId: string, atEdge: boolean): void {
    const scroll = this.scrollStates.get(streamId);
    if (!scroll) return;
    scroll.setAtLiveEdge(atEdge);
    if (atEdge) {
      this.scheduleAutoMarkRead(streamId);
    } else {
      this.cancelAutoMarkRead(streamId);
    }
  }

  /**
   * Seed the message store with externally-sourced history (e.g. from a
   * database). Call after `joinStream()` so cursor tracking is initialized.
   *
   * Events are deduplicated and inserted in seq order via `prependBatch`.
   */
  seedHistory(streamId: string, events: EventNew[], hasMore: boolean): void {
    this.messages.prependBatch(streamId, events, hasMore);
    if (events.length > 0) {
      const maxSeq = Math.max(...events.map((e) => e.seq));
      this.cursors.bumpLatestSeq(streamId, maxSeq);
    }
  }

  async loadMore(streamId: string): Promise<boolean> {
    const scroll = this.scrollStates.get(streamId);
    if (scroll?.getSnapshot().isLoadingMore) return false;

    const oldest = this.messages.getOldestSeq(streamId);
    if (!this.messages.hasMoreHistory(streamId)) return false;

    scroll?.setLoadingMore(true);
    try {
      const batch = await this.client.fetch(streamId, {
        before: oldest,
        limit: this.loadMoreLimit,
      });
      this.messages.prependBatch(streamId, batch.events, batch.has_more);
      return batch.has_more;
    } finally {
      scroll?.setLoadingMore(false);
    }
  }

  /**
   * Like `loadMore()` but uses an external fetcher instead of the Herald
   * server. Useful when the Herald WAL's TTL has expired and history must
   * come from an application database.
   */
  async loadMoreWith(
    streamId: string,
    fetcher: (before: number, limit: number) => Promise<{ events: EventNew[]; hasMore: boolean }>,
  ): Promise<boolean> {
    const scroll = this.scrollStates.get(streamId);
    if (scroll?.getSnapshot().isLoadingMore) return false;

    const oldest = this.messages.getOldestSeq(streamId);
    if (!this.messages.hasMoreHistory(streamId)) return false;

    scroll?.setLoadingMore(true);
    try {
      const batch = await fetcher(oldest!, this.loadMoreLimit);
      this.messages.prependBatch(streamId, batch.events, batch.hasMore);
      return batch.hasMore;
    } finally {
      scroll?.setLoadingMore(false);
    }
  }

  // ── Change notification ──────────────────────────────────────────

  subscribe(slice: string, listener: () => void): () => void {
    return this.notifier.subscribe(slice, listener);
  }

  // ── Internal event handlers ──────────────────────────────────────

  private handleEvent(event: EventNew): void {
    const envelope = parseEnvelope(event.body);

    // Non-message envelopes ride the same publish path but are folded into
    // chat state via their own dispatchers (reactions/edits/deletes/cursors
    // are not "messages" and must not be appended to the message list).
    if (envelope && envelope.kind !== "message") {
      this.dispatchEnvelope(envelope, event);
      return;
    }

    // Plain message: either a message envelope (unwrap text) or a raw body
    // (treat as message text — backward compat with non-enveloped publishers
    // and external HTTP injectors that don't know about chat envelopes).
    const messageEvent: EventNew = envelope
      ? { ...event, body: envelope.text }
      : event;

    const listen = this.listenOnly.has(event.stream);
    this.runMiddleware({ type: "event", data: messageEvent }, () => {
      this.notifier.notify(`event:${event.stream}`);
      // Bump latestSeq for both full-state and listen-only (unread counts)
      this.cursors.bumpLatestSeq(event.stream, event.seq);
      if (listen) return;

      const inserted = this.messages.appendEvent(messageEvent);
      if (!inserted) return;

      const scroll = this.scrollStates.get(event.stream);
      if (scroll) {
        if (scroll.atLiveEdge && this.livenessState !== "hidden") {
          this.scheduleAutoMarkRead(event.stream);
        } else {
          scroll.incrementPending();
        }
      }
    });
  }

  /** Dispatch a non-message chat envelope to the appropriate handler. */
  private dispatchEnvelope(envelope: ChatEnvelope, event: EventNew): void {
    switch (envelope.kind) {
      case "reaction":
        this.handleEnvelopeReaction(envelope, event);
        break;
      case "edit":
        this.handleEnvelopeEdit(envelope, event);
        break;
      case "delete":
        this.handleEnvelopeDelete(envelope, event);
        break;
      case "cursor":
        this.handleEnvelopeCursor(envelope, event);
        break;
    }
  }

  /** Apply an enveloped reaction. Synthesizes a `ReactionChanged` so existing
   *  middleware that subscribes to `"reaction.changed"` keeps working. */
  private handleEnvelopeReaction(env: ReactionEnvelope, event: EventNew): void {
    const synthesized: ReactionChanged = {
      stream: event.stream,
      event_id: env.targetId,
      emoji: env.emoji,
      user_id: event.sender,
      action: env.op,
    };
    const listen = this.listenOnly.has(event.stream);
    this.runMiddleware({ type: "reaction.changed", data: synthesized }, () => {
      this.notifier.notify(`event:${event.stream}`);
      if (!listen) this.messages.applyReaction(synthesized);
    });
  }

  /** Apply an enveloped edit. The envelope's `targetId` is the original
   *  event being edited; the enveloped event's own `seq`/`sent_at` carry
   *  the edit's ordering and timestamp. */
  private handleEnvelopeEdit(env: EditEnvelope, event: EventNew): void {
    const synthesized: EventEdited = {
      stream: event.stream,
      id: env.targetId,
      seq: event.seq,
      body: env.text,
      edited_at: event.sent_at,
    };
    const listen = this.listenOnly.has(event.stream);
    this.runMiddleware({ type: "event.edited", data: synthesized }, () => {
      this.notifier.notify(`event:${event.stream}`);
      if (!listen) this.messages.applyEdit(synthesized);
    });
  }

  private handleEnvelopeDelete(env: DeleteEnvelope, event: EventNew): void {
    const synthesized: EventDeleted = {
      stream: event.stream,
      id: env.targetId,
      seq: event.seq,
    };
    const listen = this.listenOnly.has(event.stream);
    this.runMiddleware({ type: "event.deleted", data: synthesized }, () => {
      this.notifier.notify(`event:${event.stream}`);
      if (!listen) this.messages.applyDelete(synthesized);
    });
  }

  /** Apply an enveloped cursor advance. Mirrors `handleCursor` semantics:
   *  ignore own cursor, ignore listen-only streams, MAX-only advancement,
   *  and flip my own messages to "read" when a remote user passes them. */
  private handleEnvelopeCursor(env: CursorEnvelope, event: EventNew): void {
    const synthesized: CursorMoved = {
      stream: event.stream,
      user_id: event.sender,
      seq: env.seq,
    };
    this.runMiddleware({ type: "cursor", data: synthesized }, () => {
      if (event.sender === this.userId) return;
      if (this.listenOnly.has(event.stream)) return;
      const advanced = this.cursors.updateRemoteCursor(event.stream, event.sender, env.seq);
      if (!advanced) return;
      this.messages.markRead(event.stream, env.seq, this.userId);
    });
  }

  private handleEdited(edit: EventEdited): void {
    const listen = this.listenOnly.has(edit.stream);
    this.runMiddleware({ type: "event.edited", data: edit }, () => {
      this.notifier.notify(`event:${edit.stream}`);
      if (!listen) this.messages.applyEdit(edit);
    });
  }

  private handleDeleted(del: EventDeleted): void {
    const listen = this.listenOnly.has(del.stream);
    this.runMiddleware({ type: "event.deleted", data: del }, () => {
      this.notifier.notify(`event:${del.stream}`);
      if (!listen) this.messages.applyDelete(del);
    });
  }

  private handleReaction(reaction: ReactionChanged): void {
    const listen = this.listenOnly.has(reaction.stream);
    this.runMiddleware({ type: "reaction.changed", data: reaction }, () => {
      this.notifier.notify(`event:${reaction.stream}`);
      if (!listen) this.messages.applyReaction(reaction);
    });
  }

  private handlePresence(event: PresenceChanged): void {
    this.runMiddleware({ type: "presence", data: event }, () => {
      for (const streamId of this.members.streamsForUser(event.user_id)) {
        this.members.updatePresence(streamId, event.user_id, event.presence);
      }
    });
  }

  private handleCursor(event: CursorMoved): void {
    this.runMiddleware({ type: "cursor", data: event }, () => {
      if (event.user_id === this.userId) return;
      if (this.listenOnly.has(event.stream)) return;
      const advanced = this.cursors.updateRemoteCursor(event.stream, event.user_id, event.seq);
      if (!advanced) return;
      this.messages.markRead(event.stream, event.seq, this.userId);
    });
  }

  private handleTyping(event: TypingEvent): void {
    const listen = this.listenOnly.has(event.stream);
    this.runMiddleware({ type: "typing", data: event }, () => {
      if (listen) {
        this.notifier.notify(`typing:${event.stream}`);
        return;
      }
      if (event.user_id === this.userId) return; // don't show own typing
      this.typing.setTyping(event.stream, event.user_id, event.active);
    });
  }

  private handleMemberJoined(event: MemberEvent): void {
    const listen = this.listenOnly.has(event.stream);
    this.runMiddleware({ type: "member.joined", data: event }, () => {
      if (listen) {
        this.notifier.notify(`members:${event.stream}`);
        return;
      }
      this.members.addMember(event.stream, event.user_id, event.role);
    });
  }

  private handleMemberLeft(event: MemberEvent): void {
    const listen = this.listenOnly.has(event.stream);
    this.runMiddleware({ type: "member.left", data: event }, () => {
      if (listen) {
        this.notifier.notify(`members:${event.stream}`);
        return;
      }
      this.members.removeMember(event.stream, event.user_id);
    });
  }

  private handleEphemeral(event: EventReceived): void {
    this.runMiddleware({ type: "ephemeral", data: event }, () => {
      let byType = this.lastEphemeral.get(event.stream);
      if (!byType) {
        byType = new Map();
        this.lastEphemeral.set(event.stream, byType);
      }
      byType.set(event.event, event);
      this.notifier.notify(`ephemeral:${event.stream}`);
    });
  }

  private handleDelivered(event: EventDelivered): void {
    this.runMiddleware({ type: "event.delivered", data: event }, () => {
      if (event.user_id === this.userId) return; // own delivery ack, skip
      if (this.listenOnly.has(event.stream)) return; // no stores for listen-only
      this.messages.markDelivered(event.stream, event.seq, this.userId);
    });
  }

  private handleConnected(): void {
    // On reconnect, the SDK re-subscribes automatically.
    // We may need to re-init members/cursors from fresh subscribed payloads.
    // For now, the existing state is kept and will be updated by incoming events.
  }

  private handleDisconnected(): void {
    // Clear ephemeral state — typing indicators are stale after disconnect
    this.typing.clearAll();
    this.typing.startExpiry(); // restart when we re-attach
  }

  private createPendingMessage(localId: string): PendingMessage {
    const core = this;
    return {
      localId,
      get status() {
        return core.messages.getMessageByLocalId(localId)?.status ?? "failed";
      },
      async retry() {
        await core.retrySend(localId);
      },
      cancel() {
        core.cancelSend(localId);
      },
    };
  }

  private scheduleAutoMarkRead(streamId: string): void {
    this.cancelAutoMarkRead(streamId);
    if (this.scrollIdleMs <= 0) {
      this.autoMarkRead(streamId);
      return;
    }
    const timer = setTimeout(() => {
      this.readTimers.delete(streamId);
      this.autoMarkRead(streamId);
    }, this.scrollIdleMs);
    this.readTimers.set(streamId, timer);
  }

  private cancelAutoMarkRead(streamId: string): void {
    const timer = this.readTimers.get(streamId);
    if (timer !== undefined) {
      clearTimeout(timer);
      this.readTimers.delete(streamId);
    }
  }

  private autoMarkRead(streamId: string): void {
    const msgs = this.messages.getMessages(streamId);
    if (msgs.length === 0) return;

    // Find the latest real (positive) seq from messages, or fall back to
    // the stream's latestSeq from the cursor store. DB-seeded messages use
    // negative synthetic seqs which aren't valid Herald cursors, but the
    // cursor store's latestSeq (from SubscribedPayload) is always valid.
    let latestSeq = 0;
    for (let i = msgs.length - 1; i >= 0; i--) {
      if (msgs[i].seq > 0) {
        latestSeq = msgs[i].seq;
        break;
      }
    }
    if (latestSeq === 0) {
      latestSeq = this.cursors.getLatestSeq(streamId);
    }

    if (latestSeq > 0) {
      const current = this.cursors.getMyCursor(streamId);
      if (latestSeq > current) {
        this.cursors.updateMyCursor(streamId, latestSeq);
        if (this.writer?.updateCursor) {
          // Fire-and-forget: cursor updates are best-effort and we don't
          // block the UI on them. App writers that need to surface errors
          // should handle them internally.
          void this.writer.updateCursor(streamId, latestSeq);
        } else {
          this.chat.updateCursor(streamId, latestSeq);
        }
      }
    }
  }

  private runMiddleware(event: ChatEvent, apply: () => void): void {
    if (this.middleware.length === 0) {
      apply();
      return;
    }
    let idx = 0;
    const next = (): void => {
      if (idx < this.middleware.length) {
        const mw = this.middleware[idx++];
        mw(event, next);
      } else {
        apply();
      }
    };
    next();
  }

  private syncPresence(state: LivenessState): void {
    // Don't auto-sync if user has a manual override active
    if (this.manualPresenceOverride) return;
    if (!this.presenceClient) return;

    switch (state) {
      case "active":
        this.presenceClient.setPresence("online");
        break;
      case "idle":
      case "hidden":
        this.presenceClient.setPresence("away");
        break;
    }
  }

  // ── Presence manual override API ──────────────────────────────────

  /**
   * Set a manual presence override. Suspends automatic liveness-driven
   * presence until {@link clearPresenceOverride} is called.
   *
   * @param status - The presence status to set.
   * @param until - Optional ISO 8601 datetime for when the override expires.
   */
  setPresence(status: PresenceStatus, until?: string): void {
    if (status === "online") {
      this.clearPresenceOverride();
      return;
    }
    this.manualPresenceOverride = status;
    this.presenceClient?.setPresence(status, until);
    this.notifier.notify("presence");
  }

  /**
   * Clear any manual presence override. Reverts to liveness-driven presence
   * and re-enables automatic sync.
   */
  clearPresenceOverride(): void {
    this.manualPresenceOverride = null;
    this.presenceClient?.clearOverride();
    this.notifier.notify("presence");
    // Re-sync from current liveness state
    this.syncPresence(this.livenessState);
  }

  /**
   * Get the current effective presence: manual override if set, otherwise
   * liveness-derived.
   */
  getPresence(): PresenceStatus {
    if (this.manualPresenceOverride) return this.manualPresenceOverride;
    return this.livenessState === "active" ? "online" : "away";
  }
}

const DEFAULT_SCROLL: ScrollStateSnapshot = {
  atLiveEdge: true,
  pendingCount: 0,
  isLoadingMore: false,
};
