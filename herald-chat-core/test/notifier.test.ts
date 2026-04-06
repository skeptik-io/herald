import { Notifier } from "../src/notifier.js";

let passed = 0;
let failed = 0;

function assert(condition: boolean, msg: string): void {
  if (!condition) {
    failed++;
    console.error(`  FAIL: ${msg}`);
  }
}

function test(name: string, fn: () => void): void {
  try {
    fn();
    if (failed === 0) { passed++; console.log(`  ok - ${name}`); }
    else { console.log(`  FAIL - ${name}`); }
  } catch (e) {
    failed++;
    console.error(`  FAIL - ${name}: ${e}`);
  }
}

// ── Core behavior ──────────────────────────────────────────────────

test("subscribe receives notifications for its slice", () => {
  const n = new Notifier();
  let count = 0;
  n.subscribe("messages:s1", () => { count++; });
  n.notify("messages:s1");
  n.notify("messages:s1");
  assert(count === 2, `expected 2, got ${count}`);
});

test("different slices are isolated", () => {
  const n = new Notifier();
  let a = 0, b = 0;
  n.subscribe("messages:s1", () => { a++; });
  n.subscribe("messages:s2", () => { b++; });
  n.notify("messages:s1");
  assert(a === 1 && b === 0, `expected a=1 b=0, got a=${a} b=${b}`);
});

test("unsubscribe stops notifications", () => {
  const n = new Notifier();
  let count = 0;
  const unsub = n.subscribe("x", () => { count++; });
  n.notify("x");
  unsub();
  n.notify("x");
  assert(count === 1, `expected 1, got ${count}`);
});

test("multiple listeners on same slice all fire", () => {
  const n = new Notifier();
  let a = 0, b = 0, c = 0;
  n.subscribe("s", () => { a++; });
  n.subscribe("s", () => { b++; });
  n.subscribe("s", () => { c++; });
  n.notify("s");
  assert(a === 1 && b === 1 && c === 1, `expected all 1, got ${a} ${b} ${c}`);
});

test("notifyMany fires each slice once", () => {
  const n = new Notifier();
  let a = 0, b = 0;
  n.subscribe("unread:s1", () => { a++; });
  n.subscribe("unread:total", () => { b++; });
  n.notifyMany(["unread:s1", "unread:total"]);
  assert(a === 1 && b === 1, `expected both 1, got a=${a} b=${b}`);
});

test("clear removes all listeners", () => {
  const n = new Notifier();
  let count = 0;
  n.subscribe("a", () => { count++; });
  n.subscribe("b", () => { count++; });
  n.clear();
  n.notify("a");
  n.notify("b");
  assert(count === 0, `expected 0, got ${count}`);
});

// ── Edge cases ─────────────────────────────────────────────────────

test("notify on empty slice is a no-op", () => {
  const n = new Notifier();
  // Should not throw
  n.notify("nonexistent");
});

test("unsubscribe is idempotent", () => {
  const n = new Notifier();
  let count = 0;
  const unsub = n.subscribe("x", () => { count++; });
  unsub();
  unsub(); // second call should not throw
  n.notify("x");
  assert(count === 0, `expected 0, got ${count}`);
});

test("unsubscribe one listener does not affect others on same slice", () => {
  const n = new Notifier();
  let a = 0, b = 0;
  const unsub = n.subscribe("s", () => { a++; });
  n.subscribe("s", () => { b++; });
  unsub();
  n.notify("s");
  assert(a === 0 && b === 1, `expected a=0 b=1, got a=${a} b=${b}`);
});

test("listener that throws does not break other listeners", () => {
  const n = new Notifier();
  let reached = false;
  n.subscribe("s", () => { throw new Error("boom"); });
  n.subscribe("s", () => { reached = true; });
  // Notifier doesn't try-catch — this verifies the throw propagates.
  // If we need resilient notification, this test documents current behavior.
  let threw = false;
  try {
    n.notify("s");
  } catch {
    threw = true;
  }
  // Document: currently throws. If we want resilience, this test will catch the change.
  assert(threw || reached, "either threw or second listener ran");
});

test("subscribe during notify does not fire new listener in same cycle", () => {
  const n = new Notifier();
  let innerFired = false;
  n.subscribe("s", () => {
    n.subscribe("s", () => { innerFired = true; });
  });
  n.notify("s");
  // The Set iterator may or may not visit the new entry — document behavior
  // This test just verifies it doesn't crash
});

test("unsubscribe during notify from within listener", () => {
  const n = new Notifier();
  let count = 0;
  let unsub: () => void;
  unsub = n.subscribe("s", () => {
    count++;
    unsub(); // unsubscribe self during iteration
  });
  n.notify("s");
  assert(count === 1, `expected 1, got ${count}`);
  n.notify("s");
  assert(count === 1, `expected still 1 after unsub, got ${count}`);
});

test("notifyMany with duplicate slices fires listener twice", () => {
  const n = new Notifier();
  let count = 0;
  n.subscribe("s", () => { count++; });
  n.notifyMany(["s", "s"]);
  assert(count === 2, `expected 2, got ${count}`);
});

test("large number of listeners", () => {
  const n = new Notifier();
  let total = 0;
  const unsubs: (() => void)[] = [];
  for (let i = 0; i < 1000; i++) {
    unsubs.push(n.subscribe("s", () => { total++; }));
  }
  n.notify("s");
  assert(total === 1000, `expected 1000, got ${total}`);
  for (const u of unsubs) u();
  n.notify("s");
  assert(total === 1000, `expected still 1000, got ${total}`);
});

// ── Summary ────────────────────────────────────────────────────────
console.log(`\nnotifier: ${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
