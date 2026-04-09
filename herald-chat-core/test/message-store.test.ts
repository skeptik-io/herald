import { Notifier } from "../src/notifier.js";
import { MessageStore } from "../src/stores/message-store.js";
import type { EventNew, EventEdited, EventDeleted, EventAck, ReactionChanged } from "herald-sdk";

let passed = 0;
let failed = 0;

function assert(condition: boolean, msg: string): void {
  if (!condition) { failed++; console.error(`  FAIL: ${msg}`); }
}

function test(name: string, fn: () => void): void {
  const before = failed;
  try {
    fn();
    if (failed === before) { passed++; console.log(`  ok - ${name}`); }
    else { console.log(`  FAIL - ${name}`); }
  } catch (e) {
    failed++;
    console.error(`  FAIL - ${name}: ${e}`);
  }
}

function makeEvent(overrides: Partial<EventNew> & { id: string; seq: number; stream: string }): EventNew {
  return { sender: "user1", body: "hello", sent_at: Date.now(), ...overrides };
}

function makeStore(): { store: MessageStore; notifier: Notifier } {
  const notifier = new Notifier();
  return { store: new MessageStore(notifier), notifier };
}

// ── Append ─────────────────────────────────────────────────────────

test("append inserts and returns true", () => {
  const { store } = makeStore();
  assert(store.appendEvent(makeEvent({ id: "e1", seq: 1, stream: "s1" })) === true, "should return true");
  assert(store.getMessages("s1").length === 1, "should have 1 message");
});

test("append deduplicates by id", () => {
  const { store } = makeStore();
  store.appendEvent(makeEvent({ id: "e1", seq: 1, stream: "s1" }));
  assert(store.appendEvent(makeEvent({ id: "e1", seq: 1, stream: "s1" })) === false, "dup returns false");
  assert(store.getMessages("s1").length === 1, "still 1");
});

test("append deduplicates same id even with different seq", () => {
  const { store } = makeStore();
  store.appendEvent(makeEvent({ id: "e1", seq: 1, stream: "s1" }));
  assert(store.appendEvent(makeEvent({ id: "e1", seq: 99, stream: "s1" })) === false, "same id different seq");
  assert(store.getMessages("s1")[0].seq === 1, "original seq preserved");
});

test("append maintains ascending seq order regardless of arrival order", () => {
  const { store } = makeStore();
  store.appendEvent(makeEvent({ id: "e5", seq: 5, stream: "s1" }));
  store.appendEvent(makeEvent({ id: "e1", seq: 1, stream: "s1" }));
  store.appendEvent(makeEvent({ id: "e3", seq: 3, stream: "s1" }));
  store.appendEvent(makeEvent({ id: "e2", seq: 2, stream: "s1" }));
  store.appendEvent(makeEvent({ id: "e4", seq: 4, stream: "s1" }));

  const seqs = store.getMessages("s1").map((m) => m.seq);
  assert(JSON.stringify(seqs) === "[1,2,3,4,5]", `wrong order: ${JSON.stringify(seqs)}`);
});

test("append to different streams are isolated", () => {
  const { store } = makeStore();
  store.appendEvent(makeEvent({ id: "a1", seq: 1, stream: "s1" }));
  store.appendEvent(makeEvent({ id: "b1", seq: 1, stream: "s2" }));
  assert(store.getMessages("s1").length === 1, "s1 has 1");
  assert(store.getMessages("s2").length === 1, "s2 has 1");
  assert(store.getMessages("s1")[0].id === "a1", "s1 correct id");
  assert(store.getMessages("s2")[0].id === "b1", "s2 correct id");
});

