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
    scrollIdleMs: 0, // disable debounce by default for test determinism
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

// ── Envelope dispatch (pure-transport chat ops) ────────────────────

await test("envelope: message envelope unwraps text into Message.body", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  const env = JSON.stringify({ kind: "message", text: "hello" });
  client.emit("event", { stream: "s1", id: "e1", seq: 1, sender: "alice", body: env, sent_at: 1 });

  const msgs = core.getMessages("s1");
  assert(msgs.length === 1, "1 message");
  assert(msgs[0].body === "hello", `body should be unwrapped text, got ${msgs[0].body}`);
  core.destroy();
});

await test("envelope: plain (non-JSON) body is treated as message text", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  client.emit("event", { stream: "s1", id: "e1", seq: 1, sender: "alice", body: "raw text", sent_at: 1 });

  assert(core.getMessages("s1").length === 1, "1 message");
  assert(core.getMessages("s1")[0].body === "raw text", "raw body preserved");
  core.destroy();
});

await test("envelope: reaction envelope mutates target message reactions", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  // Seed a message
  client.emit("event", { stream: "s1", id: "e1", seq: 1, sender: "alice", body: "hi", sent_at: 1 });

  // Reaction add via envelope (delivered as a normal event from "bob")
  const env = JSON.stringify({ kind: "reaction", targetId: "e1", op: "add", emoji: "🔥" });
  client.emit("event", { stream: "s1", id: "e2", seq: 2, sender: "bob", body: env, sent_at: 2 });

  const msgs = core.getMessages("s1");
  assert(msgs.length === 1, "reaction did not append a message — still 1");
  const r = msgs[0].reactions;
  assert(r.get("🔥")?.has("bob") === true, "bob's reaction recorded");

  // Reaction remove via envelope
  const envRemove = JSON.stringify({ kind: "reaction", targetId: "e1", op: "remove", emoji: "🔥" });
  client.emit("event", { stream: "s1", id: "e3", seq: 3, sender: "bob", body: envRemove, sent_at: 3 });

  assert(msgs[0].reactions.get("🔥") === undefined, "reaction removed");
  core.destroy();
});

await test("envelope: edit envelope updates target message body and editedAt", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  client.emit("event", { stream: "s1", id: "e1", seq: 1, sender: "alice", body: "orig", sent_at: 1 });

  const env = JSON.stringify({ kind: "edit", targetId: "e1", text: "edited" });
  client.emit("event", { stream: "s1", id: "e2", seq: 2, sender: "alice", body: env, sent_at: 99 });

  const msgs = core.getMessages("s1");
  assert(msgs.length === 1, "edit did not append a message");
  assert(msgs[0].body === "edited", `body should be edited, got ${msgs[0].body}`);
  assert(msgs[0].editedAt === 99, "editedAt set from envelope event sent_at");
  core.destroy();
});

await test("envelope: delete envelope tombstones target message", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  client.emit("event", { stream: "s1", id: "e1", seq: 1, sender: "alice", body: "secret", sent_at: 1 });

  const env = JSON.stringify({ kind: "delete", targetId: "e1" });
  client.emit("event", { stream: "s1", id: "e2", seq: 2, sender: "alice", body: env, sent_at: 2 });

  const msgs = core.getMessages("s1");
  assert(msgs.length === 1, "delete did not append a message");
  assert(msgs[0].deleted === true, "marked deleted");
  assert(msgs[0].body === "", "body cleared");
  core.destroy();
});

await test("envelope: cursor envelope advances remote cursor and flips read", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  // My message
  await core.send("s1", "hello");
  const seq = core.getMessages("s1")[0].seq;

  // Bob's cursor envelope past my message
  const env = JSON.stringify({ kind: "cursor", seq });
  client.emit("event", { stream: "s1", id: "c1", seq: seq + 1, sender: "bob", body: env, sent_at: 100 });

  assert(core.getRemoteCursors("s1").get("bob") === seq, "bob's cursor recorded");
  assert(core.getMessages("s1")[0].status === "read", "my message flipped to read");
  core.destroy();
});

await test("envelope: own cursor envelope is ignored", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  await core.send("s1", "hello");
  const seq = core.getMessages("s1")[0].seq;

  const env = JSON.stringify({ kind: "cursor", seq });
  client.emit("event", { stream: "s1", id: "c1", seq: seq + 1, sender: "me", body: env, sent_at: 100 });

  assert(core.getMessages("s1")[0].status === "sent", "own cursor envelope ignored");
  assert(core.getRemoteCursors("s1").size === 0, "no remote cursor recorded for self");
  core.destroy();
});

await test("envelope: middleware sees synthesized typed events for non-message envelopes", async () => {
  const seen: string[] = [];
  const { client, core } = makeCore({}, {
    middleware: [(event, next) => { seen.push(event.type); next(); }],
  });
  core.attach();
  await core.joinStream("s1");

  client.emit("event", { stream: "s1", id: "e1", seq: 1, sender: "alice", body: "hi", sent_at: 1 });
  const reactionEnv = JSON.stringify({ kind: "reaction", targetId: "e1", op: "add", emoji: "👍" });
  client.emit("event", { stream: "s1", id: "e2", seq: 2, sender: "bob", body: reactionEnv, sent_at: 2 });
  const editEnv = JSON.stringify({ kind: "edit", targetId: "e1", text: "edited" });
  client.emit("event", { stream: "s1", id: "e3", seq: 3, sender: "alice", body: editEnv, sent_at: 3 });
  const deleteEnv = JSON.stringify({ kind: "delete", targetId: "e1" });
  client.emit("event", { stream: "s1", id: "e4", seq: 4, sender: "alice", body: deleteEnv, sent_at: 4 });
  const cursorEnv = JSON.stringify({ kind: "cursor", seq: 4 });
  client.emit("event", { stream: "s1", id: "e5", seq: 5, sender: "bob", body: cursorEnv, sent_at: 5 });

  assert(seen[0] === "event", `expected event, got ${seen[0]}`);
  assert(seen[1] === "reaction.changed", `expected reaction.changed, got ${seen[1]}`);
  assert(seen[2] === "event.edited", `expected event.edited, got ${seen[2]}`);
  assert(seen[3] === "event.deleted", `expected event.deleted, got ${seen[3]}`);
  assert(seen[4] === "cursor", `expected cursor, got ${seen[4]}`);
  core.destroy();
});

