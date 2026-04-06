import { ChatCore } from "../src/chat-core.js";
import type { ChatCoreOptions, Message } from "../src/types.js";
import type { HeraldClient, SubscribedPayload, EventAck, EventsBatch, HeraldEvent, HeraldEventMap } from "herald-sdk";
import type { HeraldChatClient } from "herald-chat-sdk";

let passed = 0;
let failed = 0;

function assert(condition: boolean, msg: string): void {
  if (!condition) { failed++; console.error(`  FAIL: ${msg}`); }
}

async function test(name: string, fn: () => Promise<void> | void): Promise<void> {
  const before = failed;
  try {
    await fn();
    if (failed === before) { passed++; console.log(`  ok - ${name}`); }
    else { console.log(`  FAIL - ${name}`); }
  } catch (e) {
    failed++;
    console.error(`  FAIL - ${name}: ${e}`);
  }
}

// ── Mock client ────────────────────────────────────────────────────

type Handler<T> = (data: T) => void;

interface MockClientOptions {
  /** If set, publish() will reject with this error. */
  publishError?: Error;
  /** If set, subscribe returns this for each stream. */
  subscribePayload?: (stream: string) => SubscribedPayload;
  /** If set, fetch returns this. */
  fetchResult?: (stream: string) => EventsBatch;
  /** If set, editEvent rejects. */
  editError?: Error;
  /** If set, deleteEvent rejects. */
  deleteError?: Error;
}

class MockClient {
  private handlers = new Map<string, Set<Handler<unknown>>>();
  private nextSeq = 1;
  private opts: MockClientOptions;

  publishCalls: Array<{ stream: string; body: string; opts?: unknown }> = [];
  subscribeCalls: string[][] = [];
  unsubscribeCalls: string[][] = [];

  constructor(opts: MockClientOptions = {}) { this.opts = opts; }

  on<E extends HeraldEvent>(event: E, handler: Handler<HeraldEventMap[E]>): void {
    if (!this.handlers.has(event)) this.handlers.set(event, new Set());
    this.handlers.get(event)!.add(handler as Handler<unknown>);
  }

  off<E extends HeraldEvent>(event: E, handler: Handler<HeraldEventMap[E]>): void {
    this.handlers.get(event)?.delete(handler as Handler<unknown>);
  }

  async subscribe(streams: string[]): Promise<SubscribedPayload[]> {
    this.subscribeCalls.push(streams);
    return streams.map((s) =>
      this.opts.subscribePayload?.(s) ?? {
        stream: s,
        members: [{ user_id: "me", role: "member", presence: "online" }],
        cursor: 0,
        latest_seq: 0,
      },
    );
  }

  unsubscribe(streams: string[]): void { this.unsubscribeCalls.push(streams); }

  async publish(stream: string, body: string, opts?: unknown): Promise<EventAck> {
    this.publishCalls.push({ stream, body, opts });
    if (this.opts.publishError) throw this.opts.publishError;
    const seq = this.nextSeq++;
    return { id: `srv_${seq}`, seq, sent_at: Date.now() };
  }

  async fetch(stream: string, opts?: unknown): Promise<EventsBatch> {
    if (this.opts.fetchResult) return this.opts.fetchResult(stream);
    return { stream, events: [], has_more: false };
  }

  sendFrame(_frame: Record<string, unknown>): void {}
  requestFrame(_ref: string, _frame: Record<string, unknown>): Promise<unknown> {
    return Promise.resolve({});
  }
  get e2eeManager() { return null; }

  emit<E extends HeraldEvent>(event: E, data: HeraldEventMap[E]): void {
    const set = this.handlers.get(event);
    if (set) for (const h of set) h(data);
  }

  get handlerCount(): number {
    let n = 0;
    for (const s of this.handlers.values()) n += s.size;
    return n;
  }
}

class MockChatClient {
  cursorCalls: Array<{ stream: string; seq: number }> = [];
  presenceCalls: string[] = [];
  typingStartCalls: string[] = [];
  typingStopCalls: string[] = [];
  reactionCalls: Array<{ type: string; stream: string; eventId: string; emoji: string }> = [];
  private opts: MockClientOptions;

  constructor(opts: MockClientOptions = {}) { this.opts = opts; }

  async editEvent(stream: string, id: string, body: string): Promise<EventAck> {
    if (this.opts.editError) throw this.opts.editError;
    return { id, seq: 1, sent_at: Date.now() };
  }