test("append tracks oldest seq", () => {
  const { store } = makeStore();
  store.appendEvent(makeEvent({ id: "e5", seq: 5, stream: "s1" }));
  assert(store.getOldestSeq("s1") === 5, "oldest should be 5");
  store.appendEvent(makeEvent({ id: "e2", seq: 2, stream: "s1" }));
  assert(store.getOldestSeq("s1") === 2, "oldest should be 2");
  store.appendEvent(makeEvent({ id: "e7", seq: 7, stream: "s1" }));
  assert(store.getOldestSeq("s1") === 2, "oldest still 2");
});

test("append preserves all fields from EventNew", () => {
  const { store } = makeStore();
  store.appendEvent(makeEvent({
    id: "e1", seq: 1, stream: "s1", sender: "alice", body: "hi",
    meta: { foo: 1 }, parent_id: "parent", edited_at: 12345, sent_at: 99999,
  }));
  const m = store.getMessages("s1")[0];
  assert(m.sender === "alice", "sender");
  assert(m.body === "hi", "body");
  assert((m.meta as { foo: number }).foo === 1, "meta");
  assert(m.parentId === "parent", "parentId");
  assert(m.editedAt === 12345, "editedAt");
  assert(m.sentAt === 99999, "sentAt");
  assert(m.deleted === false, "not deleted");
  assert(m.status === "sent", "status sent");
  assert(m.reactions.size === 0, "empty reactions");
});

// ── Prepend batch ──────────────────────────────────────────────────

test("prependBatch inserts in correct order", () => {
  const { store } = makeStore();
  store.appendEvent(makeEvent({ id: "e10", seq: 10, stream: "s1" }));
  store.prependBatch("s1", [
    makeEvent({ id: "e3", seq: 3, stream: "s1" }),
    makeEvent({ id: "e1", seq: 1, stream: "s1" }),
    makeEvent({ id: "e7", seq: 7, stream: "s1" }),
  ], true);

  const seqs = store.getMessages("s1").map((m) => m.seq);
  assert(JSON.stringify(seqs) === "[1,3,7,10]", `wrong order: ${JSON.stringify(seqs)}`);
  assert(store.hasMoreHistory("s1") === true, "has_more true");
});

test("prependBatch deduplicates against existing", () => {
  const { store } = makeStore();
  store.appendEvent(makeEvent({ id: "e5", seq: 5, stream: "s1" }));
  store.prependBatch("s1", [
    makeEvent({ id: "e5", seq: 5, stream: "s1" }), // dup
    makeEvent({ id: "e3", seq: 3, stream: "s1" }),
  ], false);

  assert(store.getMessages("s1").length === 2, "should skip dup");
  assert(store.hasMoreHistory("s1") === false, "has_more false");
});

test("prependBatch updates oldest seq", () => {
  const { store } = makeStore();
  store.appendEvent(makeEvent({ id: "e10", seq: 10, stream: "s1" }));
  store.prependBatch("s1", [makeEvent({ id: "e2", seq: 2, stream: "s1" })], true);
  assert(store.getOldestSeq("s1") === 2, "oldest should be 2");
});

test("prependBatch with empty array is a no-op except hasMore update", () => {
  const { store, notifier } = makeStore();
  store.appendEvent(makeEvent({ id: "e1", seq: 1, stream: "s1" }));
  let notified = 0;
  notifier.subscribe("messages:s1", () => { notified++; });
  store.prependBatch("s1", [], false);
  assert(store.getMessages("s1").length === 1, "unchanged");
  assert(store.hasMoreHistory("s1") === false, "has_more updated");
  assert(notified === 0, "no notification for empty batch");
});

// ── Optimistic sends ───────────────────────────────────────────────

test("addOptimistic appends at end with seq 0 and status sending", () => {
  const { store } = makeStore();
  store.appendEvent(makeEvent({ id: "e1", seq: 1, stream: "s1" }));
  store.addOptimistic("s1", "local:1", "me", "pending", { x: 1 }, "parent1");

  const msgs = store.getMessages("s1");
  assert(msgs.length === 2, "2 messages");
  assert(msgs[1].seq === 0, "optimistic seq is 0");
  assert(msgs[1].status === "sending", "status sending");
  assert(msgs[1].localId === "local:1", "localId set");
  assert(msgs[1].sender === "me", "sender");
  assert(msgs[1].body === "pending", "body");
  assert((msgs[1].meta as { x: number }).x === 1, "meta");
  assert(msgs[1].parentId === "parent1", "parentId");
});