await test("envelope: malformed JSON body is treated as plain message text", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  client.emit("event", { stream: "s1", id: "e1", seq: 1, sender: "a", body: "{not json", sent_at: 1 });
  client.emit("event", { stream: "s1", id: "e2", seq: 2, sender: "a", body: '{"kind":"unknown","x":1}', sent_at: 2 });

  // Both fall through to message handling
  const msgs = core.getMessages("s1");
  assert(msgs.length === 2, "both treated as messages");
  core.destroy();
});

await test("envelope: listen-only stream notifies but does not mutate message store", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.listen("inbox");

  let count = 0;
  core.subscribe("event:inbox", () => { count++; });

  // Pre-state cannot have a message because listen() doesn't store messages,
  // so a reaction has no target — but the dispatcher should still notify.
  const env = JSON.stringify({ kind: "reaction", targetId: "e1", op: "add", emoji: "👍" });
  client.emit("event", { stream: "inbox", id: "e2", seq: 2, sender: "bob", body: env, sent_at: 2 });

  assert(count === 1, "event: slice notified for envelope on listen-only stream");
  assert(core.getMessages("inbox").length === 0, "no messages stored");
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

  const pending = await promise;
  assert(pending.localId.startsWith("local:"), "has localId");
  assert(pending.status === "sent", "status is sent after resolve");

  const after = core.getMessages("s1");
  assert(after.length === 1, "still 1 after reconcile");
  assert(after[0].status === "sent", "sent");
  assert(after[0].id.startsWith("srv_"), "server id assigned");
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

await test("addReaction publishes envelope and applies optimistic", async () => {
  const { client, chatClient, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  // Seed a target message
  client.emit("event", { stream: "s1", id: "e1", seq: 1, sender: "alice", body: "hi", sent_at: 1 });

  await core.addReaction("s1", "e1", "🔥");

  // Optimistic mutation visible immediately
  const msgs = core.getMessages("s1");
  assert(msgs[0].reactions.get("🔥")?.has("me") === true, "optimistic reaction applied");

  // Envelope was published, not the legacy chat.addReaction frame
  assert(chatClient.reactionCalls.length === 0, "legacy chat.addReaction NOT called");
  // First publish was the seeded message via emit, not via publish; check the new one
  const reactionPublish = client.publishCalls.find((c) =>
    typeof c.body === "string" && c.body.includes('"reaction"'));
  assert(reactionPublish !== undefined, "reaction envelope published");
  const env = JSON.parse(reactionPublish!.body);
  assert(env.kind === "reaction", "kind");
  assert(env.targetId === "e1", "targetId");
  assert(env.op === "add", "op");
  assert(env.emoji === "🔥", "emoji");
  core.destroy();
});

await test("removeReaction publishes envelope and applies optimistic", async () => {
  const { client, chatClient, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  // Seed message + an existing reaction from "me"
  client.emit("event", { stream: "s1", id: "e1", seq: 1, sender: "alice", body: "hi", sent_at: 1 });
  client.emit("reaction.changed", { stream: "s1", event_id: "e1", emoji: "🔥", user_id: "me", action: "add" });

  await core.removeReaction("s1", "e1", "🔥");

  // Optimistic removal: empty reactions for that emoji means the emoji entry is gone
  assert(core.getMessages("s1")[0].reactions.get("🔥") === undefined, "optimistic remove applied");

  assert(chatClient.reactionCalls.length === 0, "legacy chat.removeReaction NOT called");
  const reactionPublish = client.publishCalls.find((c) =>
    typeof c.body === "string" && c.body.includes('"reaction"'));
  const env = JSON.parse(reactionPublish!.body);
  assert(env.op === "remove", "op");
  core.destroy();
});

await test("addReaction rolls back optimistic on publish failure", async () => {
  const { client, chatClient, core } = makeCore({ publishError: new Error("boom") });
  core.attach();
  await core.joinStream("s1");
  client.emit("event", { stream: "s1", id: "e1", seq: 1, sender: "alice", body: "hi", sent_at: 1 });

  let threw = false;
  try { await core.addReaction("s1", "e1", "🔥"); } catch { threw = true; }
  assert(threw, "addReaction threw");

  // Optimistic was rolled back — no reaction on the message
  assert(core.getMessages("s1")[0].reactions.get("🔥") === undefined, "optimistic rolled back");
  core.destroy();
});

await test("removeReaction rolls back optimistic on publish failure", async () => {
  const { client, chatClient, core } = makeCore({ publishError: new Error("boom") });
  core.attach();
  await core.joinStream("s1");
  client.emit("event", { stream: "s1", id: "e1", seq: 1, sender: "alice", body: "hi", sent_at: 1 });
  client.emit("reaction.changed", { stream: "s1", event_id: "e1", emoji: "🔥", user_id: "me", action: "add" });

  let threw = false;
  try { await core.removeReaction("s1", "e1", "🔥"); } catch { threw = true; }
  assert(threw, "removeReaction threw");

  // Rollback: reaction restored
  assert(core.getMessages("s1")[0].reactions.get("🔥")?.has("me") === true, "reaction restored");
  core.destroy();
});

await test("addReaction echo is idempotent (server fanout = no-op)", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.joinStream("s1");
  client.emit("event", { stream: "s1", id: "e1", seq: 1, sender: "alice", body: "hi", sent_at: 1 });

  await core.addReaction("s1", "e1", "🔥");
  // Simulate the server's envelope echo
  const env = JSON.stringify({ kind: "reaction", targetId: "e1", op: "add", emoji: "🔥" });
  client.emit("event", { stream: "s1", id: "rx1", seq: 2, sender: "me", body: env, sent_at: 2 });

  // Still exactly one user in the set (Set dedupe)
  assert(core.getMessages("s1")[0].reactions.get("🔥")?.size === 1, "no double-count from echo");
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

// ── scrollIdleMs debounce ──────────────────────────────────────────

await test("scrollIdleMs debounce: cursor not sent immediately", async () => {
  const { client, chatClient, core } = makeCore(undefined, { scrollIdleMs: 100 });
  core.attach();
  await core.joinStream("s1");

  // Publish an event while at live edge
  client.emit("event", { id: "e1", seq: 1, stream: "s1", sender: "bob", body: "hi", sent_at: 1, meta: null });

  // Cursor should NOT have been sent yet (debounce pending)
  assert(chatClient.cursorCalls.length === 0, "no cursor sent yet");

  core.destroy();
});

await test("scrollIdleMs debounce: cursor sent after delay", async () => {
  const { client, chatClient, core } = makeCore(undefined, { scrollIdleMs: 50 });
  core.attach();
  await core.joinStream("s1");

  client.emit("event", { id: "e1", seq: 1, stream: "s1", sender: "bob", body: "hi", sent_at: 1, meta: null });
  assert(chatClient.cursorCalls.length === 0, "not sent immediately");

  // Wait for debounce
  await new Promise((r) => setTimeout(r, 80));
  assert(chatClient.cursorCalls.length === 1, "cursor sent after delay");
  assert(chatClient.cursorCalls[0].seq === 1, "correct seq");

  core.destroy();
});

await test("scrollIdleMs debounce: scroll away cancels pending cursor", async () => {
  const { client, chatClient, core } = makeCore(undefined, { scrollIdleMs: 100 });
  core.attach();
  await core.joinStream("s1");

  client.emit("event", { id: "e1", seq: 1, stream: "s1", sender: "bob", body: "hi", sent_at: 1, meta: null });
  assert(chatClient.cursorCalls.length === 0, "not sent yet");

  // Scroll away from live edge — should cancel the pending timer
  core.setAtLiveEdge("s1", false);

  // Wait longer than the debounce
  await new Promise((r) => setTimeout(r, 150));
  assert(chatClient.cursorCalls.length === 0, "cursor never sent after scroll-away");

  core.destroy();
});

await test("scrollIdleMs debounce: rapid events coalesce into one cursor update", async () => {
  const { client, chatClient, core } = makeCore(undefined, { scrollIdleMs: 50 });
  core.attach();
  await core.joinStream("s1");

  // Rapid-fire 5 events
  for (let i = 1; i <= 5; i++) {
    client.emit("event", { id: `e${i}`, seq: i, stream: "s1", sender: "bob", body: `msg ${i}`, sent_at: i, meta: null });
  }

  // Wait for debounce
  await new Promise((r) => setTimeout(r, 80));
  // Should send only one cursor update for the latest seq
  assert(chatClient.cursorCalls.length === 1, `expected 1 cursor call, got ${chatClient.cursorCalls.length}`);
  assert(chatClient.cursorCalls[0].seq === 5, "cursor sent for latest seq");

  core.destroy();
});

await test("scrollIdleMs: 0 fires cursor immediately (no debounce)", async () => {
  const { client, chatClient, core } = makeCore(undefined, { scrollIdleMs: 0 });
  core.attach();
  await core.joinStream("s1");

  client.emit("event", { id: "e1", seq: 1, stream: "s1", sender: "bob", body: "hi", sent_at: 1, meta: null });
  assert(chatClient.cursorCalls.length === 1, "cursor sent immediately");

  core.destroy();
});

// ── PendingMessage interface ───────────────────────────────────────

await test("PendingMessage: status reflects message state", async () => {
  const { core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  const pending = await core.send("s1", "hello");
  assert(pending.localId.startsWith("local:"), "has localId");
  assert(pending.status === "sent", "status is sent after ack");
});

await test("PendingMessage: cancel removes the message", async () => {
  const { core } = makeCore({ publishError: new Error("fail") });
  core.attach();
  await core.joinStream("s1");

  let pending;
  try {
    pending = await core.send("s1", "hello");
  } catch {
    // Expected — send fails
  }

  // Message should be in failed state
  const msgs = core.getMessages("s1");
  assert(msgs.length === 1, "failed msg in list");
  assert(msgs[0].status === "failed", "status is failed");

  // Cancel via the localId (from the failed message)
  core.cancelSend(msgs[0].localId!);
  assert(core.getMessages("s1").length === 0, "message removed after cancel");

  core.destroy();
});

await test("PendingMessage: retry re-sends failed message", async () => {
  // First send fails, retry succeeds
  const { core } = makeCore({ publishError: new Error("fail") });
  core.attach();
  await core.joinStream("s1");

  try { await core.send("s1", "hello"); } catch {}

  const failedMsg = core.getMessages("s1")[0];
  assert(failedMsg.status === "failed", "initially failed");
  const failedLocalId = failedMsg.localId!;

  // Now allow retry to succeed by creating a new core (simpler test)
  // Instead, let's test the retry path is callable
  try {
    await core.retrySend(failedLocalId);
  } catch {
    // Expected to fail again since publishError is still set
  }

  // The old message should be replaced with a new optimistic
  const msgs = core.getMessages("s1");
  assert(msgs.length === 1, "still 1 message");
  assert(msgs[0].localId !== failedLocalId, "new localId after retry");

  core.destroy();
});

// ── Ephemeral events (N-5) ─────────────────────────────────────────

await test("ephemeral event fires notifier and is accessible via getLastEphemeral", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  let notified = 0;
  core.subscribe("ephemeral:s1", () => { notified++; });

  client.emit("event.received", { stream: "s1", event: "custom.ping", sender: "alice", data: { ts: 123 } });

  assert(notified === 1, "notifier fired");
  const last = core.getLastEphemeral("s1");
  assert(last !== undefined, "lastEphemeral set");
  assert(last!.event === "custom.ping", "correct event type");
  assert(last!.sender === "alice", "correct sender");
  assert((last!.data as { ts: number }).ts === 123, "correct data");
  core.destroy();
});

await test("ephemeral events do not persist in message store", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  client.emit("event.received", { stream: "s1", event: "custom.ping", sender: "alice" });

  assert(core.getMessages("s1").length === 0, "no messages from ephemeral");
  core.destroy();
});

await test("leaveStream clears lastEphemeral", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  client.emit("event.received", { stream: "s1", event: "custom.ping", sender: "alice" });
  assert(core.getLastEphemeral("s1") !== undefined, "set before leave");

  core.leaveStream("s1");
  assert(core.getLastEphemeral("s1") === undefined, "cleared after leave");
  core.destroy();
});

await test("getLastEphemeral with eventType filters by type", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  client.emit("event.received", { stream: "s1", event: "custom.ping", sender: "alice", data: { x: 1 } });
  client.emit("event.received", { stream: "s1", event: "custom.session", sender: "bob", data: { y: 2 } });

  const ping = core.getLastEphemeral("s1", "custom.ping");
  const session = core.getLastEphemeral("s1", "custom.session");
  const latest = core.getLastEphemeral("s1");
  const missing = core.getLastEphemeral("s1", "nonexistent");

  assert(ping !== undefined, "ping found");
  assert(ping!.sender === "alice", "ping sender");
  assert(session !== undefined, "session found");
  assert(session!.sender === "bob", "session sender");
  assert(latest !== undefined, "latest found");
  assert(latest!.event === "custom.session", "latest is most recent");
  assert(missing === undefined, "nonexistent type returns undefined");
  core.destroy();
});

await test("getLastEphemeral per-type: later event of same type overwrites", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  client.emit("event.received", { stream: "s1", event: "custom.ping", sender: "alice", data: { v: 1 } });
  client.emit("event.received", { stream: "s1", event: "custom.ping", sender: "bob", data: { v: 2 } });

  const ping = core.getLastEphemeral("s1", "custom.ping");
  assert(ping!.sender === "bob", "latest ping from bob");
  assert((ping!.data as { v: number }).v === 2, "latest data");
  core.destroy();
});

