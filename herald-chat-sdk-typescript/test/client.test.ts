import { HeraldChatClient } from "../src/client.js";
import type { HeraldClient } from "herald-sdk";

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

  sendFrame(frame: Record<string, unknown>): void {
    this.sentFrames.push(frame as SentFrame);
  }
}

function makeClient(): { mock: MockHeraldClient; chat: HeraldChatClient } {
  const mock = new MockHeraldClient();
  const chat = new HeraldChatClient(mock as unknown as HeraldClient);
  return { mock, chat };
}

// ── Typing frames ──────────────────────────────────────────────────

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

await test("start then stop produces two independent frames", async () => {
  const { mock, chat } = makeClient();
  chat.startTyping("s1");
  chat.stopTyping("s1");

  assert(mock.sentFrames.length === 2, "2 frames");
  assert(mock.sentFrames[0].type === "typing.start", "1st is start");
  assert(mock.sentFrames[1].type === "typing.stop", "2nd is stop");
});

await test("typing frames on different streams are isolated", async () => {
  const { mock, chat } = makeClient();
  chat.startTyping("s1");
  chat.startTyping("s2");

  const streams = mock.sentFrames.map((f) => (f.payload as { stream: string }).stream);
  assert(streams[0] === "s1", "1st stream");
  assert(streams[1] === "s2", "2nd stream");
});

await test("typing frame preserves special characters in stream id", async () => {
  const { mock, chat } = makeClient();
  chat.startTyping("stream/with/slashes");
  const p = mock.sentFrames[0].payload as { stream: string };
  assert(p.stream === "stream/with/slashes", "slashes preserved");
});

// ── Summary ────────────────────────────────────────────────────────
console.log(`\nherald-chat-sdk: ${passed} passed, ${failed} failed`);
process.exit(failed > 0 ? 1 : 0);
