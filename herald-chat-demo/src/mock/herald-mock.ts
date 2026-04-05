/**
 * Mock Herald client that simulates Herald WebSocket behavior in-memory.
 *
 * Mirrors the real HeraldClient API surface so the React app demonstrates
 * exactly how Herald would be integrated, but without needing a running server.
 */

import type { Message, User, Stream } from "../data/seed";
import {
  users,
  streams,
  seedMessages,
  seedCursors,
  CURRENT_USER_ID,
} from "../data/seed";

// ── Types matching Herald SDK ──────────────────────────────────────────

export interface EventNew {
  stream: string;
  id: string;
  seq: number;
  sender: string;
  body: string;
  sent_at: number;
  parent_id?: string;
}

export interface PresenceChanged {
  user_id: string;
  presence: string;
}

export interface CursorMoved {
  stream: string;
  user_id: string;
  seq: number;
}

export interface TypingEvent {
  stream: string;
  user_id: string;
  active: boolean;
}

export interface MemberPresence {
  user_id: string;
  role: string;
  presence: string;
}

export interface SubscribedPayload {
  stream: string;
  members: MemberPresence[];
  cursor: number;
  latest_seq: number;
}

type Handler = (data: unknown) => void;

// ── Mock Client ────────────────────────────────────────────────────────

export class MockHeraldClient {
  private handlers = new Map<string, Set<Handler>>();
  private messageStore: Message[];
  private cursorStore: Record<string, Record<string, number>>;
  private presenceStore: Record<string, User["presence"]>;
  private typingTimers = new Map<string, ReturnType<typeof setTimeout>>();
  private nextSeq: number;
  private nextMsgId: number;

  constructor() {
    this.messageStore = [...seedMessages];
    this.cursorStore = JSON.parse(JSON.stringify(seedCursors));
    this.presenceStore = {};
    for (const u of Object.values(users)) {
      this.presenceStore[u.id] = u.presence;
    }
    this.nextSeq = seedMessages.length + 1;
    this.nextMsgId = seedMessages.length + 1;
  }

  // ── Connection (no-op for mock) ─────────────────────────────────────

  async connect(): Promise<void> {
    // Simulate connection delay
    await new Promise((r) => setTimeout(r, 100));
    this.emit("connected", undefined);
    // Start simulated bot activity after connection
    this.startBotSimulation();
  }

  disconnect(): void {
    this.emit("disconnected", undefined);
  }

  // ── Subscribe ───────────────────────────────────────────────────────

  async subscribe(streamIds: string[]): Promise<SubscribedPayload[]> {
    return streamIds.map((streamId) => {
      const stream = streams.find((s) => s.id === streamId);
      const streamMessages = this.messageStore.filter((m) => m.stream === streamId);
      const latestSeq = streamMessages.length > 0
        ? Math.max(...streamMessages.map((m) => m.seq))
        : 0;
      const cursor = this.cursorStore[streamId]?.[CURRENT_USER_ID] ?? 0;

      return {
        stream: streamId,
        members: (stream?.members ?? []).map((uid) => ({
          user_id: uid,
          role: "member",
          presence: this.presenceStore[uid] ?? "offline",
        })),
        cursor,
        latest_seq: latestSeq,
      };
    });
  }

  // ── Publish ─────────────────────────────────────────────────────────

  async publish(
    stream: string,
    body: string,
    options?: { parentId?: string },
  ): Promise<{ id: string; seq: number; sent_at: number }> {
    const seq = this.nextSeq++;
    const id = `msg-${this.nextMsgId++}`;
    const sent_at = Date.now();

    const msg: Message = { id, stream, seq, sender: CURRENT_USER_ID, body, sent_at };
    if (options?.parentId) msg.parent_id = options.parentId;
    this.messageStore.push(msg);

    // Emit the event to local listeners (like the real SDK would)
    const event: EventNew = { stream, id, seq, sender: CURRENT_USER_ID, body, sent_at };
    this.emit("event", event);

    // Auto-advance our cursor
    this.updateCursorInternal(stream, CURRENT_USER_ID, seq);

    return { id, seq, sent_at };
  }

  // ── Fetch history ───────────────────────────────────────────────────

  async fetch(
    stream: string,
    options?: { before?: number; limit?: number },
  ): Promise<{ stream: string; events: EventNew[]; has_more: boolean }> {
    let msgs = this.messageStore.filter((m) => m.stream === stream);
    if (options?.before) {
      msgs = msgs.filter((m) => m.seq < options.before!);
    }
    msgs.sort((a, b) => a.seq - b.seq);
    const limit = options?.limit ?? 50;
    const sliced = msgs.slice(-limit);
    return {
      stream,
      events: sliced.map((m) => ({
        stream: m.stream,
        id: m.id,
        seq: m.seq,
        sender: m.sender,
        body: m.body,
        sent_at: m.sent_at,
      })),
      has_more: msgs.length > limit,
    };
  }

