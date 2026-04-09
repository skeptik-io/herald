import { Notifier } from "../src/notifier.js";
import { CursorStore } from "../src/stores/cursor-store.js";

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

function makeStore(): { store: CursorStore; notifier: Notifier } {
  const notifier = new Notifier();
  return { store: new CursorStore(notifier), notifier };
}

// ── Init ───────────────────────────────────────────────────────────

test("initStream sets cursor and latestSeq", () => {
  const { store } = makeStore();
  store.initStream("s1", 5, 10);
  assert(store.getMyCursor("s1") === 5, "cursor 5");
  assert(store.getUnreadCount("s1") === 5, "unread 5");
});

test("initStream with cursor equal to latest = 0 unread", () => {
  const { store } = makeStore();
  store.initStream("s1", 10, 10);
  assert(store.getUnreadCount("s1") === 0, "0 unread");
});

test("initStream with cursor 0 and latest 0", () => {
  const { store } = makeStore();
  store.initStream("s1", 0, 0);
  assert(store.getUnreadCount("s1") === 0, "0 unread");
  assert(store.getMyCursor("s1") === 0, "cursor 0");
});

test("re-init overwrites previous state", () => {
  const { store } = makeStore();
  store.initStream("s1", 5, 10);
  store.initStream("s1", 20, 20);
  assert(store.getMyCursor("s1") === 20, "cursor overwritten");
  assert(store.getUnreadCount("s1") === 0, "unread recalculated");
});

// ── MAX semantics ──────────────────────────────────────────────────

test("updateMyCursor advances forward", () => {
  const { store } = makeStore();
  store.initStream("s1", 5, 10);
  store.updateMyCursor("s1", 8);
  assert(store.getMyCursor("s1") === 8, "cursor 8");
  assert(store.getUnreadCount("s1") === 2, "unread 2");
});

test("updateMyCursor ignores lower value", () => {
  const { store } = makeStore();
  store.initStream("s1", 5, 10);
  store.updateMyCursor("s1", 3);
  assert(store.getMyCursor("s1") === 5, "cursor still 5");
});

test("updateMyCursor ignores equal value", () => {
  const { store, notifier } = makeStore();
  store.initStream("s1", 5, 10);
  let count = 0;
  notifier.subscribe("unread:s1", () => { count++; });
  store.updateMyCursor("s1", 5);
  assert(count === 0, "no notification for same value");
});

test("updateMyCursor on unknown stream is a no-op", () => {
  const { store, notifier } = makeStore();
  let count = 0;
  notifier.subscribe("unread:nope", () => { count++; });
  store.updateMyCursor("nope", 5);
  assert(store.getMyCursor("nope") === 0, "cursor 0 for unknown");
  assert(count === 0, "no notification");
});

// ── bumpLatestSeq ──────────────────────────────────────────────────

test("bumpLatestSeq increases unread", () => {
  const { store } = makeStore();
  store.initStream("s1", 10, 10);
  store.bumpLatestSeq("s1", 13);
  assert(store.getUnreadCount("s1") === 3, "unread 3");
});

test("bumpLatestSeq ignores lower value", () => {
  const { store } = makeStore();
  store.initStream("s1", 5, 10);
  store.bumpLatestSeq("s1", 8);
  assert(store.getUnreadCount("s1") === 5, "unchanged unread");
});

test("bumpLatestSeq ignores equal value", () => {
  const { store, notifier } = makeStore();
  store.initStream("s1", 5, 10);
  let count = 0;
  notifier.subscribe("unread:s1", () => { count++; });
  store.bumpLatestSeq("s1", 10);
  assert(count === 0, "no notification");
});

// ── Total unread ───────────────────────────────────────────────────

test("getTotalUnreadCount sums across streams", () => {
  const { store } = makeStore();
  store.initStream("s1", 5, 10);   // 5 unread
  store.initStream("s2", 8, 20);   // 12 unread
  store.initStream("s3", 100, 100); // 0 unread
  assert(store.getTotalUnreadCount() === 17, `expected 17, got ${store.getTotalUnreadCount()}`);
});

test("getTotalUnreadCount updates when cursor advances", () => {
  const { store } = makeStore();
  store.initStream("s1", 0, 10);
  store.initStream("s2", 0, 5);
  assert(store.getTotalUnreadCount() === 15, "15 initially");
  store.updateMyCursor("s1", 10);
  assert(store.getTotalUnreadCount() === 5, "5 after marking s1 read");
});

test("getTotalUnreadCount returns 0 with no streams", () => {
  const { store } = makeStore();
  assert(store.getTotalUnreadCount() === 0, "0 with no streams");
});

// ── Notifications ──────────────────────────────────────────────────

test("initStream notifies both stream and total", () => {
  const { store, notifier } = makeStore();
  let streamCount = 0, totalCount = 0;
  notifier.subscribe("unread:s1", () => { streamCount++; });
  notifier.subscribe("unread:total", () => { totalCount++; });
  store.initStream("s1", 0, 5);
  assert(streamCount === 1, "stream notified");
  assert(totalCount === 1, "total notified");
});

test("bumpLatestSeq notifies both", () => {
  const { store, notifier } = makeStore();
  store.initStream("s1", 0, 0);
  let streamCount = 0, totalCount = 0;
  notifier.subscribe("unread:s1", () => { streamCount++; });
  notifier.subscribe("unread:total", () => { totalCount++; });
  store.bumpLatestSeq("s1", 3);
  assert(streamCount === 1, "stream");
  assert(totalCount === 1, "total");
});

test("updateMyCursor notifies both", () => {
  const { store, notifier } = makeStore();
  store.initStream("s1", 0, 10);
  let streamCount = 0, totalCount = 0;
  notifier.subscribe("unread:s1", () => { streamCount++; });
  notifier.subscribe("unread:total", () => { totalCount++; });
  store.updateMyCursor("s1", 5);
  assert(streamCount === 1, "stream");
  assert(totalCount === 1, "total");
});

// ── Clear ──────────────────────────────────────────────────────────

test("clear removes a stream", () => {
  const { store } = makeStore();
  store.initStream("s1", 0, 10);
  store.initStream("s2", 0, 5);
  store.clear("s1");
  assert(store.getMyCursor("s1") === 0, "s1 cursor gone");
  assert(store.getUnreadCount("s1") === 0, "s1 unread 0");
  assert(store.getTotalUnreadCount() === 5, "total only s2");
});

test("clearAll removes everything", () => {
  const { store } = makeStore();
  store.initStream("s1", 0, 10);
  store.initStream("s2", 0, 5);
  store.clearAll();
  assert(store.getTotalUnreadCount() === 0, "0 total");
});

// ── Edge: unread never negative ────────────────────────────────────

test("unread is clamped to 0 when cursor exceeds latest", () => {
  const { store } = makeStore();
  store.initStream("s1", 10, 5); // cursor ahead of latest (shouldn't normally happen)
  assert(store.getUnreadCount("s1") === 0, "clamped to 0");
});

// ── Summary ────────────────────────────────────────────────────────
console.log(`\ncursor-store: ${passed} passed, ${failed} failed`);
process.exit(failed > 0 ? 1 : 0);
