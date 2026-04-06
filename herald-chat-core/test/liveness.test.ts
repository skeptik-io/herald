import type { LivenessEnvironment, LivenessState } from "../src/types.js";
import { LivenessController } from "../src/liveness/liveness.js";

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

// ── Mock environment ───────────────────────────────────────────────

class MockEnv implements LivenessEnvironment {
  private listeners = new Map<string, Set<() => void>>();
  private timers = new Map<number, { fn: () => void; ms: number }>();
  private nextId = 1;
  visibilityState: "visible" | "hidden" = "visible";

  addEventListener(target: "document" | "window", event: string, handler: () => void): void {
    const key = `${target}:${event}`;
    if (!this.listeners.has(key)) this.listeners.set(key, new Set());
    this.listeners.get(key)!.add(handler);
  }

  removeEventListener(target: "document" | "window", event: string, handler: () => void): void {
    this.listeners.get(`${target}:${event}`)?.delete(handler);
  }

  getVisibilityState(): "visible" | "hidden" { return this.visibilityState; }

  setTimeout(fn: () => void, ms: number): number {
    const id = this.nextId++;
    this.timers.set(id, { fn, ms });
    return id;
  }

  clearTimeout(id: number): void { this.timers.delete(id); }

  // ── Helpers ──
  fire(target: string, event: string): void {
    for (const fn of this.listeners.get(`${target}:${event}`) ?? []) fn();
  }

  fireAllTimers(): void {
    const entries = Array.from(this.timers.entries());
    this.timers.clear();
    for (const [, { fn }] of entries) fn();
  }

  get timerCount(): number { return this.timers.size; }

  get listenerCount(): number {
    let n = 0;
    for (const s of this.listeners.values()) n += s.size;
    return n;
  }
}

function makeLiveness(overrides?: {
  idleTimeoutMs?: number;
  throttleMs?: number;
}): { env: MockEnv; lc: LivenessController; states: LivenessState[] } {
  const env = new MockEnv();
  const states: LivenessState[] = [];
  const lc = new LivenessController(env, {
    idleTimeoutMs: overrides?.idleTimeoutMs ?? 100,
    throttleMs: overrides?.throttleMs ?? 0,
  }, (s) => states.push(s));
  return { env, lc, states };
}

// ── Initial state ──────────────────────────────────────────────────

test("initial state is active", () => {
  const { lc } = makeLiveness();
  assert(lc.getState() === "active", "should be active");
});

test("no transitions before attach", () => {
  const { lc, states } = makeLiveness();
  assert(states.length === 0, "no transitions");
  assert(lc.getState() === "active", "active");
});

// ── Idle timeout ──���────────────────────────────────────────────────

test("transitions to idle after timeout", () => {
  const { env, lc, states } = makeLiveness();
  lc.attach();
  env.fireAllTimers();
  assert(lc.getState() === "idle", "idle");
  assert(states.length === 1 && states[0] === "idle", "one idle transition");
  lc.detach();
});

test("activity resets idle timer and goes back to active", () => {
  const { env, lc, states } = makeLiveness();
  lc.attach();

  // Go idle
  env.fireAllTimers();
  assert(lc.getState() === "idle", "idle first");

  // Activity
  env.fire("document", "mousemove");
  assert(lc.getState() === "active", "active after mousemove");
  assert(states[1] === "active", "transition to active");

  // New idle timer should be set
  assert(env.timerCount === 1, "new timer set");
  lc.detach();
});

test("keydown resets idle", () => {
  const { env, lc } = makeLiveness();
  lc.attach();
  env.fireAllTimers(); // go idle
  env.fire("document", "keydown");
  assert(lc.getState() === "active", "active after keydown");
  lc.detach();
});

test("touchstart resets idle", () => {
  const { env, lc } = makeLiveness();
  lc.attach();
  env.fireAllTimers();
  env.fire("document", "touchstart");
  assert(lc.getState() === "active", "active after touchstart");
  lc.detach();
});

test("scroll resets idle", () => {
  const { env, lc } = makeLiveness();
  lc.attach();
  env.fireAllTimers();
  env.fire("document", "scroll");
  assert(lc.getState() === "active", "active after scroll");
  lc.detach();
});

test("mousedown resets idle", () => {
  const { env, lc } = makeLiveness();
  lc.attach();
  env.fireAllTimers();
  env.fire("document", "mousedown");
  assert(lc.getState() === "active", "active after mousedown");
  lc.detach();
});

// ── Throttling ─────────────────────────────────────────────────────

test("activity events are throttled by throttleMs", () => {
  // Throttle uses real Date.now(), so we test that the throttle mechanism
  // exists by verifying that with a very large throttle window, rapid
  // activity after the initial activity is suppressed.
  //
  // Since Date.now() doesn't meaningfully advance within a synchronous test,
  // we verify: with throttleMs=0, every activity event resets the idle timer
  // (confirmed by other tests). With a large throttleMs, activity within the
  // window is suppressed.
  const { env, lc, states } = makeLiveness({ throttleMs: 999_999 });
  lc.attach();

  // lastActivity is set to Date.now() at attach time.
  // Any activity within 999999ms of that is throttled.
  env.fireAllTimers(); // go idle
  assert(lc.getState() === "idle", "idle");

  // Activity is throttled because Date.now() - lastActivity < 999999ms
  env.fire("document", "mousemove");
  assert(lc.getState() === "idle", "still idle (throttled)");

  // Verify: with throttleMs=0, same scenario transitions to active
  const { env: env2, lc: lc2 } = makeLiveness({ throttleMs: 0 });
  lc2.attach();
  env2.fireAllTimers();
  assert(lc2.getState() === "idle", "idle");
  env2.fire("document", "mousemove");
  assert(lc2.getState() === "active", "active with throttle=0");

  lc.detach();
  lc2.detach();
});