// ── Middleware (N-5) ──────────────────────────────────────────────

await test("middleware: passthrough (calling next) preserves normal behavior", async () => {
  const log: string[] = [];
  const { client, core } = makeCore(undefined, {
    middleware: [
      (_event, next) => { log.push("before"); next(); log.push("after"); },
    ],
  });
  core.attach();
  await core.joinStream("s1");

  client.emit("event", { stream: "s1", id: "e1", seq: 1, sender: "a", body: "hi", sent_at: 1 });

  assert(core.getMessages("s1").length === 1, "message stored");
  assert(log[0] === "before", "ran before next");
  assert(log[1] === "after", "ran after next");
  core.destroy();
});

await test("middleware: skip (not calling next) drops event", async () => {
  const { client, core } = makeCore(undefined, {
    middleware: [
      (_event, _next) => { /* intentionally skip next() */ },
    ],
  });
  core.attach();
  await core.joinStream("s1");

  client.emit("event", { stream: "s1", id: "e1", seq: 1, sender: "a", body: "hi", sent_at: 1 });

  assert(core.getMessages("s1").length === 0, "event dropped by middleware");
  core.destroy();
});

await test("middleware: enrich (modify event data before next)", async () => {
  const { client, core } = makeCore(undefined, {
    middleware: [
      (event, next) => {
        if (event.type === "event") {
          (event.data as { body: string }).body = "enriched";
        }
        next();
      },
    ],
  });
  core.attach();
  await core.joinStream("s1");

  client.emit("event", { stream: "s1", id: "e1", seq: 1, sender: "a", body: "original", sent_at: 1 });

  assert(core.getMessages("s1")[0].body === "enriched", "body was enriched");
  core.destroy();
});

