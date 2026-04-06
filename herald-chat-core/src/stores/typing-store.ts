import { Notifier } from "../notifier.js";

const CLIENT_TYPING_TTL_MS = 12_000; // slightly > server's 10s
const EXPIRY_CHECK_INTERVAL_MS = 2_000;

export class TypingStore {
  private streams = new Map<string, Map<string, number>>(); // streamId → (userId → timestamp)
  private snapshots = new Map<string, string[]>();
  private versions = new Map<string, number>();
  private expiryTimer: ReturnType<typeof setInterval> | null = null;

  constructor(private notifier: Notifier) {}

  setTyping(streamId: string, userId: string, active: boolean): void {
    let users = this.streams.get(streamId);

    if (active) {
      if (!users) {
        users = new Map();
        this.streams.set(streamId, users);
      }
      users.set(userId, Date.now());
    } else {
      if (users) {
        users.delete(userId);
        if (users.size === 0) this.streams.delete(streamId);
      }
    }

    this.rebuildSnapshot(streamId);
  }

  getTypingUsers(streamId: string): string[] {
    return this.snapshots.get(streamId) ?? EMPTY;
  }

  getVersion(streamId: string): number {
    return this.versions.get(streamId) ?? 0;
  }

  startExpiry(): void {
    if (this.expiryTimer !== null) return;
    this.expiryTimer = setInterval(() => this.expire(), EXPIRY_CHECK_INTERVAL_MS);
  }

  stopExpiry(): void {
    if (this.expiryTimer !== null) {
      clearInterval(this.expiryTimer);
      this.expiryTimer = null;
    }
  }

  clear(streamId: string): void {
    this.streams.delete(streamId);
    this.snapshots.delete(streamId);
    this.versions.delete(streamId);
  }

  clearAll(): void {
    this.stopExpiry();
    this.streams.clear();
    this.snapshots.clear();
    this.versions.clear();
  }

  private expire(): void {
    const now = Date.now();
    for (const [streamId, users] of this.streams) {
      let changed = false;
      for (const [userId, timestamp] of users) {
        if (now - timestamp > CLIENT_TYPING_TTL_MS) {
          users.delete(userId);
          changed = true;
        }
      }
      if (users.size === 0) this.streams.delete(streamId);
      if (changed) this.rebuildSnapshot(streamId);
    }
  }

  private rebuildSnapshot(streamId: string): void {
    const users = this.streams.get(streamId);
    const prev = this.snapshots.get(streamId) ?? EMPTY;
    const next = users ? Array.from(users.keys()) : EMPTY;

    // Only notify if the list actually changed
    if (prev.length === next.length && prev.every((u, i) => u === next[i])) {
      return;
    }

    if (next.length === 0) {
      this.snapshots.delete(streamId);
    } else {
      this.snapshots.set(streamId, next);
    }
    this.versions.set(streamId, (this.versions.get(streamId) ?? 0) + 1);
    this.notifier.notify(`typing:${streamId}`);
  }
}

const EMPTY: string[] = [];