test("multiple optimistic sends stay at end in insertion order", () => {
  const { store } = makeStore();
  store.appendEvent(makeEvent({ id: "e1", seq: 1, stream: "s1" }));
  store.addOptimistic("s1", "local:a", "me", "first");
  store.addOptimistic("s1", "local:b", "me", "second");

  const msgs = store.getMessages("s1");
  assert(msgs.length === 3, "3 messages");
  assert(msgs[0].seq === 1, "real msg first");
  assert(msgs[1].body === "first", "first optimistic");
  assert(msgs[2].body === "second", "second optimistic");
});

test("reconcile updates optimistic to server values and re-sorts", () => {
  const { store } = makeStore();
  store.appendEvent(makeEvent({ id: "e1", seq: 1, stream: "s1" }));
  store.appendEvent(makeEvent({ id: "e3", seq: 3, stream: "s1" }));
  store.addOptimistic("s1", "local:x", "me", "will be seq 2");

  store.reconcile("local:x", { id: "e2", seq: 2, sent_at: 50000 });

  const msgs = store.getMessages("s1");
  assert(msgs.length === 3, "3 messages");
  assert(msgs[0].seq === 1 && msgs[1].seq === 2 && msgs[2].seq === 3, "sorted by seq");
  assert(msgs[1].id === "e2", "server id");
  assert(msgs[1].status === "sent", "status sent");
  assert(msgs[1].sentAt === 50000, "server sent_at");
  assert(msgs[1].localId === "local:x", "localId preserved");
});

test("reconcile with unknown localId is a no-op", () => {
  const { store } = makeStore();
  store.addOptimistic("s1", "local:1", "me", "msg");
  store.reconcile("local:nonexistent", { id: "e1", seq: 1, sent_at: Date.now() });
  assert(store.getMessages("s1").length === 1, "unchanged");
  assert(store.getMessages("s1")[0].localId === "local:1", "original still there");
});

test("reconcile when event.new already arrived removes optimistic", () => {
  const { store } = makeStore();
  store.addOptimistic("s1", "local:1", "me", "msg");
  // event.new arrives first (race)
  store.appendEvent(makeEvent({ id: "srv1", seq: 5, stream: "s1", sender: "me", body: "msg" }));
  assert(store.getMessages("s1").length === 2, "both present before reconcile");

  store.reconcile("local:1", { id: "srv1", seq: 5, sent_at: Date.now() });
  assert(store.getMessages("s1").length === 1, "optimistic removed");
  assert(store.getMessages("s1")[0].id === "srv1", "server message kept");
});

test("failOptimistic sets status to failed", () => {
  const { store } = makeStore();
  store.addOptimistic("s1", "local:1", "me", "msg");
  store.failOptimistic("local:1");
  assert(store.getMessages("s1")[0].status === "failed", "status failed");
});

test("failOptimistic with unknown localId is a no-op", () => {
  const { store } = makeStore();
  store.failOptimistic("local:nonexistent"); // should not throw
});

test("removeOptimistic removes the message entirely", () => {
  const { store } = makeStore();
  store.addOptimistic("s1", "local:1", "me", "msg");
  store.removeOptimistic("local:1");
  assert(store.getMessages("s1").length === 0, "removed");
});

test("removeOptimistic with unknown localId is a no-op", () => {
  const { store } = makeStore();
  store.removeOptimistic("local:nonexistent"); // should not throw
});

// ── Edit ───────────────────────────────────────────────────────────