await test("middleware: ordering — multiple middleware run in order", async () => {
  const order: number[] = [];
  const { client, core } = makeCore(undefined, {
    middleware: [
      (_event, next) => { order.push(1); next(); },
      (_event, next) => { order.push(2); next(); },
      (_event, next) => { order.push(3); next(); },
    ],
  });
  core.attach();
  await core.joinStream("s1");

  client.emit("event", { stream: "s1", id: "e1", seq: 1, sender: "a", body: "x", sent_at: 1 });

  assert(order.length === 3, "all 3 ran");
  assert(order[0] === 1 && order[1] === 2 && order[2] === 3, "correct order");
  core.destroy();
});

await test("middleware: early middleware can prevent later ones from running", async () => {
  const ran: number[] = [];
  const { client, core } = makeCore(undefined, {
    middleware: [
      (_event, _next) => { ran.push(1); /* skip next */ },
      (_event, next) => { ran.push(2); next(); },
    ],
  });
  core.attach();
  await core.joinStream("s1");

  client.emit("event", { stream: "s1", id: "e1", seq: 1, sender: "a", body: "x", sent_at: 1 });

  assert(ran.length === 1, "only first ran");
  assert(ran[0] === 1, "first middleware");
  assert(core.getMessages("s1").length === 0, "event dropped");
  core.destroy();
});

