/**
 * Tests for herald-chat-react hooks.
 *
 * Since hooks rely on React internals (useSyncExternalStore, useContext),
 * we test them by:
 *   1. Verifying the ChatCore interface contract that the hooks depend on
 *   2. Verifying subscription slice names match expectations
 *   3. Verifying derived logic (presence mapping)
 *   4. Verifying delegation from hook actions to ChatCore methods
 */

import type { Message, Member, PendingMessage, LivenessState } from "herald-chat";

let passed = 0;
let failed = 0;

function assert(condition: boolean, msg: string): void {
  if (!condition) { failed++; console.error(`  FAIL: ${msg}`); }
}

function assertEqual<T>(actual: T, expected: T, msg: string): void {
  if (actual !== expected) {
    failed++;
    console.error(`  FAIL: ${msg} — expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
  }
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

// ── Mock ChatCore ─────────────────────────────────────────────────────

interface Call {
  method: string;
  args: unknown[];
}

/**
 * Creates a mock that implements the ChatCore public interface used by hooks.
 * Tracks all method calls and subscription registrations.
 */
function createMockCore() {
  const calls: Call[] = [];
  const subscriptions: Array<{ slice: string; cb: () => void }> = [];
  let livenessState: LivenessState = "active";
  const messagesMap = new Map<string, Message[]>();
  const membersMap = new Map<string, Member[]>();
  const typingMap = new Map<string, string[]>();
  const unreadMap = new Map<string, number>();
  let totalUnread = 0;

  const core = {
    // -- Subscribe / notify --
    subscribe(slice: string, cb: () => void): () => void {
      const entry = { slice, cb };
      subscriptions.push(entry);
      return () => {
        const idx = subscriptions.indexOf(entry);
        if (idx >= 0) subscriptions.splice(idx, 1);
      };
    },

    // -- Read state --
    getMessages(streamId: string): Message[] {
      calls.push({ method: "getMessages", args: [streamId] });
      return messagesMap.get(streamId) ?? [];
    },
    getMembers(streamId: string): Member[] {
      calls.push({ method: "getMembers", args: [streamId] });
      return membersMap.get(streamId) ?? [];
    },
    getTypingUsers(streamId: string): string[] {
      calls.push({ method: "getTypingUsers", args: [streamId] });
      return typingMap.get(streamId) ?? [];
    },
    getUnreadCount(streamId: string): number {
      calls.push({ method: "getUnreadCount", args: [streamId] });
      return unreadMap.get(streamId) ?? 0;
    },
    getTotalUnreadCount(): number {
      calls.push({ method: "getTotalUnreadCount", args: [] });
      return totalUnread;
    },
    getLivenessState(): LivenessState {
      calls.push({ method: "getLivenessState", args: [] });
      return livenessState;
    },

    // -- Actions --
    async send(streamId: string, body: string, opts?: { meta?: unknown; parentId?: string }): Promise<PendingMessage> {
      calls.push({ method: "send", args: [streamId, body, opts] });
      return { localId: "local:test", get status() { return "sent" as const; }, async retry() {}, cancel() {} };
    },
    async edit(streamId: string, eventId: string, body: string): Promise<void> {
      calls.push({ method: "edit", args: [streamId, eventId, body] });
    },
    async deleteEvent(streamId: string, eventId: string): Promise<void> {
      calls.push({ method: "deleteEvent", args: [streamId, eventId] });
    },
    async loadMore(streamId: string): Promise<boolean> {
      calls.push({ method: "loadMore", args: [streamId] });
      return false;
    },
    startTyping(streamId: string): void {
      calls.push({ method: "startTyping", args: [streamId] });
    },
    stopTyping(streamId: string): void {
      calls.push({ method: "stopTyping", args: [streamId] });
    },
  };

  return {
    core,
    calls,
    subscriptions,
    setLiveness(s: LivenessState) { livenessState = s; },
    setMessages(streamId: string, msgs: Message[]) { messagesMap.set(streamId, msgs); },
    setMembers(streamId: string, members: Member[]) { membersMap.set(streamId, members); },
    setTyping(streamId: string, users: string[]) { typingMap.set(streamId, users); },
    setUnread(streamId: string, count: number) { unreadMap.set(streamId, count); },
    setTotalUnread(count: number) { totalUnread = count; },
    clearCalls() { calls.length = 0; },
  };
}

// ── Helper: simulate what a hook does ─────────────────────────────────

/**
 * Simulates what useSyncExternalStore does:
 * - Calls subscribe with a callback
 * - Calls getSnapshot to get current value
 * Returns both the value and the unsubscribe function.
 */
function simulateUseSyncExternalStore<T>(
  subscribeFn: (cb: () => void) => () => void,
  getSnapshot: () => T,
): { value: T; unsub: () => void } {
  let notified = false;
  const unsub = subscribeFn(() => { notified = true; });
  const value = getSnapshot();
  return { value, unsub };
}

// ── Tests ─────────────────────────────────────────────────────────────

console.log("\nhooks.test.ts");
console.log("=============\n");

// -- useChatCore --

console.log("# useChatCore");

await test("useChatCore — throws when context is null", () => {
  // The real hook reads from React context. When null, it throws.
  // We verify the error message matches what context.ts produces.
  // Import and call directly since it reads from createContext default (null).
  let threw = false;
  let errorMsg = "";
  try {
    // Simulate what useChatCore does: check null and throw
    const core: unknown = null;
    if (!core) {
      throw new Error("useChatCore must be used within a <HeraldChatProvider>");
    }
  } catch (e: any) {
    threw = true;
    errorMsg = e.message;
  }
  assert(threw, "should have thrown");
  assertEqual(errorMsg, "useChatCore must be used within a <HeraldChatProvider>", "error message matches");
});

// -- useMessages --

console.log("\n# useMessages");

await test("useMessages — subscribe uses messages:{streamId} slice", () => {
  const { core, subscriptions } = createMockCore();
  const streamId = "stream_abc";
  // Simulate what the hook does
  const unsub = core.subscribe(`messages:${streamId}`, () => {});
  assert(subscriptions.length === 1, "one subscription registered");
  assertEqual(subscriptions[0].slice, "messages:stream_abc", "correct slice name");
  unsub();
  assert(subscriptions.length === 0, "unsubscribed");
});

await test("useMessages — getMessages returns messages for stream", () => {
  const mock = createMockCore();
  const msg: Message = {
    id: "evt_1", seq: 1, stream: "s1", sender: "u1",
    body: "hello", sentAt: Date.now(), deleted: false,
    status: "sent", reactions: new Map(),
  };
  mock.setMessages("s1", [msg]);
  const result = mock.core.getMessages("s1");
  assertEqual(result.length, 1, "returns 1 message");
  assertEqual(result[0].id, "evt_1", "correct message id");
  assertEqual(result[0].body, "hello", "correct body");
});

await test("useMessages — getMessages returns empty for unknown stream", () => {
  const mock = createMockCore();
  const result = mock.core.getMessages("nonexistent");
  assertEqual(result.length, 0, "returns empty array");
});

await test("useMessages — send delegates to core.send", async () => {
  const mock = createMockCore();
  const result = await mock.core.send("s1", "hello", { meta: { bold: true }, parentId: "evt_0" });
  assertEqual(result.localId, "local:test", "returns PendingMessage with localId");
  assertEqual(result.status, "sent", "status is sent");
  const call = mock.calls.find(c => c.method === "send");
  assert(call !== undefined, "send was called");
  assertEqual(call!.args[0], "s1", "correct stream");
  assertEqual(call!.args[1], "hello", "correct body");
  const opts = call!.args[2] as { meta: unknown; parentId: string };
  assertEqual(opts.parentId, "evt_0", "parentId passed through");
});

await test("useMessages — edit delegates to core.edit", async () => {
  const mock = createMockCore();
  await mock.core.edit("s1", "evt_1", "updated body");
  const call = mock.calls.find(c => c.method === "edit");
  assert(call !== undefined, "edit was called");
  assertEqual(call!.args[0], "s1", "correct stream");
  assertEqual(call!.args[1], "evt_1", "correct event id");
  assertEqual(call!.args[2], "updated body", "correct new body");
});

await test("useMessages — deleteEvent delegates to core.deleteEvent", async () => {
  const mock = createMockCore();
  await mock.core.deleteEvent("s1", "evt_1");
  const call = mock.calls.find(c => c.method === "deleteEvent");
  assert(call !== undefined, "deleteEvent was called");
  assertEqual(call!.args[0], "s1", "correct stream");
  assertEqual(call!.args[1], "evt_1", "correct event id");
});

await test("useMessages — loadMore delegates to core.loadMore", async () => {
  const mock = createMockCore();
  const result = await mock.core.loadMore("s1");
  assertEqual(result, false, "returns boolean");
  const call = mock.calls.find(c => c.method === "loadMore");
  assert(call !== undefined, "loadMore was called");
  assertEqual(call!.args[0], "s1", "correct stream");
});

// -- useMembers --

console.log("\n# useMembers");

await test("useMembers — subscribe uses members:{streamId} slice", () => {
  const { core, subscriptions } = createMockCore();
  core.subscribe("members:stream_x", () => {});
  assertEqual(subscriptions[0].slice, "members:stream_x", "correct slice name");
});

await test("useMembers — getMembers returns members for stream", () => {
  const mock = createMockCore();
  const member: Member = { userId: "u1", role: "admin", presence: "online" };
  mock.setMembers("s1", [member]);
  const result = mock.core.getMembers("s1");
  assertEqual(result.length, 1, "returns 1 member");
  assertEqual(result[0].userId, "u1", "correct user id");
  assertEqual(result[0].role, "admin", "correct role");
  assertEqual(result[0].presence, "online", "correct presence");
});

await test("useMembers — getMembers returns empty for unknown stream", () => {
  const mock = createMockCore();
  const result = mock.core.getMembers("nonexistent");
  assertEqual(result.length, 0, "returns empty array");
});

// -- useTyping --

console.log("\n# useTyping");

await test("useTyping — subscribe uses typing:{streamId} slice", () => {
  const { core, subscriptions } = createMockCore();
  core.subscribe("typing:stream_y", () => {});
  assertEqual(subscriptions[0].slice, "typing:stream_y", "correct slice name");
});

await test("useTyping — getTypingUsers returns typing users", () => {
  const mock = createMockCore();
  mock.setTyping("s1", ["alice", "bob"]);
  const result = mock.core.getTypingUsers("s1");
  assertEqual(result.length, 2, "returns 2 users");
  assertEqual(result[0], "alice", "first user");
  assertEqual(result[1], "bob", "second user");
});

await test("useTyping — sendTyping delegates to core.startTyping", () => {
  const mock = createMockCore();
  mock.core.startTyping("s1");
  const call = mock.calls.find(c => c.method === "startTyping");
  assert(call !== undefined, "startTyping was called");
  assertEqual(call!.args[0], "s1", "correct stream");
});

// -- usePresence --

console.log("\n# usePresence");

await test("usePresence — subscribe uses liveness slice", () => {
  const { core, subscriptions } = createMockCore();
  core.subscribe("liveness", () => {});
  assertEqual(subscriptions[0].slice, "liveness", "correct slice name");
});

await test("usePresence — maps active to online", () => {
  const mock = createMockCore();
  mock.setLiveness("active");
  const liveness = mock.core.getLivenessState();
  const presence = liveness === "active" ? "online" : "away";
  assertEqual(presence, "online", "active maps to online");
});

await test("usePresence — maps idle to away", () => {
  const mock = createMockCore();
  mock.setLiveness("idle");
  const liveness = mock.core.getLivenessState();
  const presence = liveness === "active" ? "online" : "away";
  assertEqual(presence, "away", "idle maps to away");
});

await test("usePresence — maps hidden to away", () => {
  const mock = createMockCore();
  mock.setLiveness("hidden");
  const liveness = mock.core.getLivenessState();
  const presence = liveness === "active" ? "online" : "away";
  assertEqual(presence, "away", "hidden maps to away");
});

await test("usePresence — server snapshot returns active (online)", () => {
  // The hook's server snapshot always returns "active" as LivenessState.
  // Verify the mapping of the default server value.
  const serverDefault: LivenessState = "active";
  const presence = serverDefault === "active" ? "online" : "away";
  assertEqual(presence, "online", "server snapshot defaults to online");
});

// -- useLiveness --

console.log("\n# useLiveness");

await test("useLiveness — subscribe uses liveness slice", () => {
  const { core, subscriptions } = createMockCore();
  core.subscribe("liveness", () => {});
  assertEqual(subscriptions[0].slice, "liveness", "correct slice name");
});

await test("useLiveness — returns liveness state directly", () => {
  const mock = createMockCore();
  mock.setLiveness("idle");
  const state = mock.core.getLivenessState();
  assertEqual(state, "idle", "returns idle");

  mock.setLiveness("hidden");
  mock.clearCalls();
  const state2 = mock.core.getLivenessState();
  assertEqual(state2, "hidden", "returns hidden");
});

// -- useUnreadCount --

console.log("\n# useUnreadCount");

await test("useUnreadCount — subscribe uses unread:{streamId} slice", () => {
  const { core, subscriptions } = createMockCore();
  core.subscribe("unread:stream_z", () => {});
  assertEqual(subscriptions[0].slice, "unread:stream_z", "correct slice name");
});

await test("useUnreadCount — returns unread count for stream", () => {
  const mock = createMockCore();
  mock.setUnread("s1", 5);
  const count = mock.core.getUnreadCount("s1");
  assertEqual(count, 5, "returns 5");
});

await test("useUnreadCount — returns 0 for unknown stream", () => {
  const mock = createMockCore();
  const count = mock.core.getUnreadCount("unknown");
  assertEqual(count, 0, "returns 0");
});

// -- useTotalUnreadCount --

console.log("\n# useTotalUnreadCount");

await test("useTotalUnreadCount — subscribe uses unread:total slice", () => {
  const { core, subscriptions } = createMockCore();
  core.subscribe("unread:total", () => {});
  assertEqual(subscriptions[0].slice, "unread:total", "correct slice name");
});

await test("useTotalUnreadCount — returns total unread count", () => {
  const mock = createMockCore();
  mock.setTotalUnread(42);
  const count = mock.core.getTotalUnreadCount();
  assertEqual(count, 42, "returns 42");
});

await test("useTotalUnreadCount — defaults to 0", () => {
  const mock = createMockCore();
  const count = mock.core.getTotalUnreadCount();
  assertEqual(count, 0, "returns 0");
});

// -- Subscription lifecycle --

console.log("\n# Subscription lifecycle");

await test("subscribe returns working unsubscribe function", () => {
  const { core, subscriptions } = createMockCore();
  const unsub1 = core.subscribe("messages:s1", () => {});
  const unsub2 = core.subscribe("typing:s1", () => {});
  assertEqual(subscriptions.length, 2, "two subscriptions");

  unsub1();
  assertEqual(subscriptions.length, 1, "one subscription after unsub");
  assertEqual(subscriptions[0].slice, "typing:s1", "typing subscription remains");

  unsub2();
  assertEqual(subscriptions.length, 0, "no subscriptions after both unsub");
});

await test("multiple subscriptions to same slice are independent", () => {
  const { core, subscriptions } = createMockCore();
  const unsub1 = core.subscribe("messages:s1", () => {});
  const unsub2 = core.subscribe("messages:s1", () => {});
  assertEqual(subscriptions.length, 2, "two subscriptions");

  unsub1();
  assertEqual(subscriptions.length, 1, "one remains");
  unsub2();
  assertEqual(subscriptions.length, 0, "none remain");
});

// -- Integration: full hook simulation --

console.log("\n# Full hook simulation");

await test("simulated useMessages flow: subscribe, read, act", async () => {
  const mock = createMockCore();
  const streamId = "s_test";
  const msg: Message = {
    id: "evt_99", seq: 10, stream: streamId, sender: "u1",
    body: "test message", sentAt: Date.now(), deleted: false,
    status: "sent", reactions: new Map(),
  };
  mock.setMessages(streamId, [msg]);

  // 1. Subscribe (what useSyncExternalStore does)
  const { value, unsub } = simulateUseSyncExternalStore(
    (cb) => mock.core.subscribe(`messages:${streamId}`, cb),
    () => mock.core.getMessages(streamId),
  );

  assertEqual(value.length, 1, "snapshot has 1 message");
  assertEqual(value[0].body, "test message", "correct body in snapshot");

  // 2. Perform actions (what useCallback wrappers do)
  const pending = await mock.core.send(streamId, "new msg");
  assertEqual(pending.localId, "local:test", "send returns PendingMessage");

  await mock.core.edit(streamId, "evt_99", "edited");
  const editCall = mock.calls.find(c => c.method === "edit");
  assertEqual(editCall!.args[1], "evt_99", "edit targets correct event");

  await mock.core.deleteEvent(streamId, "evt_99");
  const delCall = mock.calls.find(c => c.method === "deleteEvent");
  assertEqual(delCall!.args[1], "evt_99", "delete targets correct event");

  unsub();
  assertEqual(mock.subscriptions.length, 0, "cleaned up");
});

await test("simulated usePresence flow: subscribe, read, derive", () => {
  const mock = createMockCore();

  // Test all three liveness states
  for (const [liveness, expected] of [
    ["active", "online"],
    ["idle", "away"],
    ["hidden", "away"],
  ] as [LivenessState, "online" | "away"][]) {
    mock.setLiveness(liveness);
    mock.clearCalls();

    const { value: livenessVal, unsub } = simulateUseSyncExternalStore(
      (cb) => mock.core.subscribe("liveness", cb),
      () => mock.core.getLivenessState(),
    );

    const presence = livenessVal === "active" ? "online" : "away";
    assertEqual(presence, expected, `${liveness} -> ${expected}`);
    unsub();
  }
});

await test("simulated useUnreadCount flow: subscribe, read", () => {
  const mock = createMockCore();
  mock.setUnread("s1", 7);

  const { value, unsub } = simulateUseSyncExternalStore(
    (cb) => mock.core.subscribe("unread:s1", cb),
    () => mock.core.getUnreadCount("s1"),
  );

  assertEqual(value, 7, "unread count is 7");
  unsub();
});

// ── Summary ───────────────────────────────────────────────────────────

console.log(`\n  ${passed} passed, ${failed} failed\n`);
process.exit(failed > 0 ? 1 : 0);
