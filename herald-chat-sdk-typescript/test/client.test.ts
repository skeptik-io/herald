import { HeraldChatClient } from "../src/client.js";
import type { HeraldClient, EventAck } from "herald-sdk";

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

// ── Mock HeraldClient ──────────────────────────────────────────────

interface SentFrame {
  type: string;
  ref?: string;
  payload?: unknown;
}

class MockHeraldClient {
  sentFrames: SentFrame[] = [];
  requestedFrames: Array<{ ref: string; frame: SentFrame }> = [];
  private requestResult: unknown = { id: "ack1", seq: 1, sent_at: Date.now() };
  private requestError: Error | null = null;
  private _e2ee: MockE2EE | null = null;

  sendFrame(frame: Record<string, unknown>): void {
    this.sentFrames.push(frame as SentFrame);
  }

  requestFrame(ref: string, frame: Record<string, unknown>): Promise<unknown> {
    this.requestedFrames.push({ ref, frame: frame as SentFrame });
    if (this.requestError) return Promise.reject(this.requestError);
    return Promise.resolve(this.requestResult);
  }

  get e2eeManager(): MockE2EE | null {
    return this._e2ee;
  }

  // Test helpers
  setRequestResult(result: unknown): void { this.requestResult = result; }
  setRequestError(err: Error): void { this.requestError = err; }
  enableE2EE(): void { this._e2ee = new MockE2EE(); }
  reset(): void {
    this.sentFrames = [];
    this.requestedFrames = [];
    this.requestError = null;
  }
}

class MockE2EE {
  encryptCalls: Array<{ stream: string; body: string }> = [];

  encryptOutgoing(stream: string, body: string, _meta?: unknown): { body: string; meta?: unknown } {
    this.encryptCalls.push({ stream, body });
    return { body: `encrypted:${body}` };
  }
}

function makeClient(): { mock: MockHeraldClient; chat: HeraldChatClient } {
  const mock = new MockHeraldClient();
  const chat = new HeraldChatClient(mock as unknown as HeraldClient);
  return { mock, chat };
}

// ── Fire-and-forget frame methods ──────────────────────────────────

await test("updateCursor sends cursor.update frame with correct payload", async () => {
  const { mock, chat } = makeClient();
  chat.updateCursor("stream1", 42);

  assert(mock.sentFrames.length === 1, `expected 1 frame, got ${mock.sentFrames.length}`);
  const f = mock.sentFrames[0];
  assert(f.type === "cursor.update", `type: ${f.type}`);
  const p = f.payload as { stream: string; seq: number };
  assert(p.stream === "stream1", `stream: ${p.stream}`);
  assert(p.seq === 42, `seq: ${p.seq}`);
});

await test("setPresence sends presence.set frame with status", async () => {
  const { mock, chat } = makeClient();

  for (const status of ["online", "away", "dnd"] as const) {
    mock.reset();
    chat.setPresence(status);
    assert(mock.sentFrames.length === 1, `${status}: 1 frame`);
    const p = mock.sentFrames[0].payload as { status: string };
    assert(p.status === status, `${status}: payload.status`);
    assert(mock.sentFrames[0].type === "presence.set", `${status}: type`);
  }
});

await test("startTyping sends typing.start frame", async () => {
  const { mock, chat } = makeClient();
  chat.startTyping("s1");

  assert(mock.sentFrames.length === 1, "1 frame");
  assert(mock.sentFrames[0].type === "typing.start", "type");
  assert((mock.sentFrames[0].payload as { stream: string }).stream === "s1", "stream");
});

await test("stopTyping sends typing.stop frame", async () => {
  const { mock, chat } = makeClient();
  chat.stopTyping("s1");

  assert(mock.sentFrames.length === 1, "1 frame");
  assert(mock.sentFrames[0].type === "typing.stop", "type");
  assert((mock.sentFrames[0].payload as { stream: string }).stream === "s1", "stream");
});

await test("addReaction sends reaction.add frame with correct fields", async () => {
  const { mock, chat } = makeClient();
  chat.addReaction("s1", "evt1", "🔥");

  assert(mock.sentFrames.length === 1, "1 frame");
  assert(mock.sentFrames[0].type === "reaction.add", "type");
  const p = mock.sentFrames[0].payload as { stream: string; event_id: string; emoji: string };
  assert(p.stream === "s1", "stream");
  assert(p.event_id === "evt1", "event_id");
  assert(p.emoji === "🔥", "emoji");
});

await test("removeReaction sends reaction.remove frame", async () => {
  const { mock, chat } = makeClient();
  chat.removeReaction("s1", "evt1", "👍");

  assert(mock.sentFrames.length === 1, "1 frame");
  assert(mock.sentFrames[0].type === "reaction.remove", "type");
  const p = mock.sentFrames[0].payload as { stream: string; event_id: string; emoji: string };
  assert(p.event_id === "evt1", "event_id");
  assert(p.emoji === "👍", "emoji");
});

// ── Request-response methods ───────────────────────────────────────

