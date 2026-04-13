import type { EventNew, EventAck } from "herald-sdk";
import type { Message, EventEdited, EventDeleted, ReactionChanged } from "../types.js";
import { Notifier } from "../notifier.js";

export class MessageStore {
  private streams = new Map<string, Message[]>();
  private byId = new Map<string, Message>();
  private byLocalId = new Map<string, Message>();
  private oldestSeq = new Map<string, number>();
  private _hasMore = new Map<string, boolean>();
  private versions = new Map<string, number>();

  constructor(private notifier: Notifier) {}

  // ── Mutations ──────────────────────────────────────────────────────

  appendEvent(event: EventNew): boolean {
    if (this.byId.has(event.id)) return false;

    const msg = eventToMessage(event);
    this.byId.set(msg.id, msg);

    const list = this.getOrCreateList(event.stream);
    // Binary search insert by seq (ascending), treating seq 0 (optimistic) as +Infinity
    let lo = 0;
    let hi = list.length;
    while (lo < hi) {
      const mid = (lo + hi) >>> 1;
      const midSeq = list[mid].seq === 0 ? Infinity : list[mid].seq;
      if (midSeq < msg.seq) lo = mid + 1;
      else hi = mid;
    }
    list.splice(lo, 0, msg);
    this.replaceList(event.stream, list);

    // Update oldest seq tracking
    const oldest = this.oldestSeq.get(event.stream);
    if (oldest === undefined || msg.seq < oldest) {
      this.oldestSeq.set(event.stream, msg.seq);
    }

    return true;
  }

  prependBatch(streamId: string, events: EventNew[], hasMore: boolean): void {
    const list = this.getOrCreateList(streamId);
    let inserted = false;

    for (const event of events) {
      if (this.byId.has(event.id)) continue;
      const msg = eventToMessage(event);
      this.byId.set(msg.id, msg);

      // Binary search insert (same algorithm as appendEvent)
      const seq = msg.seq === 0 ? Infinity : msg.seq;
      let lo = 0;
      let hi = list.length;
      while (lo < hi) {
        const mid = (lo + hi) >>> 1;
        const midSeq = list[mid].seq === 0 ? Infinity : list[mid].seq;
        if (midSeq < seq) lo = mid + 1;
        else hi = mid;
      }
      list.splice(lo, 0, msg);
      inserted = true;
    }

    if (inserted) {
      this.replaceList(streamId, list);
    }

    this._hasMore.set(streamId, hasMore);

    if (events.length > 0) {
      const minSeq = Math.min(...events.map((e) => e.seq));
      const oldest = this.oldestSeq.get(streamId);
      if (oldest === undefined || minSeq < oldest) {
        this.oldestSeq.set(streamId, minSeq);
      }
    }
  }

  addOptimistic(
    streamId: string,
    localId: string,
    sender: string,
    body: string,
    meta?: unknown,
    parentId?: string,
  ): void {
    const msg: Message = {
      id: localId,
      localId,
      seq: 0,
      stream: streamId,
      sender,
      body,
      meta,
      parentId,
      sentAt: Date.now(),
      deleted: false,
      status: "sending",
      reactions: new Map(),
    };

    this.byLocalId.set(localId, msg);
    const list = this.getOrCreateList(streamId);
    list.push(msg); // optimistic always at end (seq=0)
    this.replaceList(streamId, list);
  }

  reconcile(
    localId: string,
    ack: EventAck & { body?: string; meta?: unknown },
  ): void {
    const msg = this.byLocalId.get(localId);
    if (!msg) return;

    // If the server event already arrived via event.new, just remove the optimistic
    if (this.byId.has(ack.id) && this.byId.get(ack.id) !== msg) {
      this.byLocalId.delete(localId);
      const list = this.streams.get(msg.stream);
      if (list) {
        const idx = list.indexOf(msg);
        if (idx !== -1) {
          list.splice(idx, 1);
          this.replaceList(msg.stream, list);
        }
      }
      return;
    }

    // Reconcile: update the optimistic message with server data
    msg.id = ack.id;
    msg.seq = ack.seq;
    msg.sentAt = ack.sent_at;
    msg.status = "sent";
    // If the app-side writer returned canonical body/meta (sanitized,
    // attachments expanded, mentions resolved), replace the optimistic
    // values so the UI shows what was actually persisted.
    if (ack.body !== undefined) msg.body = ack.body;
    if (ack.meta !== undefined) msg.meta = ack.meta;

    this.byId.set(ack.id, msg);
    this.byLocalId.delete(localId);

    // Re-sort into correct seq position
    const list = this.streams.get(msg.stream);
    if (list) {
      list.sort((a, b) => {
        if (a.seq === 0 && b.seq === 0) return 0;
        if (a.seq === 0) return 1;
        if (b.seq === 0) return -1;
        return a.seq - b.seq;
      });
      this.replaceList(msg.stream, list);
    }
  }