  async deleteEvent(stream: string, id: string): Promise<EventAck> {
    if (this.opts.deleteError) throw this.opts.deleteError;
    return { id, seq: 1, sent_at: Date.now() };
  }

  addReaction(stream: string, eventId: string, emoji: string): void {
    this.reactionCalls.push({ type: "add", stream, eventId, emoji });
  }

  removeReaction(stream: string, eventId: string, emoji: string): void {
    this.reactionCalls.push({ type: "remove", stream, eventId, emoji });
  }

  startTyping(stream: string): void { this.typingStartCalls.push(stream); }
  stopTyping(stream: string): void { this.typingStopCalls.push(stream); }

  setPresence(status: "online" | "away" | "dnd"): void {
    this.presenceCalls.push(status);
  }

  updateCursor(stream: string, seq: number): void {
    this.cursorCalls.push({ stream, seq });
  }
}

function makeCore(clientOpts?: MockClientOptions, coreOpts?: Partial<ChatCoreOptions>): {
  client: MockClient;
  chatClient: MockChatClient;
  core: ChatCore;
} {
  const client = new MockClient(clientOpts);
  const chatClient = new MockChatClient(clientOpts);
  const core = new ChatCore({
    client: client as unknown as HeraldClient,
    chat: chatClient as unknown as HeraldChatClient,
    userId: "me",
    ...coreOpts,
  });
  return { client, chatClient, core };
}

// ── Lifecycle ──────────────────────────────────────────────────────

await test("attach registers event listeners", async () => {
  const { client, chatClient, core } = makeCore();
  assert(client.handlerCount === 0, "no handlers before attach");
  core.attach();
  assert(client.handlerCount > 0, "handlers after attach");
  core.destroy();
});

await test("detach removes all event listeners", async () => {
  const { client, chatClient, core } = makeCore();
  core.attach();
  core.detach();
  assert(client.handlerCount === 0, "no handlers after detach");
});

await test("double attach is idempotent", async () => {
  const { client, chatClient, core } = makeCore();
  core.attach();
  const count = client.handlerCount;
  core.attach();
  assert(client.handlerCount === count, "same handler count");
  core.destroy();
});

await test("double detach is safe", async () => {
  const { core } = makeCore();
  core.attach();
  core.detach();
  core.detach(); // no throw
});

await test("destroy clears all state", async () => {
  const { client, chatClient, core } = makeCore();
  core.attach();
  await core.joinStream("s1");
  client.emit("event", { stream: "s1", id: "e1", seq: 1, sender: "other", body: "hi", sent_at: 1 });
  core.destroy();
  assert(core.getMessages("s1").length === 0, "messages cleared");
  assert(core.getMembers("s1").length === 0, "members cleared");
  assert(core.getUnreadCount("s1") === 0, "unread cleared");
  assert(client.handlerCount === 0, "handlers removed");
});

// ── joinStream / leaveStream ───────────────────────────────────────

await test("joinStream subscribes and initializes members + cursors", async () => {
  const { client, chatClient, core } = makeCore({
    subscribePayload: (s) => ({
      stream: s,
      members: [
        { user_id: "me", role: "admin", presence: "online" },
        { user_id: "other", role: "member", presence: "away" },
      ],
      cursor: 5,
      latest_seq: 10,
    }),
  });
  core.attach();
  await core.joinStream("s1");

  assert(client.subscribeCalls.length === 1, "subscribe called");
  assert(core.getMembers("s1").length === 2, "2 members");
  assert(core.getMembers("s1")[0].role === "admin", "first member admin");
  assert(core.getUnreadCount("s1") === 5, "5 unread");
  core.destroy();
});

await test("leaveStream unsubscribes and clears all state for that stream", async () => {
  const { client, chatClient, core } = makeCore();
  core.attach();
  await core.joinStream("s1");
  await core.joinStream("s2");
  client.emit("event", { stream: "s1", id: "e1", seq: 1, sender: "other", body: "x", sent_at: 1 });

  core.leaveStream("s1");

  assert(client.unsubscribeCalls.length === 1, "unsubscribe called");
  assert(core.getMessages("s1").length === 0, "s1 messages cleared");
  assert(core.getMembers("s1").length === 0, "s1 members cleared");
  assert(core.getMessages("s2").length === 0, "s2 unaffected (empty but present)");
  core.destroy();
});