  // ── Presence ────────────────────────────────────────────────────────

  setPresence(status: "online" | "away" | "dnd"): void {
    this.presenceStore[CURRENT_USER_ID] = status;
    this.emit("presence", { user_id: CURRENT_USER_ID, presence: status });
  }

  getPresence(userId: string): User["presence"] {
    return this.presenceStore[userId] ?? "offline";
  }

  // ── Cursors (read receipts) ─────────────────────────────────────────

  updateCursor(stream: string, seq: number): void {
    this.updateCursorInternal(stream, CURRENT_USER_ID, seq);
  }

  private updateCursorInternal(stream: string, userId: string, seq: number): void {
    if (!this.cursorStore[stream]) this.cursorStore[stream] = {};
    const current = this.cursorStore[stream][userId] ?? 0;
    if (seq > current) {
      this.cursorStore[stream][userId] = seq;
      this.emit("cursor", { stream, user_id: userId, seq });
    }
  }

  getCursor(stream: string, userId: string): number {
    return this.cursorStore[stream]?.[userId] ?? 0;
  }

  getCursors(stream: string): Record<string, number> {
    return { ...(this.cursorStore[stream] ?? {}) };
  }

  // ── Typing ──────────────────────────────────────────────────────────

  startTyping(stream: string): void {
    this.emit("typing", { stream, user_id: CURRENT_USER_ID, active: true });
  }

  stopTyping(stream: string): void {
    this.emit("typing", { stream, user_id: CURRENT_USER_ID, active: false });
  }

  // ── Events ──────────────────────────────────────────────────────────

  on(event: string, handler: Handler): void {
    if (!this.handlers.has(event)) {
      this.handlers.set(event, new Set());
    }
    this.handlers.get(event)!.add(handler);
  }

  off(event: string, handler: Handler): void {
    this.handlers.get(event)?.delete(handler);
  }

  private emit(event: string, data: unknown): void {
    const set = this.handlers.get(event);
    if (set) {
      for (const handler of set) {
        try { handler(data); } catch { /* */ }
      }
    }
  }

  // ── Unread count helper ─────────────────────────────────────────────

  getUnreadCount(stream: string): number {
    const cursor = this.cursorStore[stream]?.[CURRENT_USER_ID] ?? 0;
    const msgs = this.messageStore.filter(
      (m) => m.stream === stream && m.seq > cursor && m.sender !== CURRENT_USER_ID,
    );
    return msgs.length;
  }

  // ── Bot simulation ──────────────────────────────────────────────────

  private startBotSimulation(): void {
    const botResponses: { stream: string; sender: string; body: string; delay: number }[] = [
      { stream: "dm-alice-bob", sender: "bob", body: "By the way, I just pushed the cursor sync fix 🎉", delay: 15_000 },
      { stream: "group-eng", sender: "dave", body: "PR is up: herald-server#142 — regression test for the WAL flush bug", delay: 25_000 },
      { stream: "dm-alice-carol", sender: "carol", body: "Did you get a chance to look at the color tokens?", delay: 35_000 },
      { stream: "group-design", sender: "eve", body: "New icon set preview is in the #design-review channel", delay: 45_000 },
      { stream: "group-eng", sender: "bob", body: "Looks good Dave, approving now", delay: 55_000 },
    ];

    for (const resp of botResponses) {
      setTimeout(() => {
        // Show typing first
        this.emit("typing", { stream: resp.stream, user_id: resp.sender, active: true });

        // Then send message after a short delay
        setTimeout(() => {
          this.emit("typing", { stream: resp.stream, user_id: resp.sender, active: false });

          const seq = this.nextSeq++;
          const id = `msg-${this.nextMsgId++}`;
          const sent_at = Date.now();
          const msg: Message = {
            id, stream: resp.stream, seq, sender: resp.sender, body: resp.body, sent_at,
          };
          this.messageStore.push(msg);
          this.emit("event", { stream: resp.stream, id, seq, sender: resp.sender, body: resp.body, sent_at });

          // Bot reads their own message
          this.updateCursorInternal(resp.stream, resp.sender, seq);
        }, 2000 + Math.random() * 1500);
      }, resp.delay);
    }

    // Simulate presence changes
    setTimeout(() => {
      this.presenceStore["carol"] = "online";
      this.emit("presence", { user_id: "carol", presence: "online" });
    }, 20_000);

    setTimeout(() => {
      this.presenceStore["eve"] = "online";
      this.emit("presence", { user_id: "eve", presence: "online" });
    }, 40_000);
  }
}