  failOptimistic(localId: string): void {
    const msg = this.byLocalId.get(localId);
    if (!msg) return;
    msg.status = "failed";
    this.bumpVersion(msg.stream);
  }

  removeOptimistic(localId: string): void {
    const msg = this.byLocalId.get(localId);
    if (!msg) return;
    this.byLocalId.delete(localId);
    const list = this.streams.get(msg.stream);
    if (list) {
      const idx = list.indexOf(msg);
      if (idx !== -1) {
        list.splice(idx, 1);
        this.replaceList(msg.stream, list);
      }
    }
  }

  applyEdit(edit: EventEdited): void {
    const msg = this.byId.get(edit.id);
    if (!msg || msg.deleted) return;
    msg.body = edit.body;
    msg.editedAt = edit.edited_at;
    this.bumpVersion(edit.stream);
  }

  /** Apply an edit speculatively before the server confirms. Returns the
   *  previous body+editedAt so a caller can revert on publish failure, or
   *  `null` when the target message isn't in the store (no-op). */
  applyEditOptimistic(
    streamId: string,
    eventId: string,
    newBody: string,
  ): { previousBody: string; previousEditedAt?: number } | null {
    const msg = this.byId.get(eventId);
    if (!msg || msg.deleted) return null;
    const previousBody = msg.body;
    const previousEditedAt = msg.editedAt;
    msg.body = newBody;
    msg.editedAt = Date.now();
    this.bumpVersion(streamId);
    return { previousBody, previousEditedAt };
  }

  /** Restore a message's body+editedAt to a previously-captured snapshot.
   *  Used to roll back a failed optimistic edit. */
  revertEdit(streamId: string, eventId: string, body: string, editedAt?: number): void {
    const msg = this.byId.get(eventId);
    if (!msg) return;
    msg.body = body;
    msg.editedAt = editedAt;
    this.bumpVersion(streamId);
  }

  applyDelete(del: EventDeleted): void {
    const msg = this.byId.get(del.id);
    if (!msg) return;
    msg.deleted = true;
    msg.body = "";
    this.bumpVersion(del.stream);
  }

  /** Apply a delete speculatively. Returns the previous body+deleted state
   *  so a caller can revert on publish failure, or `null` when the target
   *  isn't in the store. */
  applyDeleteOptimistic(
    streamId: string,
    eventId: string,
  ): { previousBody: string; previousDeleted: boolean } | null {
    const msg = this.byId.get(eventId);
    if (!msg) return null;
    const previousBody = msg.body;
    const previousDeleted = msg.deleted;
    msg.deleted = true;
    msg.body = "";
    this.bumpVersion(streamId);
    return { previousBody, previousDeleted };
  }

  /** Restore a message after a failed optimistic delete. */
  revertDelete(streamId: string, eventId: string, body: string, deleted: boolean): void {
    const msg = this.byId.get(eventId);
    if (!msg) return;
    msg.body = body;
    msg.deleted = deleted;
    this.bumpVersion(streamId);
  }

  applyReaction(reaction: ReactionChanged): void {
    const msg = this.byId.get(reaction.event_id);
    if (!msg) return;

    if (reaction.action === "add") {
      let users = msg.reactions.get(reaction.emoji);
      if (!users) {
        users = new Set();
        msg.reactions.set(reaction.emoji, users);
      }
      users.add(reaction.user_id);
    } else {
      const users = msg.reactions.get(reaction.emoji);
      if (users) {
        users.delete(reaction.user_id);
        if (users.size === 0) {
          msg.reactions.delete(reaction.emoji);
        }
      }
    }

    this.bumpVersion(reaction.stream);
  }