test("applyEdit updates body and editedAt", () => {
  const { store } = makeStore();
  store.appendEvent(makeEvent({ id: "e1", seq: 1, stream: "s1", body: "original" }));
  store.applyEdit({ stream: "s1", id: "e1", seq: 1, body: "edited", edited_at: 12345 });

  const m = store.getMessages("s1")[0];
  assert(m.body === "edited", "body updated");
  assert(m.editedAt === 12345, "editedAt set");
});

test("applyEdit for unknown id is a no-op", () => {
  const { store } = makeStore();
  store.applyEdit({ stream: "s1", id: "nonexistent", seq: 1, body: "x", edited_at: 1 });
  // should not throw
});

test("applyEdit to empty body", () => {
  const { store } = makeStore();
  store.appendEvent(makeEvent({ id: "e1", seq: 1, stream: "s1", body: "original" }));
  store.applyEdit({ stream: "s1", id: "e1", seq: 1, body: "", edited_at: 1 });
  assert(store.getMessages("s1")[0].body === "", "body empty");
});

test("multiple edits to same message", () => {
  const { store } = makeStore();
  store.appendEvent(makeEvent({ id: "e1", seq: 1, stream: "s1", body: "v1" }));
  store.applyEdit({ stream: "s1", id: "e1", seq: 1, body: "v2", edited_at: 1 });
  store.applyEdit({ stream: "s1", id: "e1", seq: 1, body: "v3", edited_at: 2 });
  assert(store.getMessages("s1")[0].body === "v3", "latest edit wins");
  assert(store.getMessages("s1")[0].editedAt === 2, "latest editedAt");
});

// ── Delete ─────────────────────────────────────────────────────────

test("applyDelete marks deleted and clears body", () => {
  const { store } = makeStore();
  store.appendEvent(makeEvent({ id: "e1", seq: 1, stream: "s1", body: "content" }));
  store.applyDelete({ stream: "s1", id: "e1", seq: 1 });

  const m = store.getMessages("s1")[0];
  assert(m.deleted === true, "deleted");
  assert(m.body === "", "body cleared");
  // Message stays in list (tombstone)
  assert(store.getMessages("s1").length === 1, "still in list");
});

test("applyDelete for unknown id is a no-op", () => {
  const { store } = makeStore();
  store.applyDelete({ stream: "s1", id: "nonexistent", seq: 1 });
});

test("edit after delete is rejected (tombstone guard)", () => {
  const { store } = makeStore();
  store.appendEvent(makeEvent({ id: "e1", seq: 1, stream: "s1", body: "orig" }));
  store.applyDelete({ stream: "s1", id: "e1", seq: 1 });
  store.applyEdit({ stream: "s1", id: "e1", seq: 1, body: "ghost edit", edited_at: 1 });
  assert(store.getMessages("s1")[0].body === "", "body stays empty");
  assert(store.getMessages("s1")[0].deleted === true, "still deleted");
  assert(store.getMessages("s1")[0].editedAt === undefined, "no editedAt on tombstone");
});

// ── Reactions ──────────────────────────────────────────────────────

test("add reaction creates emoji with user", () => {
  const { store } = makeStore();
  store.appendEvent(makeEvent({ id: "e1", seq: 1, stream: "s1" }));
  store.applyReaction({ stream: "s1", event_id: "e1", emoji: "👍", user_id: "alice", action: "add" });

  const r = store.getMessages("s1")[0].reactions;
  assert(r.has("👍"), "has emoji");
  assert(r.get("👍")!.has("alice"), "has user");
});

test("multiple users on same emoji", () => {
  const { store } = makeStore();
  store.appendEvent(makeEvent({ id: "e1", seq: 1, stream: "s1" }));
  store.applyReaction({ stream: "s1", event_id: "e1", emoji: "👍", user_id: "alice", action: "add" });
  store.applyReaction({ stream: "s1", event_id: "e1", emoji: "👍", user_id: "bob", action: "add" });

  const users = store.getMessages("s1")[0].reactions.get("👍")!;
  assert(users.size === 2, "2 users");
  assert(users.has("alice") && users.has("bob"), "both users");
});