await test("editEvent sends event.edit request and returns ack", async () => {
  const { mock, chat } = makeClient();
  const expectedAck: EventAck = { id: "e1", seq: 5, sent_at: 12345 };
  mock.setRequestResult(expectedAck);

  const ack = await chat.editEvent("s1", "e1", "new body");

  assert(mock.requestedFrames.length === 1, "1 request");
  const f = mock.requestedFrames[0];
  assert(f.frame.type === "event.edit", "type");
  assert(typeof f.ref === "string" && f.ref.length > 0, "has ref");
  assert(f.frame.ref === f.ref, "frame.ref matches");
  const p = f.frame.payload as { stream: string; id: string; body: string };
  assert(p.stream === "s1", "stream");
  assert(p.id === "e1", "id");
  assert(p.body === "new body", "body (plaintext, no e2ee)");
  assert(ack.id === "e1", "ack.id");
  assert(ack.seq === 5, "ack.seq");
});

await test("editEvent with E2EE encrypts body", async () => {
  const { mock, chat } = makeClient();
  mock.enableE2EE();

  await chat.editEvent("s1", "e1", "secret");

  const p = mock.requestedFrames[0].frame.payload as { body: string };
  assert(p.body === "encrypted:secret", `body should be encrypted, got: ${p.body}`);
  assert(mock.e2eeManager!.encryptCalls.length === 1, "encrypt called once");
  assert(mock.e2eeManager!.encryptCalls[0].stream === "s1", "encrypted for correct stream");
});

await test("editEvent without E2EE sends plaintext", async () => {
  const { mock, chat } = makeClient();
  // e2eeManager is null by default

  await chat.editEvent("s1", "e1", "plain");

  const p = mock.requestedFrames[0].frame.payload as { body: string };
  assert(p.body === "plain", "body is plaintext");
});

await test("deleteEvent sends event.delete request and returns ack", async () => {
  const { mock, chat } = makeClient();
  const expectedAck: EventAck = { id: "e2", seq: 3, sent_at: 99999 };
  mock.setRequestResult(expectedAck);

  const ack = await chat.deleteEvent("s1", "e2");

  assert(mock.requestedFrames.length === 1, "1 request");
  const f = mock.requestedFrames[0];
  assert(f.frame.type === "event.delete", "type");
  const p = f.frame.payload as { stream: string; id: string };
  assert(p.stream === "s1", "stream");
  assert(p.id === "e2", "id");
  assert(ack.id === "e2", "ack.id");
  assert(ack.seq === 3, "ack.seq");
});

// ── Error propagation ──────────────────────────────────────────────

await test("editEvent propagates requestFrame rejection", async () => {
  const { mock, chat } = makeClient();
  mock.setRequestError(new Error("timeout"));

  let threw = false;
  try { await chat.editEvent("s1", "e1", "x"); } catch (e: any) {
    threw = true;
    assert(e.message === "timeout", `error message: ${e.message}`);
  }
  assert(threw, "should throw");
});

await test("deleteEvent propagates requestFrame rejection", async () => {
  const { mock, chat } = makeClient();
  mock.setRequestError(new Error("disconnected"));

  let threw = false;
  try { await chat.deleteEvent("s1", "e1"); } catch (e: any) {
    threw = true;
    assert(e.message === "disconnected", `error message: ${e.message}`);
  }
  assert(threw, "should throw");
});

// ── Ref uniqueness ─────────────────────────────────────────────────

await test("each request gets a unique ref", async () => {
  const { mock, chat } = makeClient();

  await chat.editEvent("s1", "e1", "a");
  await chat.deleteEvent("s1", "e2");
  await chat.editEvent("s1", "e3", "b");

  const refs = mock.requestedFrames.map((r) => r.ref);
  const uniqueRefs = new Set(refs);
  assert(uniqueRefs.size === 3, `expected 3 unique refs, got ${uniqueRefs.size}`);
});

// ── Multiple calls don't interfere ─────────────────────────────────

await test("fire-and-forget calls are independent", async () => {
  const { mock, chat } = makeClient();

  chat.updateCursor("s1", 1);
  chat.setPresence("away");
  chat.startTyping("s2");
  chat.stopTyping("s2");
  chat.addReaction("s1", "e1", "👍");
  chat.removeReaction("s1", "e1", "👍");

  assert(mock.sentFrames.length === 6, `expected 6 frames, got ${mock.sentFrames.length}`);
  const types = mock.sentFrames.map((f) => f.type);
  assert(types[0] === "cursor.update", "1st");
  assert(types[1] === "presence.set", "2nd");
  assert(types[2] === "typing.start", "3rd");
  assert(types[3] === "typing.stop", "4th");
  assert(types[4] === "reaction.add", "5th");
  assert(types[5] === "reaction.remove", "6th");
});

// ── Edge: special characters in stream/event IDs ───────────────────

await test("handles special characters in stream and event IDs", async () => {
  const { mock, chat } = makeClient();

  chat.updateCursor("stream/with/slashes", 1);
  chat.addReaction("stream with spaces", "event:with:colons", "🎉");

  const p1 = mock.sentFrames[0].payload as { stream: string };
  assert(p1.stream === "stream/with/slashes", "slashes preserved");
  const p2 = mock.sentFrames[1].payload as { stream: string; event_id: string };
  assert(p2.stream === "stream with spaces", "spaces preserved");
  assert(p2.event_id === "event:with:colons", "colons preserved");
});

await test("handles empty string body in editEvent", async () => {
  const { mock, chat } = makeClient();
  await chat.editEvent("s1", "e1", "");

  const p = mock.requestedFrames[0].frame.payload as { body: string };
  assert(p.body === "", "empty body passed through");
});

// ── Summary ────────────────────────────────────────────────────────
console.log(`\nherald-chat-sdk: ${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
