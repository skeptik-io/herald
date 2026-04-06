import type { EventNew, EventEdited, EventDeleted, EventAck, ReactionChanged } from "herald-sdk";
import type { Message } from "../types.js";
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
      list.push(msg);
      inserted = true;
    }

    if (inserted) {
      list.sort((a, b) => {
        if (a.seq === 0 && b.seq === 0) return 0;
        if (a.seq === 0) return 1; // optimistic at end
        if (b.seq === 0) return -1;
        return a.seq - b.seq;
      });
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

  reconcile(localId: string, ack: EventAck): void {
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
    if (!msg) return;
    msg.body = edit.body;
    msg.editedAt = edit.edited_at;
    this.bumpVersion(edit.stream);
  }

  applyDelete(del: EventDeleted): void {
    const msg = this.byId.get(del.id);
    if (!msg) return;
    msg.deleted = true;
    msg.body = "";
    this.bumpVersion(del.stream);
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
    // Create a new array reference for useSyncExternalStore stability
    this.streams.set(streamId, [...list]);
    this.bumpVersion(streamId);
  }

  private bumpVersion(streamId: string): void {
    this.versions.set(streamId, (this.versions.get(streamId) ?? 0) + 1);
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