// ── Visibility ─────────────────────────────────────────────────────

test("hidden tab transitions to hidden", () => {
  const { env, lc, states } = makeLiveness();
  lc.attach();
  env.visibilityState = "hidden";
  env.fire("document", "visibilitychange");
  assert(lc.getState() === "hidden", "hidden");
  assert(states[0] === "hidden", "transition to hidden");
  lc.detach();
});

test("visible again transitions to active", () => {
  const { env, lc, states } = makeLiveness();
  lc.attach();
  env.visibilityState = "hidden";
  env.fire("document", "visibilitychange");
  env.visibilityState = "visible";
  env.fire("document", "visibilitychange");
  assert(lc.getState() === "active", "active after visible");
  assert(states[states.length - 1] === "active", "last transition is active");
  lc.detach();
});

test("hidden clears idle timer", () => {
  const { env, lc } = makeLiveness();
  lc.attach();
  assert(env.timerCount === 1, "idle timer set");
  env.visibilityState = "hidden";
  env.fire("document", "visibilitychange");
  assert(env.timerCount === 0, "timer cleared on hidden");
  lc.detach();
});

test("visible resets idle timer", () => {
  const { env, lc } = makeLiveness();
  lc.attach();
  env.visibilityState = "hidden";
  env.fire("document", "visibilitychange");
  assert(env.timerCount === 0, "no timers when hidden");
  env.visibilityState = "visible";
  env.fire("document", "visibilitychange");
  assert(env.timerCount === 1, "new timer on visible");
  lc.detach();
});

test("hidden from idle state goes to hidden", () => {
  const { env, lc, states } = makeLiveness();
  lc.attach();
  env.fireAllTimers(); // idle
  env.visibilityState = "hidden";
  env.fire("document", "visibilitychange");
  assert(lc.getState() === "hidden", "hidden from idle");
  lc.detach();
});

test("visible from hidden→idle goes to active (not idle)", () => {
  const { env, lc } = makeLiveness();
  lc.attach();
  env.fireAllTimers(); // idle
  env.visibilityState = "hidden";
  env.fire("document", "visibilitychange");
  env.visibilityState = "visible";
  env.fire("document", "visibilitychange");
  assert(lc.getState() === "active", "active, not idle");
  lc.detach();
});

// ── No duplicate transitions ───────────────────────────────────────

test("repeated hidden does not fire duplicate transitions", () => {
  const { env, lc, states } = makeLiveness();
  lc.attach();
  env.visibilityState = "hidden";
  env.fire("document", "visibilitychange");
  env.fire("document", "visibilitychange");
  assert(states.length === 1, `expected 1, got ${states.length}`);
  lc.detach();
});

test("activity while already active does not fire transition", () => {
  const { env, lc, states } = makeLiveness({ throttleMs: 0 });
  lc.attach();
  env.fire("document", "mousemove");
  env.fire("document", "keydown");
  assert(states.length === 0, "no transitions while active");
  lc.detach();
});

// ── Detach ─────────────────────────────────────────────────────────

test("detach removes all listeners and timers", () => {
  const { env, lc } = makeLiveness();
  lc.attach();
  assert(env.listenerCount > 0, "listeners attached");
  assert(env.timerCount > 0, "timers set");
  lc.detach();
  assert(env.timerCount === 0, "timers cleared");
  // Listeners removed — fire should be harmless
  env.fire("document", "mousemove");
  env.visibilityState = "hidden";
  env.fire("document", "visibilitychange");
  assert(lc.getState() === "active", "state frozen after detach");
});

test("double attach is idempotent", () => {
  const { env, lc } = makeLiveness();
  lc.attach();
  const timersBefore = env.timerCount;
  lc.attach(); // second attach
  assert(env.timerCount === timersBefore, "no extra timers");
  lc.detach();
});

test("double detach is safe", () => {
  const { lc } = makeLiveness();
  lc.attach();
  lc.detach();
  lc.detach(); // should not throw
});

test("attach after detach works", () => {
  const { env, lc, states } = makeLiveness();
  lc.attach();
  lc.detach();
  lc.attach();
  env.fireAllTimers();
  assert(lc.getState() === "idle", "idle after re-attach");
  lc.detach();
});

// ── Rapid transitions ──────────────────────────────────────────────

test("rapid hidden-visible-hidden-visible", () => {
  const { env, lc, states } = makeLiveness();
  lc.attach();
  for (let i = 0; i < 10; i++) {
    env.visibilityState = "hidden";
    env.fire("document", "visibilitychange");
    env.visibilityState = "visible";
    env.fire("document", "visibilitychange");
  }
  assert(lc.getState() === "active", "ends active");
  // Each cycle: hidden + active = 2 transitions, 10 cycles = 20
  assert(states.length === 20, `expected 20 transitions, got ${states.length}`);
  lc.detach();
});

// ── Summary ────────────────────────────────────────────────────────
console.log(`\nliveness: ${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