  updateMeta(streamId: string, messageId: string, meta: unknown): void {
    const msg = this.byId.get(messageId);
    if (!msg) return;
    msg.meta = meta;
    this.bumpVersion(streamId);
  }

  markDelivered(streamId: string, upToSeq: number, sender: string): boolean {
    const list = this.streams.get(streamId);
    if (!list) return false;
    let changed = false;
    for (const msg of list) {
      if (msg.seq === 0) continue; // optimistic
      if (msg.seq > upToSeq) break; // sorted ascending
      if (msg.sender === sender && msg.status === "sent") {
        msg.status = "delivered";
        changed = true;
      }
    }
    if (changed) this.bumpVersion(streamId);
    return changed;
  }

  markRead(streamId: string, upToSeq: number, sender: string): boolean {
    const list = this.streams.get(streamId);
    if (!list) return false;
    let changed = false;
    for (const msg of list) {
      if (msg.seq === 0) continue; // optimistic
      if (msg.seq > upToSeq) break; // sorted ascending, no need to continue
      if (msg.sender === sender && (msg.status === "sent" || msg.status === "delivered")) {
        msg.status = "read";
        changed = true;
      }
    }
    if (changed) this.bumpVersion(streamId);
    return changed;
  }

  // ── Reads ──────────────────────────────────────────────────────────

  getMessages(streamId: string): Message[] {
    return this.streams.get(streamId) ?? EMPTY;
  }

  getVersion(streamId: string): number {
    return this.versions.get(streamId) ?? 0;
  }

  getOldestSeq(streamId: string): number | undefined {
    return this.oldestSeq.get(streamId);
  }

  getMessageByLocalId(localId: string): Message | undefined {
    const pending = this.byLocalId.get(localId);
    if (pending) return pending;
    // After reconciliation, the message is in byId keyed by server ID but still has localId set
    for (const msg of this.byId.values()) {
      if (msg.localId === localId) return msg;
    }
    return undefined;
  }

  hasMoreHistory(streamId: string): boolean {
    return this._hasMore.get(streamId) ?? true;
  }

  clear(streamId: string): void {
    const list = this.streams.get(streamId);
    if (list) {
      for (const msg of list) {
        this.byId.delete(msg.id);
        if (msg.localId) this.byLocalId.delete(msg.localId);
      }
    }
    this.streams.delete(streamId);
    this.oldestSeq.delete(streamId);
    this._hasMore.delete(streamId);
    this.versions.delete(streamId);
  }

  clearAll(): void {
    this.streams.clear();
    this.byId.clear();
    this.byLocalId.clear();
    this.oldestSeq.clear();
    this._hasMore.clear();
    this.versions.clear();
  }

  // ── Internals ──────────────────────────────────────────────────────

  private getOrCreateList(streamId: string): Message[] {
    let list = this.streams.get(streamId);
    if (!list) {
      list = [];
      this.streams.set(streamId, list);
    }
    return list;
  }

  private replaceList(streamId: string, list: Message[]): void {
    // Set the new list directly — bumpVersion creates the snapshot copy
    this.streams.set(streamId, list);
    this.bumpVersion(streamId);
  }

  private bumpVersion(streamId: string): void {
    this.versions.set(streamId, (this.versions.get(streamId) ?? 0) + 1);
    // New array reference so useSyncExternalStore detects the change.
    // In-place mutations (reactions, edits, deletes) don't go through
    // replaceList, so the snapshot must be refreshed here.
    const list = this.streams.get(streamId);
    if (list) this.streams.set(streamId, [...list]);
    this.notifier.notify(`messages:${streamId}`);
  }
}

const EMPTY: Message[] = [];

function eventToMessage(event: EventNew): Message {
  return {
    id: event.id,
    seq: event.seq,
    stream: event.stream,
    sender: event.sender,
    body: event.body,
    meta: event.meta,
    parentId: event.parent_id,
    sentAt: event.sent_at,
    editedAt: event.edited_at,
    deleted: false,
    status: "sent",
    reactions: new Map(),
  };
}