test("remove reaction deletes user from emoji", () => {
  const { store } = makeStore();
  store.appendEvent(makeEvent({ id: "e1", seq: 1, stream: "s1" }));
  store.applyReaction({ stream: "s1", event_id: "e1", emoji: "👍", user_id: "alice", action: "add" });
  store.applyReaction({ stream: "s1", event_id: "e1", emoji: "👍", user_id: "bob", action: "add" });
  store.applyReaction({ stream: "s1", event_id: "e1", emoji: "👍", user_id: "alice", action: "remove" });

  const users = store.getMessages("s1")[0].reactions.get("👍")!;
  assert(users.size === 1, "1 user left");
  assert(!users.has("alice"), "alice removed");
});

test("remove last user on emoji removes the emoji key", () => {
  const { store } = makeStore();
  store.appendEvent(makeEvent({ id: "e1", seq: 1, stream: "s1" }));
  store.applyReaction({ stream: "s1", event_id: "e1", emoji: "👍", user_id: "alice", action: "add" });
  store.applyReaction({ stream: "s1", event_id: "e1", emoji: "👍", user_id: "alice", action: "remove" });
  assert(!store.getMessages("s1")[0].reactions.has("👍"), "emoji key removed");
});

test("remove reaction for unknown emoji is a no-op", () => {
  const { store } = makeStore();
  store.appendEvent(makeEvent({ id: "e1", seq: 1, stream: "s1" }));
  store.applyReaction({ stream: "s1", event_id: "e1", emoji: "🤷", user_id: "alice", action: "remove" });
  assert(store.getMessages("s1")[0].reactions.size === 0, "still empty");
});

test("reaction for unknown event_id is a no-op", () => {
  const { store } = makeStore();
  store.applyReaction({ stream: "s1", event_id: "nonexistent", emoji: "👍", user_id: "alice", action: "add" });
  // should not throw
});

test("duplicate add is idempotent", () => {
  const { store } = makeStore();
  store.appendEvent(makeEvent({ id: "e1", seq: 1, stream: "s1" }));
  store.applyReaction({ stream: "s1", event_id: "e1", emoji: "👍", user_id: "alice", action: "add" });
  store.applyReaction({ stream: "s1", event_id: "e1", emoji: "👍", user_id: "alice", action: "add" });
  assert(store.getMessages("s1")[0].reactions.get("👍")!.size === 1, "still 1 user");
});

// ── Notifications ──────────────────────────────────────────────────

test("each mutation fires notification for the stream", () => {
  const { store, notifier } = makeStore();
  let count = 0;
  notifier.subscribe("messages:s1", () => { count++; });

  store.appendEvent(makeEvent({ id: "e1", seq: 1, stream: "s1" }));
  assert(count === 1, "append");
  store.applyEdit({ stream: "s1", id: "e1", seq: 1, body: "ed", edited_at: 1 });
  assert(count === 2, "edit");
  store.applyDelete({ stream: "s1", id: "e1", seq: 1 });
  assert(count === 3, "delete");
  store.applyReaction({ stream: "s1", event_id: "e1", emoji: "x", user_id: "u", action: "add" });
  assert(count === 4, "reaction");
});

test("mutations on stream s1 do not notify stream s2", () => {
  const { store, notifier } = makeStore();
  let s2count = 0;
  notifier.subscribe("messages:s2", () => { s2count++; });
  store.appendEvent(makeEvent({ id: "e1", seq: 1, stream: "s1" }));
  assert(s2count === 0, "s2 not notified");
});

// ── Referential stability ──────────────────────────────────────────