await test("middleware: receives correct ChatEvent type for each handler", async () => {
  const types: string[] = [];
  const { client, core } = makeCore(undefined, {
    middleware: [
      (event, next) => { types.push(event.type); next(); },
    ],
  });
  core.attach();
  await core.joinStream("s1");

  client.emit("event", { stream: "s1", id: "e1", seq: 1, sender: "a", body: "x", sent_at: 1 });
  client.emit("event.edited", { stream: "s1", id: "e1", seq: 1, body: "edited", edited_at: 2 });
  client.emit("event.deleted", { stream: "s1", id: "e1", seq: 1 });
  client.emit("reaction.changed", { stream: "s1", event_id: "e1", emoji: "🔥", user_id: "a", action: "add" });
  client.emit("typing", { stream: "s1", user_id: "alice", active: true });
  client.emit("member.joined", { stream: "s1", user_id: "bob", role: "member" });
  client.emit("member.left", { stream: "s1", user_id: "bob", role: "member" });
  client.emit("cursor", { stream: "s1", user_id: "alice", seq: 1 });
  client.emit("presence", { user_id: "alice", presence: "away" });
  client.emit("event.received", { stream: "s1", event: "custom", sender: "alice" });
  client.emit("event.delivered", { stream: "s1", user_id: "bob", seq: 1 });

  const expected = [
    "event", "event.edited", "event.deleted", "reaction.changed",
    "typing", "member.joined", "member.left", "cursor", "presence", "ephemeral", "event.delivered",
  ];
  assert(types.length === expected.length, `got ${types.length} events, expected ${expected.length}`);
  for (let i = 0; i < expected.length; i++) {
    assert(types[i] === expected[i], `type[${i}] expected ${expected[i]}, got ${types[i]}`);
  }
  core.destroy();
});

await test("middleware: can intercept ephemeral events", async () => {
  let intercepted = false;
  const { client, core } = makeCore(undefined, {
    middleware: [
      (event, next) => {
        if (event.type === "ephemeral") {
          intercepted = true;
        }
        next();
      },
    ],
  });
  core.attach();
  await core.joinStream("s1");

  client.emit("event.received", { stream: "s1", event: "custom", sender: "alice" });
  assert(intercepted, "ephemeral intercepted by middleware");
  assert(core.getLastEphemeral("s1") !== undefined, "still stored after passthrough");
  core.destroy();
});

// ── Delivery receipts ─────────────────────────────────────────────

await test("event.delivered flips self-sent message from sent to delivered", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  await core.send("s1", "hello");
  const msgs = core.getMessages("s1");
  assert(msgs.length === 1, "1 message");
  assert(msgs[0].status === "sent", "initially sent");

  // Remote user's delivery ack
  client.emit("event.delivered", { stream: "s1", user_id: "bob", seq: msgs[0].seq });

  const after = core.getMessages("s1");
  assert(after[0].status === "delivered", `expected delivered, got ${after[0].status}`);
  core.destroy();
});

await test("event.delivered does not flip other users' messages", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  client.emit("event", { stream: "s1", id: "e1", seq: 1, sender: "alice", body: "from alice", sent_at: 1 });

  client.emit("event.delivered", { stream: "s1", user_id: "bob", seq: 5 });

  assert(core.getMessages("s1")[0].status === "sent", "other user's message stays sent");
  core.destroy();
});

await test("own delivery ack is ignored", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  await core.send("s1", "hello");
  const seq = core.getMessages("s1")[0].seq;

  client.emit("event.delivered", { stream: "s1", user_id: "me", seq });

  assert(core.getMessages("s1")[0].status === "sent", "own delivery ack ignored");
  core.destroy();
});

await test("delivered → read: cursor advances past delivered message", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  await core.send("s1", "hello");
  const seq = core.getMessages("s1")[0].seq;

  // First: delivered
  client.emit("event.delivered", { stream: "s1", user_id: "bob", seq });
  assert(core.getMessages("s1")[0].status === "delivered", "now delivered");

  // Then: read (cursor advance)
  client.emit("cursor", { stream: "s1", user_id: "bob", seq });
  assert(core.getMessages("s1")[0].status === "read", "now read");
  core.destroy();
});

await test("event.delivered flows through middleware", async () => {
  const types: string[] = [];
  const { client, core } = makeCore(undefined, {
    middleware: [(event, next) => { types.push(event.type); next(); }],
  });
  core.attach();
  await core.joinStream("s1");

  client.emit("event.delivered", { stream: "s1", user_id: "bob", seq: 1 });

  assert(types.includes("event.delivered"), "middleware received event.delivered");
  core.destroy();
});

// ── Per-message read status (N-5) ─────────────────────────────────

await test("remote cursor flips self-sent message to read", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  // Send a message (auto-reconciles with mock)
  await core.send("s1", "hello");
  const msgs = core.getMessages("s1");
  assert(msgs.length === 1, "1 message");
  assert(msgs[0].status === "sent", "initially sent");

  // Remote user advances cursor past our message's seq
  client.emit("cursor", { stream: "s1", user_id: "bob", seq: msgs[0].seq });

  const after = core.getMessages("s1");
  assert(after[0].status === "read", `expected read, got ${after[0].status}`);
  core.destroy();
});

await test("own cursor does not trigger read status change", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  await core.send("s1", "hello");
  const seq = core.getMessages("s1")[0].seq;

  // Own cursor — should not flip to read
  client.emit("cursor", { stream: "s1", user_id: "me", seq });

  assert(core.getMessages("s1")[0].status === "sent", "still sent — own cursor ignored");
  core.destroy();
});

await test("cursor behind message seq does not trigger read", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  await core.send("s1", "hello");
  const seq = core.getMessages("s1")[0].seq;

  // Remote cursor at seq - 1 — should not flip
  client.emit("cursor", { stream: "s1", user_id: "bob", seq: seq - 1 });

  assert(core.getMessages("s1")[0].status === "sent", "still sent — cursor behind");
  core.destroy();
});