// ── Incoming events ────────────────────────────────────────────────

await test("event.new appends message to correct stream", async () => {
  const { client, chatClient, core } = makeCore();
  core.attach();
  await core.joinStream("s1");
  await core.joinStream("s2");

  client.emit("event", { stream: "s1", id: "e1", seq: 1, sender: "alice", body: "hi", sent_at: 100 });
  client.emit("event", { stream: "s2", id: "e2", seq: 1, sender: "bob", body: "yo", sent_at: 200 });

  assert(core.getMessages("s1").length === 1, "s1 has 1");
  assert(core.getMessages("s2").length === 1, "s2 has 1");
  assert(core.getMessages("s1")[0].sender === "alice", "s1 sender");
  assert(core.getMessages("s2")[0].sender === "bob", "s2 sender");
  core.destroy();
});

await test("duplicate event.new is ignored", async () => {
  const { client, chatClient, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  client.emit("event", { stream: "s1", id: "e1", seq: 1, sender: "a", body: "hi", sent_at: 1 });
  client.emit("event", { stream: "s1", id: "e1", seq: 1, sender: "a", body: "hi", sent_at: 1 });

  assert(core.getMessages("s1").length === 1, "deduped");
  core.destroy();
});

await test("event.new updates unread and bumps latestSeq", async () => {
  const { client, chatClient, core } = makeCore({
    subscribePayload: (s) => ({
      stream: s, members: [], cursor: 0, latest_seq: 0,
    }),
  });
  core.attach();
  await core.joinStream("s1");
  core.setAtLiveEdge("s1", false);

  client.emit("event", { stream: "s1", id: "e1", seq: 1, sender: "a", body: "x", sent_at: 1 });
  client.emit("event", { stream: "s1", id: "e2", seq: 2, sender: "a", body: "y", sent_at: 2 });

  assert(core.getUnreadCount("s1") === 2, `expected 2, got ${core.getUnreadCount("s1")}`);
  core.destroy();
});

await test("event.edited updates message body", async () => {
  const { client, chatClient, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  client.emit("event", { stream: "s1", id: "e1", seq: 1, sender: "a", body: "orig", sent_at: 1 });
  client.emit("event.edited", { stream: "s1", id: "e1", seq: 1, body: "edited", edited_at: 99 });

  assert(core.getMessages("s1")[0].body === "edited", "body edited");
  assert(core.getMessages("s1")[0].editedAt === 99, "editedAt set");
  core.destroy();
});

await test("event.deleted marks message as deleted", async () => {
  const { client, chatClient, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  client.emit("event", { stream: "s1", id: "e1", seq: 1, sender: "a", body: "content", sent_at: 1 });
  client.emit("event.deleted", { stream: "s1", id: "e1", seq: 1 });

  assert(core.getMessages("s1")[0].deleted === true, "deleted");
  assert(core.getMessages("s1")[0].body === "", "body cleared");
  core.destroy();
});

await test("edit/delete for unknown event id is harmless", async () => {
  const { client, chatClient, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  client.emit("event.edited", { stream: "s1", id: "nope", seq: 1, body: "x", edited_at: 1 });
  client.emit("event.deleted", { stream: "s1", id: "nope", seq: 1 });
  // no throw
  core.destroy();
});

// ── Reactions ──────────────────────────────────────────────────────

await test("reaction.changed adds and removes reactions on messages", async () => {
  const { client, chatClient, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  client.emit("event", { stream: "s1", id: "e1", seq: 1, sender: "a", body: "x", sent_at: 1 });
  client.emit("reaction.changed", { stream: "s1", event_id: "e1", emoji: "🔥", user_id: "bob", action: "add" });
  client.emit("reaction.changed", { stream: "s1", event_id: "e1", emoji: "🔥", user_id: "alice", action: "add" });

  let r = core.getMessages("s1")[0].reactions;
  assert(r.get("🔥")!.size === 2, "2 users");

  client.emit("reaction.changed", { stream: "s1", event_id: "e1", emoji: "🔥", user_id: "bob", action: "remove" });
  r = core.getMessages("s1")[0].reactions;
  assert(r.get("🔥")!.size === 1, "1 user after remove");
  assert(!r.get("🔥")!.has("bob"), "bob removed");
  core.destroy();
});

await test("reaction for unknown event is harmless", async () => {
  const { client, chatClient, core } = makeCore();
  core.attach();
  await core.joinStream("s1");
  client.emit("reaction.changed", { stream: "s1", event_id: "nope", emoji: "x", user_id: "u", action: "add" });
  core.destroy();
});

// ── Typing ─────────────────────────────────────────────────────────

await test("typing events track other users", async () => {
  const { client, chatClient, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  client.emit("typing", { stream: "s1", user_id: "alice", active: true });
  assert(core.getTypingUsers("s1").length === 1, "1 typing");
  assert(core.getTypingUsers("s1")[0] === "alice", "alice typing");

  client.emit("typing", { stream: "s1", user_id: "bob", active: true });
  assert(core.getTypingUsers("s1").length === 2, "2 typing");

  client.emit("typing", { stream: "s1", user_id: "alice", active: false });
  assert(core.getTypingUsers("s1").length === 1, "1 after alice stops");
  assert(core.getTypingUsers("s1")[0] === "bob", "bob still typing");
  core.destroy();
});

await test("own typing events are filtered out", async () => {
  const { client, chatClient, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  client.emit("typing", { stream: "s1", user_id: "me", active: true });
  assert(core.getTypingUsers("s1").length === 0, "own typing filtered");
  core.destroy();
});

await test("typing across different streams are isolated", async () => {
  const { client, chatClient, core } = makeCore();
  core.attach();
  await core.joinStream("s1");
  await core.joinStream("s2");

  client.emit("typing", { stream: "s1", user_id: "alice", active: true });
  client.emit("typing", { stream: "s2", user_id: "bob", active: true });

  assert(core.getTypingUsers("s1").length === 1, "s1 has 1");
  assert(core.getTypingUsers("s2").length === 1, "s2 has 1");
  assert(core.getTypingUsers("s1")[0] === "alice", "s1 is alice");
  assert(core.getTypingUsers("s2")[0] === "bob", "s2 is bob");
  core.destroy();
});

// ── Presence + members ─────────────────────────────────────────────

await test("member.joined and member.left update member list", async () => {
  const { client, chatClient, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  client.emit("member.joined", { stream: "s1", user_id: "alice", role: "member" });
  assert(core.getMembers("s1").length === 2, "2 members"); // me + alice

  client.emit("member.left", { stream: "s1", user_id: "alice", role: "member" });
  assert(core.getMembers("s1").length === 1, "1 member");
  core.destroy();
});

await test("duplicate member.joined is idempotent", async () => {
  const { client, chatClient, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  client.emit("member.joined", { stream: "s1", user_id: "alice", role: "member" });
  client.emit("member.joined", { stream: "s1", user_id: "alice", role: "member" });
  assert(core.getMembers("s1").length === 2, "still 2");
  core.destroy();
});

await test("presence updates propagate to all streams containing that user", async () => {
  const { client, chatClient, core } = makeCore({
    subscribePayload: (s) => ({
      stream: s,
      members: [
        { user_id: "me", role: "member", presence: "online" },
        { user_id: "shared_user", role: "member", presence: "online" },
      ],
      cursor: 0,
      latest_seq: 0,
    }),
  });
  core.attach();
  await core.joinStream("s1");
  await core.joinStream("s2");

  client.emit("presence", { user_id: "shared_user", presence: "away" });

  const s1m = core.getMembers("s1").find((m) => m.userId === "shared_user");
  const s2m = core.getMembers("s2").find((m) => m.userId === "shared_user");
  assert(s1m?.presence === "away", "s1 presence updated");
  assert(s2m?.presence === "away", "s2 presence updated");
  core.destroy();
});

await test("member.left for unknown user is harmless", async () => {
  const { client, chatClient, core } = makeCore();
  core.attach();
  await core.joinStream("s1");
  client.emit("member.left", { stream: "s1", user_id: "nonexistent", role: "member" });
  assert(core.getMembers("s1").length === 1, "unchanged");
  core.destroy();
});

// ── Optimistic send ────────────────────────────────────────────────

await test("send creates optimistic message, reconciles on ack", async () => {
  const { client, chatClient, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  const promise = core.send("s1", "hello", { meta: { x: 1 }, parentId: "p1" });

  // Synchronously: optimistic message visible
  const msgs = core.getMessages("s1");
  assert(msgs.length === 1, "1 optimistic msg");
  assert(msgs[0].status === "sending", "sending");
  assert(msgs[0].body === "hello", "body");
  assert(msgs[0].sender === "me", "sender is userId");

  const serverId = await promise;
  assert(serverId.startsWith("srv_"), "server id");

  const after = core.getMessages("s1");
  assert(after.length === 1, "still 1 after reconcile");
  assert(after[0].status === "sent", "sent");
  assert(after[0].id === serverId, "server id assigned");
  assert(after[0].seq > 0, "seq assigned");

  // Verify publish was called with correct args
  assert(client.publishCalls.length === 1, "1 publish call");
  assert(client.publishCalls[0].stream === "s1", "correct stream");
  assert(client.publishCalls[0].body === "hello", "correct body");
  core.destroy();
});

await test("send failure marks message as failed", async () => {
  const { core } = makeCore({ publishError: new Error("network down") });
  core.attach();
  await core.joinStream("s1");

  let threw = false;
  try {
    await core.send("s1", "will fail");
  } catch {
    threw = true;
  }

  assert(threw, "send should throw on failure");
  assert(core.getMessages("s1").length === 1, "failed msg still visible");
  assert(core.getMessages("s1")[0].status === "failed", "status failed");
  core.destroy();
});

await test("retrySend retries a failed send", async () => {
  // First publish fails, then succeeds
  let callCount = 0;
  const client = new MockClient();
  const chatClient = new MockChatClient();
  const origPublish = client.publish.bind(client);
  client.publish = async (stream: string, body: string, opts?: unknown): Promise<EventAck> => {
    callCount++;
    if (callCount === 1) throw new Error("first fail");
    return origPublish(stream, body, opts);
  };

  const core = new ChatCore({ client: client as unknown as HeraldClient, chat: chatClient as unknown as HeraldChatClient, userId: "me" });
  core.attach();
  await core.joinStream("s1");

  let failedLocalId: string | undefined;
  try {
    await core.send("s1", "retry me");
  } catch {
    failedLocalId = core.getMessages("s1")[0].localId;
  }

  assert(failedLocalId !== undefined, "have failed localId");
  const serverId = await core.retrySend(failedLocalId!);
  assert(serverId.startsWith("srv_"), "retried successfully");
  assert(core.getMessages("s1").length === 1, "1 message");
  assert(core.getMessages("s1")[0].status === "sent", "sent after retry");
  core.destroy();
});

await test("cancelSend removes failed message", async () => {
  const { core } = makeCore({ publishError: new Error("fail") });
  core.attach();
  await core.joinStream("s1");

  try { await core.send("s1", "cancel me"); } catch {}
  const localId = core.getMessages("s1")[0].localId!;

  core.cancelSend(localId);
  assert(core.getMessages("s1").length === 0, "message removed");
  core.destroy();
});

await test("multiple concurrent sends produce correct order after reconcile", async () => {
  const { core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  // Fire 3 sends without awaiting
  const p1 = core.send("s1", "first");
  const p2 = core.send("s1", "second");
  const p3 = core.send("s1", "third");

  // All 3 should be visible as sending
  assert(core.getMessages("s1").length === 3, "3 optimistic");

  await Promise.all([p1, p2, p3]);

  const msgs = core.getMessages("s1");
  assert(msgs.length === 3, "3 after reconcile");
  assert(msgs.every((m) => m.status === "sent"), "all sent");
  // Should be in seq order
  assert(msgs[0].seq < msgs[1].seq && msgs[1].seq < msgs[2].seq, "ascending seq");
  core.destroy();
});

// ── Actions delegate to client ─────────────────────────────────────

await test("edit delegates to client.editEvent", async () => {
  const { client, chatClient, core } = makeCore();
  core.attach();
  await core.edit("s1", "e1", "new body");
  // Just verifies it doesn't throw; client mock accepts it
  core.destroy();
});

await test("edit propagates client error", async () => {
  const { core } = makeCore({ editError: new Error("forbidden") });
  core.attach();
  let threw = false;
  try { await core.edit("s1", "e1", "x"); } catch { threw = true; }
  assert(threw, "should throw");
  core.destroy();
});

await test("deleteEvent delegates to client", async () => {
  const { client, chatClient, core } = makeCore();
  core.attach();
  await core.deleteEvent("s1", "e1");
  core.destroy();
});

await test("deleteEvent propagates client error", async () => {
  const { core } = makeCore({ deleteError: new Error("forbidden") });
  core.attach();
  let threw = false;
  try { await core.deleteEvent("s1", "e1"); } catch { threw = true; }
  assert(threw, "should throw");
  core.destroy();
});

await test("addReaction delegates to client", async () => {
  const { client, chatClient, core } = makeCore();
  core.attach();
  core.addReaction("s1", "e1", "🔥");
  assert(chatClient.reactionCalls.length === 1, "1 call");
  assert(chatClient.reactionCalls[0].type === "add", "add");
  assert(chatClient.reactionCalls[0].emoji === "🔥", "emoji");
  core.destroy();
});

await test("removeReaction delegates to client", async () => {
  const { client, chatClient, core } = makeCore();
  core.attach();
  core.removeReaction("s1", "e1", "🔥");
  assert(chatClient.reactionCalls.length === 1, "1 call");
  assert(chatClient.reactionCalls[0].type === "remove", "remove");
  core.destroy();
});

await test("startTyping and stopTyping delegate to client", async () => {
  const { client, chatClient, core } = makeCore();
  core.attach();
  core.startTyping("s1");
  core.stopTyping("s1");
  assert(chatClient.typingStartCalls.length === 1, "start called");
  assert(chatClient.typingStopCalls.length === 1, "stop called");
  core.destroy();
});

// ── Scroll coordination ────────────────────────────────────────────

await test("at live edge auto-marks read on new events", async () => {
  const { client, chatClient, core } = makeCore();
  core.attach();
  await core.joinStream("s1");
  core.setAtLiveEdge("s1", true);

  client.emit("event", { stream: "s1", id: "e1", seq: 5, sender: "a", body: "x", sent_at: 1 });

  assert(core.getUnreadCount("s1") === 0, "0 unread");
  assert(chatClient.cursorCalls.length === 1, "cursor sent");
  assert(chatClient.cursorCalls[0].seq === 5, "cursor at seq 5");
  core.destroy();
});

await test("not at live edge increments pending count", async () => {
  const { client, chatClient, core } = makeCore();
  core.attach();
  await core.joinStream("s1");
  core.setAtLiveEdge("s1", false);

  client.emit("event", { stream: "s1", id: "e1", seq: 1, sender: "a", body: "x", sent_at: 1 });
  client.emit("event", { stream: "s1", id: "e2", seq: 2, sender: "a", body: "y", sent_at: 2 });

  const scroll = core.getScrollState("s1");
  assert(scroll.pendingCount === 2, `expected 2 pending, got ${scroll.pendingCount}`);
  assert(scroll.atLiveEdge === false, "not at edge");
  core.destroy();
});

await test("returning to live edge resets pending and marks read", async () => {
  const { client, chatClient, core } = makeCore();
  core.attach();
  await core.joinStream("s1");
  core.setAtLiveEdge("s1", false);

  client.emit("event", { stream: "s1", id: "e1", seq: 3, sender: "a", body: "x", sent_at: 1 });

  core.setAtLiveEdge("s1", true);

  assert(core.getScrollState("s1").pendingCount === 0, "pending reset");
  assert(chatClient.cursorCalls.length === 1, "cursor sent on edge return");
  assert(chatClient.cursorCalls[0].seq === 3, "cursor at latest seq");
  core.destroy();
});

await test("setAtLiveEdge for unknown stream is harmless", async () => {
  const { core } = makeCore();
  core.attach();
  core.setAtLiveEdge("nonexistent", true); // no throw
  core.destroy();
});

// ── loadMore ───────────────────────────────────────────────────────

await test("loadMore fetches older messages and prepends", async () => {
  const { client, chatClient, core } = makeCore({
    fetchResult: (stream) => ({
      stream,
      events: [
        { stream, id: "e1", seq: 1, sender: "a", body: "old1", sent_at: 1 },
        { stream, id: "e2", seq: 2, sender: "a", body: "old2", sent_at: 2 },
      ],
      has_more: true,
    }),
  });
  core.attach();
  await core.joinStream("s1");

  // Add a current message first
  client.emit("event", { stream: "s1", id: "e5", seq: 5, sender: "a", body: "current", sent_at: 5 });

  const hasMore = await core.loadMore("s1");
  assert(hasMore === true, "has more");
  assert(core.getMessages("s1").length === 3, "3 messages total");
  assert(core.getMessages("s1")[0].seq === 1, "oldest first");
  core.destroy();
});

await test("loadMore returns false when no more history", async () => {
  const { core } = makeCore({
    fetchResult: (stream) => ({ stream, events: [], has_more: false }),
  });
  core.attach();
  await core.joinStream("s1");

  const hasMore = await core.loadMore("s1");
  assert(hasMore === false, "no more");

  // Second call should also return false (hasMore flag cached)
  const hasMore2 = await core.loadMore("s1");
  assert(hasMore2 === false, "still no more");
  core.destroy();
});

// ── Disconnect behavior ────────────────────────────────────────────

await test("disconnect event clears typing indicators", async () => {
  const { client, chatClient, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  client.emit("typing", { stream: "s1", user_id: "alice", active: true });
  assert(core.getTypingUsers("s1").length === 1, "1 typing before disconnect");

  client.emit("disconnected", undefined as unknown as void);

  assert(core.getTypingUsers("s1").length === 0, "typing cleared on disconnect");
  core.destroy();
});

await test("events after detach are ignored", async () => {
  const { client, chatClient, core } = makeCore();
  core.attach();
  await core.joinStream("s1");
  core.detach();

  // These should not crash or modify state
  client.emit("event", { stream: "s1", id: "e1", seq: 1, sender: "a", body: "x", sent_at: 1 });
  assert(core.getMessages("s1").length === 0, "no messages after detach");
});

// ── Change notifications ───────────────────────────────────────────

await test("subscribe/unsubscribe controls notifications", async () => {
  const { client, chatClient, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  let count = 0;
  const unsub = core.subscribe("messages:s1", () => { count++; });

  client.emit("event", { stream: "s1", id: "e1", seq: 1, sender: "a", body: "x", sent_at: 1 });
  assert(count === 1, "notified");

  unsub();
  client.emit("event", { stream: "s1", id: "e2", seq: 2, sender: "a", body: "y", sent_at: 2 });
  assert(count === 1, "not notified after unsub");
  core.destroy();
});

await test("notifications fire for correct slices", async () => {
  const { client, chatClient, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  let msgCount = 0, unreadCount = 0, typingCount = 0, memberCount = 0;
  core.subscribe("messages:s1", () => { msgCount++; });
  core.subscribe("unread:s1", () => { unreadCount++; });
  core.subscribe("typing:s1", () => { typingCount++; });
  core.subscribe("members:s1", () => { memberCount++; });

  client.emit("event", { stream: "s1", id: "e1", seq: 1, sender: "a", body: "x", sent_at: 1 });
  assert(msgCount === 1, "message notified");
  assert(unreadCount >= 1, "unread notified");

  client.emit("typing", { stream: "s1", user_id: "alice", active: true });
  assert(typingCount === 1, "typing notified");

  client.emit("member.joined", { stream: "s1", user_id: "bob", role: "member" });
  assert(memberCount === 1, "member notified");

  core.destroy();
});

// ── Rapid-fire events ──────────────────────────────────────────────

await test("100 rapid events are all captured in order", async () => {
  const { client, chatClient, core } = makeCore();
  core.attach();
  await core.joinStream("s1");
  core.setAtLiveEdge("s1", false);

  for (let i = 1; i <= 100; i++) {
    client.emit("event", { stream: "s1", id: `e${i}`, seq: i, sender: "bot", body: `msg ${i}`, sent_at: i });
  }

  const msgs = core.getMessages("s1");
  assert(msgs.length === 100, `expected 100, got ${msgs.length}`);
  for (let i = 0; i < 100; i++) {
    assert(msgs[i].seq === i + 1, `msg ${i} has wrong seq ${msgs[i].seq}`);
  }
  assert(core.getScrollState("s1").pendingCount === 100, "100 pending");
  assert(core.getUnreadCount("s1") === 100, "100 unread");
  core.destroy();
});

// ── getScrollState default ─────────────────────────────────────────

await test("getScrollState for unknown stream returns defaults", async () => {
  const { core } = makeCore();
  core.attach();
  const s = core.getScrollState("nope");
  assert(s.atLiveEdge === true, "default atLiveEdge");
  assert(s.pendingCount === 0, "default pendingCount");
  assert(s.isLoadingMore === false, "default isLoadingMore");
  core.destroy();
});

// ── getLivenessState default ────────────────────────────────────────

await test("getLivenessState returns active when no liveness configured", async () => {
  const { core } = makeCore();
  core.attach();
  assert(core.getLivenessState() === "active", "default active");
  core.destroy();
});

// ── Summary ────────────────────────────────────────────────────────
console.log(`\nchat-core: ${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