test("getMessages returns new array ref after mutation", () => {
  const { store } = makeStore();
  store.appendEvent(makeEvent({ id: "e1", seq: 1, stream: "s1" }));
  const ref1 = store.getMessages("s1");
  store.appendEvent(makeEvent({ id: "e2", seq: 2, stream: "s1" }));
  const ref2 = store.getMessages("s1");
  assert(ref1 !== ref2, "different array refs");
});

test("getMessages returns same ref when no mutation", () => {
  const { store } = makeStore();
  store.appendEvent(makeEvent({ id: "e1", seq: 1, stream: "s1" }));
  const ref1 = store.getMessages("s1");
  const ref2 = store.getMessages("s1");
  assert(ref1 === ref2, "same array ref");
});

test("getMessages for unknown stream returns stable empty", () => {
  const { store } = makeStore();
  const ref1 = store.getMessages("nope");
  const ref2 = store.getMessages("nope");
  assert(ref1 === ref2, "same empty ref");
  assert(ref1.length === 0, "empty");
});

// ── Clear ──────────────────────────────────────────────────────────

test("clear removes all data for a stream", () => {
  const { store } = makeStore();
  store.appendEvent(makeEvent({ id: "e1", seq: 1, stream: "s1" }));
  store.appendEvent(makeEvent({ id: "e2", seq: 1, stream: "s2" }));
  store.clear("s1");
  assert(store.getMessages("s1").length === 0, "s1 cleared");
  assert(store.getMessages("s2").length === 1, "s2 intact");
  assert(store.getOldestSeq("s1") === undefined, "oldest cleared");
});

test("clearAll removes everything", () => {
  const { store } = makeStore();
  store.appendEvent(makeEvent({ id: "e1", seq: 1, stream: "s1" }));
  store.appendEvent(makeEvent({ id: "e2", seq: 1, stream: "s2" }));
  store.addOptimistic("s1", "local:1", "me", "x");
  store.clearAll();
  assert(store.getMessages("s1").length === 0, "s1 empty");
  assert(store.getMessages("s2").length === 0, "s2 empty");
});

// ── Stress / volume ────────────────────────────────────────────────

test("100 messages maintain correct order", () => {
  const { store } = makeStore();
  // Insert in reverse
  for (let i = 100; i >= 1; i--) {
    store.appendEvent(makeEvent({ id: `e${i}`, seq: i, stream: "s1" }));
  }
  const msgs = store.getMessages("s1");
  assert(msgs.length === 100, "100 messages");
  for (let i = 0; i < 100; i++) {
    assert(msgs[i].seq === i + 1, `msg ${i} has seq ${msgs[i].seq} expected ${i + 1}`);
  }
});

test("interleaved optimistic sends and real events", () => {
  const { store } = makeStore();
  store.appendEvent(makeEvent({ id: "e1", seq: 1, stream: "s1" }));
  store.addOptimistic("s1", "local:a", "me", "a");
  store.appendEvent(makeEvent({ id: "e2", seq: 2, stream: "s1" }));
  store.addOptimistic("s1", "local:b", "me", "b");
  store.appendEvent(makeEvent({ id: "e3", seq: 3, stream: "s1" }));

  // Real messages sorted, optimistic at end
  const msgs = store.getMessages("s1");
  assert(msgs.length === 5, "5 messages");
  assert(msgs[0].seq === 1 && msgs[1].seq === 2 && msgs[2].seq === 3, "real sorted");
  assert(msgs[3].seq === 0 && msgs[4].seq === 0, "optimistic at end");

  // Reconcile first optimistic as seq 4
  store.reconcile("local:a", { id: "e4", seq: 4, sent_at: Date.now() });
  const after = store.getMessages("s1");
  assert(after.length === 5, "still 5");
  assert(after[3].seq === 4 && after[3].id === "e4", "reconciled in position");
  assert(after[4].seq === 0, "second optimistic still at end");
});

// ── Summary ────────────────────────────────────────────────────────
console.log(`\nmessage-store: ${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