await test("remote cursor does not flip other users' messages to read", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  // Another user's message
  client.emit("event", { stream: "s1", id: "e1", seq: 1, sender: "alice", body: "from alice", sent_at: 1 });

  // Remote cursor past it
  client.emit("cursor", { stream: "s1", user_id: "bob", seq: 5 });

  assert(core.getMessages("s1")[0].status === "sent", "other user's message stays sent");
  core.destroy();
});

await test("getRemoteCursors returns remote cursor positions", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  client.emit("cursor", { stream: "s1", user_id: "alice", seq: 5 });
  client.emit("cursor", { stream: "s1", user_id: "bob", seq: 10 });

  const cursors = core.getRemoteCursors("s1");
  assert(cursors.get("alice") === 5, "alice at 5");
  assert(cursors.get("bob") === 10, "bob at 10");
  core.destroy();
});

await test("remote cursor MAX semantics — does not go backward", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  client.emit("cursor", { stream: "s1", user_id: "alice", seq: 10 });
  client.emit("cursor", { stream: "s1", user_id: "alice", seq: 5 });

  assert(core.getRemoteCursors("s1").get("alice") === 10, "stays at 10");
  core.destroy();
});

// ── Lightweight subscriptions (N-5) ───────────────────────────────

await test("listen subscribes but does not create message stores", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.listen("inbox");

  assert(client.subscribeCalls.length === 1, "subscribe called");

  // Emit an event — should not create message in store
  client.emit("event", { stream: "inbox", id: "e1", seq: 1, sender: "a", body: "hi", sent_at: 1 });
  assert(core.getMessages("inbox").length === 0, "no messages in listen-only store");
  core.destroy();
});

await test("listen-only stream tracks unread counts", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.listen("inbox");

  assert(core.getUnreadCount("inbox") === 0, "initially 0 unread");

  client.emit("event", { stream: "inbox", id: "e1", seq: 1, sender: "a", body: "hi", sent_at: 1 });
  client.emit("event", { stream: "inbox", id: "e2", seq: 2, sender: "a", body: "yo", sent_at: 2 });

  assert(core.getUnreadCount("inbox") === 2, `expected 2 unread, got ${core.getUnreadCount("inbox")}`);
  assert(core.getTotalUnreadCount() === 2, "total unread includes listen-only");
  core.destroy();
});

await test("unlisten clears cursor state", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.listen("inbox");

  client.emit("event", { stream: "inbox", id: "e1", seq: 1, sender: "a", body: "hi", sent_at: 1 });
  assert(core.getUnreadCount("inbox") === 1, "1 unread before unlisten");

  core.unlisten("inbox");
  assert(core.getUnreadCount("inbox") === 0, "unread cleared after unlisten");
  core.destroy();
});

await test("listen-only stream fires event: notifier slice", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.listen("inbox");

  let eventCount = 0;
  core.subscribe("event:inbox", () => { eventCount++; });

  client.emit("event", { stream: "inbox", id: "e1", seq: 1, sender: "a", body: "hi", sent_at: 1 });
  assert(eventCount === 1, "event: slice notified");
  core.destroy();
});

await test("listen-only stream does not update typing store", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.listen("inbox");

  client.emit("typing", { stream: "inbox", user_id: "alice", active: true });
  assert(core.getTypingUsers("inbox").length === 0, "typing not stored");
  core.destroy();
});

await test("listen-only stream fires typing: notifier slice", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.listen("inbox");

  let typingCount = 0;
  core.subscribe("typing:inbox", () => { typingCount++; });

  client.emit("typing", { stream: "inbox", user_id: "alice", active: true });
  assert(typingCount === 1, "typing: slice notified");
  core.destroy();
});

await test("listen-only stream receives ephemeral events", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.listen("inbox");

  let ephCount = 0;
  core.subscribe("ephemeral:inbox", () => { ephCount++; });

  client.emit("event.received", { stream: "inbox", event: "custom", sender: "a" });
  assert(ephCount === 1, "ephemeral notified on listen-only");
  assert(core.getLastEphemeral("inbox") !== undefined, "ephemeral stored");
  core.destroy();
});

await test("unlisten unsubscribes and cleans up", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.listen("inbox");

  client.emit("event.received", { stream: "inbox", event: "custom", sender: "a" });
  assert(core.getLastEphemeral("inbox") !== undefined, "ephemeral before unlisten");

  core.unlisten("inbox");

  assert(client.unsubscribeCalls.length === 1, "unsubscribe called");
  assert(core.getLastEphemeral("inbox") === undefined, "ephemeral cleared");
  core.destroy();
});

await test("full-state stream also fires event: notifier slice", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.joinStream("s1");

  let eventCount = 0;
  core.subscribe("event:s1", () => { eventCount++; });

  client.emit("event", { stream: "s1", id: "e1", seq: 1, sender: "a", body: "hi", sent_at: 1 });
  assert(eventCount === 1, "event: slice fires for full-state streams too");
  assert(core.getMessages("s1").length === 1, "messages still stored");
  core.destroy();
});

await test("listen-only member events fire notifier but don't update store", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.listen("inbox");

  let memberCount = 0;
  core.subscribe("members:inbox", () => { memberCount++; });

  client.emit("member.joined", { stream: "inbox", user_id: "alice", role: "member" });
  assert(memberCount === 1, "members: slice notified");
  assert(core.getMembers("inbox").length === 0, "no members in listen-only store");
  core.destroy();
});

await test("listen-only cursor events are skipped", async () => {
  const { client, core } = makeCore();
  core.attach();
  await core.listen("inbox");

  client.emit("cursor", { stream: "inbox", user_id: "alice", seq: 5 });
  assert(core.getRemoteCursors("inbox").size === 0, "no remote cursors for listen-only");
  core.destroy();
});

// ── Writer seam (persist-first) ────────────────────────────────────
// When a `writer` is provided in ChatCoreOptions, each mutation is routed
// through the app's API instead of publishing directly over the Herald
// WebSocket. This lets the app commit to its own DB *before* Herald sees
// the event — the correctness shape every durable chat product actually
// wants (Slack / Discord / Matrix all work this way). Any slot left empty
// falls back to the Herald WS path so apps can adopt incrementally.

