import { Notifier } from "../notifier.js";

export class CursorStore {
  private myCursor = new Map<string, number>();
  private latestSeq = new Map<string, number>();
  private remoteCursors = new Map<string, Map<string, number>>();
  private versions = new Map<string, number>();

  constructor(private notifier: Notifier) {}

  initStream(streamId: string, cursor: number, latestSeq: number): void {
    this.myCursor.set(streamId, cursor);
    this.latestSeq.set(streamId, latestSeq);
    this.bumpVersion(streamId);
  }

  updateMyCursor(streamId: string, seq: number): void {
    if (!this.myCursor.has(streamId)) return; // not initialized
    const current = this.myCursor.get(streamId)!;
    if (seq <= current) return; // MAX semantics
    this.myCursor.set(streamId, seq);
    this.bumpVersion(streamId);
  }

  bumpLatestSeq(streamId: string, seq: number): void {
    if (!this.latestSeq.has(streamId)) return; // not initialized
    const current = this.latestSeq.get(streamId)!;
    if (seq <= current) return;
    this.latestSeq.set(streamId, seq);
    this.bumpVersion(streamId);
  }

  getMyCursor(streamId: string): number {
    return this.myCursor.get(streamId) ?? 0;
  }

  getLatestSeq(streamId: string): number {
    return this.latestSeq.get(streamId) ?? 0;
  }

  getUnreadCount(streamId: string): number {
    const latest = this.latestSeq.get(streamId) ?? 0;
    const cursor = this.myCursor.get(streamId) ?? 0;
    return Math.max(0, latest - cursor);
  }

  getTotalUnreadCount(): number {
    let total = 0;
    for (const streamId of this.latestSeq.keys()) {
      total += this.getUnreadCount(streamId);
    }
    return total;
  }

  updateRemoteCursor(streamId: string, userId: string, seq: number): boolean {
    let userMap = this.remoteCursors.get(streamId);
    if (!userMap) {
      userMap = new Map();
      this.remoteCursors.set(streamId, userMap);
    }
    const current = userMap.get(userId) ?? 0;
    if (seq <= current) return false; // MAX semantics
    userMap.set(userId, seq);
    return true;
  }

  getRemoteCursors(streamId: string): Map<string, number> {
    return this.remoteCursors.get(streamId) ?? EMPTY_CURSORS;
  }

  getVersion(streamId: string): number {
    return this.versions.get(streamId) ?? 0;
  }

  clear(streamId: string): void {
    this.myCursor.delete(streamId);
    this.latestSeq.delete(streamId);
    this.remoteCursors.delete(streamId);
    this.versions.delete(streamId);
  }

  clearAll(): void {
    this.myCursor.clear();
    this.latestSeq.clear();
    this.remoteCursors.clear();
    this.versions.clear();
  }

  private bumpVersion(streamId: string): void {
    this.versions.set(streamId, (this.versions.get(streamId) ?? 0) + 1);
    this.notifier.notifyMany([`unread:${streamId}`, "unread:total"]);
  }
}

const EMPTY_CURSORS: Map<string, number> = new Map();