await test("writer.send: routes through app, bypasses Herald publish()", async () => {
  const { client, chatClient, core: unused } = makeCore();
  unused.destroy();

  const appCalls: Array<{ localId: string; body: string; senderId: string }> = [];
  const writer = {
    send: async (draft: { localId: string; streamId: string; body: string; meta?: unknown; parentId?: string; senderId: string }) => {
      appCalls.push({ localId: draft.localId, body: draft.body, senderId: draft.senderId });
      return {
        id: "app_id_1",
        seq: 42,
        sent_at: 1000,
        body: draft.body.trim(), // app canonicalized (e.g. trimmed)
        meta: { canonical: true },
      };
    },
  };
  const client2 = new MockClient();
  const chatClient2 = new MockChatClient();
  const core = new ChatCore({
    client: client2 as unknown as HeraldClient,
    chat: chatClient2 as unknown as HeraldChatClient,
    userId: "me",
    scrollIdleMs: 0,
    writer,
  });
  core.attach();
  await core.joinStream("s1");

  const pending = await core.send("s1", "  hi  ");
  assert(pending.status === "sent", "sent after writer.send resolves");
  assert(appCalls.length === 1, "writer.send called once");
  assert(appCalls[0].senderId === "me", "draft includes senderId");
  assert(appCalls[0].localId.startsWith("local:"), "draft includes localId as idempotency key");
  assert(client2.publishCalls.length === 0, "Herald publish NOT called when writer.send provided");

  const msgs = core.getMessages("s1");
  assert(msgs.length === 1, "1 message after reconcile");
  assert(msgs[0].id === "app_id_1", "id replaced with server canonical");
  assert(msgs[0].seq === 42, "seq replaced with server canonical");
  assert(msgs[0].sentAt === 1000, "sent_at replaced with server canonical");
  assert(msgs[0].body === "hi", "body replaced with app-canonicalized value (trimmed)");
  assert((msgs[0].meta as { canonical?: boolean })?.canonical === true, "meta replaced with canonical");
  core.destroy();
});

await test("writer.send: omitted body/meta preserves optimistic values", async () => {
  const client = new MockClient();
  const chatClient = new MockChatClient();
  const writer = {
    // Minimal ack — app didn't canonicalize; optimistic body/meta should persist
    send: async (d: { localId: string }) => ({ id: "srv_1", seq: 1, sent_at: 100 }),
  };
  const core = new ChatCore({
    client: client as unknown as HeraldClient,
    chat: chatClient as unknown as HeraldChatClient,
    userId: "me", scrollIdleMs: 0, writer,
  });
  core.attach();
  await core.joinStream("s1");

  await core.send("s1", "hello", { meta: { tag: "x" } });
  const msgs = core.getMessages("s1");
  assert(msgs[0].body === "hello", "body preserved from optimistic");
  assert((msgs[0].meta as { tag?: string })?.tag === "x", "meta preserved from optimistic");
  core.destroy();
});

await test("writer.send: failure marks message as failed, like Herald publish", async () => {
  const client = new MockClient();
  const chatClient = new MockChatClient();
  const writer = {
    send: async () => { throw new Error("API down"); },
  };
  const core = new ChatCore({
    client: client as unknown as HeraldClient,
    chat: chatClient as unknown as HeraldChatClient,
    userId: "me", scrollIdleMs: 0, writer,
  });
  core.attach();
  await core.joinStream("s1");

  let threw = false;
  try { await core.send("s1", "fails"); } catch { threw = true; }
  assert(threw, "send should throw");
  const msgs = core.getMessages("s1");
  assert(msgs.length === 1 && msgs[0].status === "failed", "message marked failed");
  assert(client.publishCalls.length === 0, "Herald publish not called");
  core.destroy();
});

await test("writer.send: retrySend also goes through writer", async () => {
  let callCount = 0;
  const client = new MockClient();
  const chatClient = new MockChatClient();
  const writer = {
    send: async (d: { localId: string; body: string }) => {
      callCount++;
      if (callCount === 1) throw new Error("first fail");
      return { id: "srv_retry", seq: 7, sent_at: 200 };
    },
  };
  const core = new ChatCore({
    client: client as unknown as HeraldClient,
    chat: chatClient as unknown as HeraldChatClient,
    userId: "me", scrollIdleMs: 0, writer,
  });
  core.attach();
  await core.joinStream("s1");

  let failedLocalId: string | undefined;
  try { await core.send("s1", "retry via writer"); } catch {
    failedLocalId = core.getMessages("s1")[0].localId;
  }
  assert(failedLocalId !== undefined, "have failed localId");
  const serverId = await core.retrySend(failedLocalId!);
  assert(serverId === "srv_retry", "retry went through writer");
  assert(callCount === 2, "writer.send called twice (first fail, second ok)");
  assert(client.publishCalls.length === 0, "Herald publish never called");
  core.destroy();
});

await test("writer.edit: routes through app, bypasses chat.editEvent", async () => {
  const client = new MockClient();
  const chatClient = new MockChatClient();
  const editCalls: Array<{ s: string; id: string; body: string }> = [];
  const writer = {
    edit: async (s: string, id: string, body: string) => { editCalls.push({ s, id, body }); },
  };
  const core = new ChatCore({
    client: client as unknown as HeraldClient,
    chat: chatClient as unknown as HeraldChatClient,
    userId: "me", scrollIdleMs: 0, writer,
  });
  await core.edit("s1", "e1", "edited body");
  assert(editCalls.length === 1 && editCalls[0].body === "edited body", "writer.edit called");
  // chat.editEvent not called; MockChatClient has no edit counter but we can
  // verify by inverting: if the mock's editEvent had been hit and there was a
  // configured editError, we'd have seen a throw. Using a separate mock with
  // editError would be more conclusive — test immediately below covers that.
  core.destroy();
});

await test("writer.edit: omitted slot falls back to Herald chat.editEvent", async () => {
  // editError configured; without a writer.edit, ChatCore should surface it
  // from chat.editEvent (proving it went through the Herald path).
  const { core } = makeCore({ editError: new Error("chat editEvent called") });
  let threw = false;
  try { await core.edit("s1", "e1", "x"); } catch (e) {
    threw = (e as Error).message === "chat editEvent called";
  }
  assert(threw, "edit fell through to chat.editEvent when writer.edit absent");
  core.destroy();
});

await test("writer.delete: routes through app, bypasses chat.deleteEvent", async () => {
  const client = new MockClient();
  const chatClient = new MockChatClient({ deleteError: new Error("should not be called") });
  const deleteCalls: Array<{ s: string; id: string }> = [];
  const writer = {
    delete: async (s: string, id: string) => { deleteCalls.push({ s, id }); },
  };
  const core = new ChatCore({
    client: client as unknown as HeraldClient,
    chat: chatClient as unknown as HeraldChatClient,
    userId: "me", scrollIdleMs: 0, writer,
  });
  await core.deleteEvent("s1", "e1");
  assert(deleteCalls.length === 1, "writer.delete called");
  // chat.deleteEvent was configured to throw; if the writer took over it
  // wouldn't have been invoked, proving the bypass.
  core.destroy();
});

await test("writer.addReaction: routes through app, bypasses chat.addReaction", async () => {
  const client = new MockClient();
  const chatClient = new MockChatClient();
  const reactionCalls: Array<{ s: string; id: string; e: string }> = [];
  const writer = {
    addReaction: async (s: string, id: string, e: string) => { reactionCalls.push({ s, id, e }); },
  };
  const core = new ChatCore({
    client: client as unknown as HeraldClient,
    chat: chatClient as unknown as HeraldChatClient,
    userId: "me", scrollIdleMs: 0, writer,
  });
  await core.addReaction("s1", "e1", "🔥");
  assert(reactionCalls.length === 1 && reactionCalls[0].e === "🔥", "writer.addReaction called");
  assert(chatClient.reactionCalls.length === 0, "chat.addReaction NOT called");
  core.destroy();
});

await test("writer.removeReaction: routes through app, bypasses chat.removeReaction", async () => {
  const client = new MockClient();
  const chatClient = new MockChatClient();
  const reactionCalls: Array<{ s: string; id: string; e: string }> = [];
  const writer = {
    removeReaction: async (s: string, id: string, e: string) => { reactionCalls.push({ s, id, e }); },
  };
  const core = new ChatCore({
    client: client as unknown as HeraldClient,
    chat: chatClient as unknown as HeraldChatClient,
    userId: "me", scrollIdleMs: 0, writer,
  });
  await core.removeReaction("s1", "e1", "🔥");
  assert(reactionCalls.length === 1, "writer.removeReaction called");
  assert(chatClient.reactionCalls.length === 0, "chat.removeReaction NOT called");
  core.destroy();
});

await test("writer.updateCursor: routes through app, bypasses chat.updateCursor", async () => {
  const client = new MockClient({
    subscribePayload: () => ({
      stream: "s1",
      members: [{ user_id: "me", role: "member", presence: "online" }],
      cursor: 0,
      latest_seq: 10,
    }),
  });
  const chatClient = new MockChatClient();
  const cursorCalls: Array<{ s: string; seq: number }> = [];
  const writer = {
    updateCursor: async (s: string, seq: number) => { cursorCalls.push({ s, seq }); },
  };
  const core = new ChatCore({
    client: client as unknown as HeraldClient,
    chat: chatClient as unknown as HeraldChatClient,
    userId: "me", scrollIdleMs: 0, writer,
  });
  core.attach();
  await core.joinStream("s1");

  // Trigger autoMarkRead via live-edge signal — with scrollIdleMs=0 this fires immediately.
  // Append a real event first so there's something to mark read.
  client.emit("event", { stream: "s1", id: "srv_1", seq: 5, sender: "other", body: "hi", sent_at: 1, meta: undefined });
  core.setAtLiveEdge("s1", true);
  await new Promise((r) => setTimeout(r, 20));

  assert(cursorCalls.length >= 1, `writer.updateCursor called (got ${cursorCalls.length})`);
  assert(chatClient.cursorCalls.length === 0, "chat.updateCursor NOT called");
  core.destroy();
});

await test("writer: partial writer only overrides provided slots", async () => {
  // Writer provides send only; edit/delete/reaction/cursor should still
  // use the Herald defaults. Verifying edit falls through by configuring
  // editError on the chat mock — if writer.edit had been called, no throw.
  const client = new MockClient();
  const chatClient = new MockChatClient({ editError: new Error("chat edit called") });
  const sendCalls: Array<{ localId: string }> = [];
  const writer = {
    send: async (d: { localId: string }) => {
      sendCalls.push({ localId: d.localId });
      return { id: "srv_s", seq: 1, sent_at: 1 };
    },
  };
  const core = new ChatCore({
    client: client as unknown as HeraldClient,
    chat: chatClient as unknown as HeraldChatClient,
    userId: "me", scrollIdleMs: 0, writer,
  });
  core.attach();
  await core.joinStream("s1");

  await core.send("s1", "hello");
  assert(sendCalls.length === 1, "writer.send used");

  let editThrew = false;
  try { await core.edit("s1", "e1", "x"); } catch (e) {
    editThrew = (e as Error).message === "chat edit called";
  }
  assert(editThrew, "edit fell through to chat path");
  core.destroy();
});

// ── Summary ────────────────────────────────────────────────────────
console.log(`\nchat-core: ${passed} passed, ${failed} failed`);
process.exit(failed > 0 ? 1 : 0);
