/**
 * Live integration tests — starts a real Herald server and exercises
 * the TypeScript SDKs against it over actual WebSocket and HTTP.
 */

import { spawn, type ChildProcess } from "node:child_process";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { randomBytes, createHmac } from "node:crypto";
import jwt from "jsonwebtoken";

import { HeraldClient } from "herald-sdk";
import { HeraldChatClient } from "herald-chat-sdk";
import { HeraldAdmin } from "herald-admin";

let passed = 0;
let failed = 0;

function assert(condition: boolean, msg: string): void {
  if (!condition) { failed++; console.error(`  FAIL: ${msg}`); }
}

async function test(name: string, fn: () => Promise<void>): Promise<void> {
  const before = failed;
  try {
    await fn();
    if (failed === before) { passed++; console.log(`  ok - ${name}`); }
    else { console.log(`  FAIL - ${name}`); }
  } catch (e: any) {
    failed++;
    console.error(`  FAIL - ${name}: ${e.message ?? e}`);
  }
}

// ── Server lifecycle ───────────────────────────────────────────────

const WS_PORT = 16200;
const HTTP_PORT = 16201;
const MASTER_KEY = randomBytes(32).toString("hex");
const JWT_SECRET = "integration-test-secret";
const ADMIN_TOKEN = "super-admin-test-token";
const API_TOKEN = "api-test-token-12345678";

let serverProcess: ChildProcess | null = null;
let dataDir: string;

async function startServer(): Promise<void> {
  dataDir = await mkdtemp(join(tmpdir(), "herald-test-"));

  const configPath = join(dataDir, "herald.toml");
  const config = `
[server]
ws_bind = "127.0.0.1:${WS_PORT}"
http_bind = "127.0.0.1:${HTTP_PORT}"
log_level = "error"
shutdown_timeout_secs = 1

[store]
path = "${join(dataDir, "data")}"

[auth]
jwt_secret = "${JWT_SECRET}"
super_admin_token = "${ADMIN_TOKEN}"

[auth.api]
tokens = ["${API_TOKEN}"]

[presence]
linger_secs = 0
manual_override_ttl_secs = 14400
`;
  await writeFile(configPath, config);

  const binaryPath = join(process.cwd(), "..", "target", "release", "herald");

  return new Promise((resolve, reject) => {
    serverProcess = spawn(binaryPath, ["--single-tenant", configPath], {
      env: { ...process.env, SHROUDB_MASTER_KEY: MASTER_KEY },
      stdio: ["ignore", "pipe", "pipe"],
    });

    let stderr = "";
    serverProcess.stderr!.on("data", (chunk: Buffer) => {
      stderr += chunk.toString();
    });

    serverProcess.on("error", (err) => {
      reject(new Error(`Failed to start server: ${err.message}`));
    });

    // Wait for server to be ready by polling health
    const maxAttempts = 40;
    let attempts = 0;
    const check = setInterval(async () => {
      attempts++;
      try {
        const resp = await fetch(`http://127.0.0.1:${HTTP_PORT}/health`);
        if (resp.ok) {
          clearInterval(check);
          resolve();
        }
      } catch {
        if (attempts > maxAttempts) {
          clearInterval(check);
          reject(new Error(`Server failed to start after ${maxAttempts} attempts.\nstderr: ${stderr}`));
        }
      }
    }, 250);
  });
}

async function stopServer(): Promise<void> {
  if (serverProcess) {
    serverProcess.kill("SIGTERM");
    await new Promise<void>((resolve) => {
      serverProcess!.on("exit", () => resolve());
      setTimeout(resolve, 3000);
    });
    serverProcess = null;
  }
  try { await rm(dataDir, { recursive: true, force: true }); } catch {}
}

function mintJwt(userId: string, streams: string[]): string {
  return jwt.sign(
    { sub: userId, tenant: "default", streams, iss: "test" },
    JWT_SECRET,
    { expiresIn: "1h" },
  );
}

function waitForEvent<T>(client: HeraldClient, event: string, timeoutMs = 5000): Promise<T> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`timeout waiting for ${event}`)), timeoutMs);
    client.on(event as any, ((data: T) => {
      clearTimeout(timer);
      resolve(data);
    }) as any);
  });
}

// ── Tests ──────────────────────────────────────────────────────────

async function run(): Promise<void> {
  console.log("Starting Herald server...");
  await startServer();
  console.log("Server started.\n");

  try {
    // ── Setup: create stream and add member via admin HTTP API ────

    const admin = new HeraldAdmin({
      url: `http://127.0.0.1:${HTTP_PORT}`,
      token: API_TOKEN,
    });

    await test("admin: create stream", async () => {
      await admin.streams.create("general", "General");
    });

    await test("admin: add members", async () => {
      await admin.members.add("general", "alice");
      await admin.members.add("general", "bob");
      const members = await admin.members.list("general");
      assert(members.length === 2, `expected 2 members, got ${members.length}`);
    });

    // ── Core SDK: connect, subscribe, publish, receive ───────────

    await test("core: connect and subscribe", async () => {
      const token = mintJwt("alice", ["general"]);
      const client = new HeraldClient({
        url: `ws://127.0.0.1:${HTTP_PORT}/ws`,
        token,
        reconnect: { enabled: false },
      });
      await client.connect();
      assert(client.connected === true, "connected");

      const payloads = await client.subscribe(["general"]);
      assert(payloads.length === 1, "1 payload");
      assert(payloads[0].stream === "general", "stream name");
      assert(payloads[0].members.length === 2, `members: ${payloads[0].members.length}`);

      client.disconnect();
    });

    await test("core: publish and receive event", async () => {
      const aliceToken = mintJwt("alice", ["general"]);
      const bobToken = mintJwt("bob", ["general"]);

      const alice = new HeraldClient({
        url: `ws://127.0.0.1:${HTTP_PORT}/ws`,
        token: aliceToken,
        reconnect: { enabled: false },
      });
      const bob = new HeraldClient({
        url: `ws://127.0.0.1:${HTTP_PORT}/ws`,
        token: bobToken,
        reconnect: { enabled: false },
      });

      await alice.connect();
      await bob.connect();
      await alice.subscribe(["general"]);
      await bob.subscribe(["general"]);

      // Bob listens for events
      const eventPromise = waitForEvent<any>(bob, "event");

      // Alice publishes
      const ack = await alice.publish("general", "Hello from Alice", { meta: { test: true } });
      assert(typeof ack.id === "string" && ack.id.length > 0, "ack has id");
      assert(ack.seq > 0, "ack has seq");

      // Bob receives
      const event = await eventPromise;
      assert(event.body === "Hello from Alice", `body: ${event.body}`);
      assert(event.sender === "alice", `sender: ${event.sender}`);
      assert(event.stream === "general", `stream: ${event.stream}`);
      assert(event.id === ack.id, "same event id");
      assert(event.seq === ack.seq, "same seq");

      alice.disconnect();
      bob.disconnect();
    });

    await test("core: fetch events history", async () => {
      const token = mintJwt("alice", ["general"]);
      const client = new HeraldClient({
        url: `ws://127.0.0.1:${HTTP_PORT}/ws`,
        token,
        reconnect: { enabled: false },
      });
      await client.connect();
      await client.subscribe(["general"]);

      const batch = await client.fetch("general");
      assert(batch.events.length >= 1, `expected >=1 events, got ${batch.events.length}`);
      assert(batch.stream === "general", "stream");

      client.disconnect();
    });

    await test("core: ephemeral trigger", async () => {
      const aliceToken = mintJwt("alice", ["general"]);
      const bobToken = mintJwt("bob", ["general"]);

      const alice = new HeraldClient({
        url: `ws://127.0.0.1:${HTTP_PORT}/ws`,
        token: aliceToken,
        reconnect: { enabled: false },
      });
      const bob = new HeraldClient({
        url: `ws://127.0.0.1:${HTTP_PORT}/ws`,
        token: bobToken,
        reconnect: { enabled: false },
      });

      await alice.connect();
      await bob.connect();
      await alice.subscribe(["general"]);
      await bob.subscribe(["general"]);

      const eventPromise = waitForEvent<any>(bob, "event.received");
      alice.trigger("general", "custom.event", { foo: "bar" });
      const event = await eventPromise;
      assert(event.event === "custom.event", `event: ${event.event}`);
      assert(event.sender === "alice", `sender: ${event.sender}`);

      alice.disconnect();
      bob.disconnect();
    });

    // ── Chat SDK: edit, delete, reactions, cursors, presence, typing ──

    await test("chat: edit event", async () => {
      const aliceToken = mintJwt("alice", ["general"]);
      const bobToken = mintJwt("bob", ["general"]);

      const alice = new HeraldClient({
        url: `ws://127.0.0.1:${HTTP_PORT}/ws`,
        token: aliceToken,
        reconnect: { enabled: false },
      });
      const aliceChat = new HeraldChatClient(alice);
      const bob = new HeraldClient({
        url: `ws://127.0.0.1:${HTTP_PORT}/ws`,
        token: bobToken,
        reconnect: { enabled: false },
      });

      await alice.connect();
      await bob.connect();
      await alice.subscribe(["general"]);
      await bob.subscribe(["general"]);

      const ack = await alice.publish("general", "original body");
      const editedPromise = waitForEvent<any>(bob, "event.edited");
      const editAck = await aliceChat.editEvent("general", ack.id, "edited body");
      assert(editAck.id === ack.id, "edit ack id matches");

      const edited = await editedPromise;
      assert(edited.body === "edited body", `edited body: ${edited.body}`);
      assert(edited.id === ack.id, "edited event id");

      alice.disconnect();
      bob.disconnect();
    });

    await test("chat: delete event", async () => {
      const aliceToken = mintJwt("alice", ["general"]);
      const bobToken = mintJwt("bob", ["general"]);

      const alice = new HeraldClient({
        url: `ws://127.0.0.1:${HTTP_PORT}/ws`,
        token: aliceToken,
        reconnect: { enabled: false },
      });
      const aliceChat = new HeraldChatClient(alice);
      const bob = new HeraldClient({
        url: `ws://127.0.0.1:${HTTP_PORT}/ws`,
        token: bobToken,
        reconnect: { enabled: false },
      });

      await alice.connect();
      await bob.connect();
      await alice.subscribe(["general"]);
      await bob.subscribe(["general"]);

      const ack = await alice.publish("general", "delete me");
      const deletedPromise = waitForEvent<any>(bob, "event.deleted");
      await aliceChat.deleteEvent("general", ack.id);
      const deleted = await deletedPromise;
      assert(deleted.id === ack.id, "deleted event id");

      alice.disconnect();
      bob.disconnect();
    });

    await test("chat: reactions round-trip", async () => {
      const aliceToken = mintJwt("alice", ["general"]);
      const bobToken = mintJwt("bob", ["general"]);

      const alice = new HeraldClient({
        url: `ws://127.0.0.1:${HTTP_PORT}/ws`,
        token: aliceToken,
        reconnect: { enabled: false },
      });
      const aliceChat = new HeraldChatClient(alice);
      const bob = new HeraldClient({
        url: `ws://127.0.0.1:${HTTP_PORT}/ws`,
        token: bobToken,
        reconnect: { enabled: false },
      });

      await alice.connect();
      await bob.connect();
      await alice.subscribe(["general"]);
      await bob.subscribe(["general"]);

      const ack = await alice.publish("general", "react to this");
      const reactionPromise = waitForEvent<any>(bob, "reaction.changed");
      aliceChat.addReaction("general", ack.id, "🔥");
      const reaction = await reactionPromise;
      assert(reaction.event_id === ack.id, `event_id: ${reaction.event_id}`);
      assert(reaction.emoji === "🔥", `emoji: ${reaction.emoji}`);
      assert(reaction.user_id === "alice", `user_id: ${reaction.user_id}`);
      assert(reaction.action === "add", `action: ${reaction.action}`);

      alice.disconnect();
      bob.disconnect();
    });

    await test("chat: typing indicators round-trip", async () => {
      const aliceToken = mintJwt("alice", ["general"]);
      const bobToken = mintJwt("bob", ["general"]);

      const alice = new HeraldClient({
        url: `ws://127.0.0.1:${HTTP_PORT}/ws`,
        token: aliceToken,
        reconnect: { enabled: false },
      });
      const aliceChat = new HeraldChatClient(alice);
      const bob = new HeraldClient({
        url: `ws://127.0.0.1:${HTTP_PORT}/ws`,
        token: bobToken,
        reconnect: { enabled: false },
      });

      await alice.connect();
      await bob.connect();
      await alice.subscribe(["general"]);
      await bob.subscribe(["general"]);

      const typingPromise = waitForEvent<any>(bob, "typing");
      aliceChat.startTyping("general");
      const typing = await typingPromise;
      assert(typing.user_id === "alice", `user_id: ${typing.user_id}`);
      assert(typing.active === true, "active");
      assert(typing.stream === "general", `stream: ${typing.stream}`);

      const stopPromise = waitForEvent<any>(bob, "typing");
      aliceChat.stopTyping("general");
      const stop = await stopPromise;
      assert(stop.active === false, "stopped");

      alice.disconnect();
      bob.disconnect();
    });

    await test("chat: cursor update round-trip", async () => {
      const aliceToken = mintJwt("alice", ["general"]);
      const bobToken = mintJwt("bob", ["general"]);

      const alice = new HeraldClient({
        url: `ws://127.0.0.1:${HTTP_PORT}/ws`,
        token: aliceToken,
        reconnect: { enabled: false },
      });
      const aliceChat = new HeraldChatClient(alice);
      const bob = new HeraldClient({
        url: `ws://127.0.0.1:${HTTP_PORT}/ws`,
        token: bobToken,
        reconnect: { enabled: false },
      });

      await alice.connect();
      await bob.connect();
      await alice.subscribe(["general"]);
      await bob.subscribe(["general"]);

      const cursorPromise = waitForEvent<any>(bob, "cursor");
      aliceChat.updateCursor("general", 5);
      const cursor = await cursorPromise;
      assert(cursor.user_id === "alice", `user_id: ${cursor.user_id}`);
      assert(cursor.seq === 5, `seq: ${cursor.seq}`);
      assert(cursor.stream === "general", `stream: ${cursor.stream}`);

      alice.disconnect();
      bob.disconnect();
    });

    await test("chat: presence round-trip", async () => {
      const aliceToken = mintJwt("alice", ["general"]);
      const bobToken = mintJwt("bob", ["general"]);

      const alice = new HeraldClient({
        url: `ws://127.0.0.1:${HTTP_PORT}/ws`,
        token: aliceToken,
        reconnect: { enabled: false },
      });
      const aliceChat = new HeraldChatClient(alice);
      const bob = new HeraldClient({
        url: `ws://127.0.0.1:${HTTP_PORT}/ws`,
        token: bobToken,
        reconnect: { enabled: false },
      });

      await alice.connect();
      await bob.connect();
      await alice.subscribe(["general"]);
      await bob.subscribe(["general"]);

      const presencePromise = waitForEvent<any>(bob, "presence");
      aliceChat.setPresence("away");
      const presence = await presencePromise;
      assert(presence.user_id === "alice", `user_id: ${presence.user_id}`);
      assert(presence.presence === "away" || presence.presence === "Away", `presence: ${presence.presence}`);

      alice.disconnect();
      bob.disconnect();
    });

    // ── Admin SDK: HTTP API round-trip ────────────────────────────

    await test("admin: list streams", async () => {
      const streams = await admin.streams.list();
      assert(Array.isArray(streams), "is array");
      assert(streams.length >= 1, `expected >=1, got ${streams.length}`);
    });

    await test("admin: publish event via HTTP", async () => {
      const result = await admin.events.publish("general", "server-bot", "Hello from admin");
      assert(typeof result.id === "string", "has id");
      assert(result.seq > 0, "has seq");
    });

    await test("admin: list events", async () => {
      const events = await admin.events.list("general");
      assert(events.events.length >= 1, `expected >=1, got ${events.events.length}`);
    });

    await test("admin: presence query", async () => {
      // Connect a user so they show up in presence
      const token = mintJwt("alice", ["general"]);
      const client = new HeraldClient({
        url: `ws://127.0.0.1:${HTTP_PORT}/ws`,
        token,
        reconnect: { enabled: false },
      });
      await client.connect();
      await client.subscribe(["general"]);

      const members = await admin.presence.getStream("general");
      assert(Array.isArray(members), "is array");
      const alice = members.find((m: any) => m.user_id === "alice");
      assert(alice !== undefined, "alice found");

      client.disconnect();
    });

    await test("admin: block/unblock user", async () => {
      await admin.blocks.block("alice", "bob");
      const blocked = await admin.blocks.list("alice");
      assert(blocked.includes("bob"), "bob is blocked");

      await admin.blocks.unblock("alice", "bob");
      const after = await admin.blocks.list("alice");
      assert(!after.includes("bob"), "bob unblocked");
    });

    await test("admin: chat namespace grouping works", async () => {
      // admin.chat.presence and admin.chat.blocks should be the same instances
      assert(admin.chat.presence === admin.presence, "chat.presence === presence");
      assert(admin.chat.blocks === admin.blocks, "chat.blocks === blocks");
    });

    // ── Error cases ──────────────────────────────────────────────

    await test("core: subscribe to unauthorized stream errors", async () => {
      const token = mintJwt("alice", ["general"]);
      const client = new HeraldClient({
        url: `ws://127.0.0.1:${HTTP_PORT}/ws`,
        token,
        reconnect: { enabled: false },
      });
      await client.connect();

      let errorReceived = false;
      client.on("error", () => { errorReceived = true; });
      // "secret" stream is not in the JWT claims
      try {
        await client.subscribe(["secret"]);
      } catch {
        errorReceived = true;
      }
      // Give time for error event
      await new Promise((r) => setTimeout(r, 200));
      assert(errorReceived, "error for unauthorized stream");

      client.disconnect();
    });

    await test("chat: edit someone else's event succeeds without Sentry (fail-open)", async () => {
      // Without Sentry configured, authorize() returns Ok (fail-open).
      // Bob CAN edit Alice's message. This test documents that behavior.
      const aliceToken = mintJwt("alice", ["general"]);
      const bobToken = mintJwt("bob", ["general"]);

      const alice = new HeraldClient({
        url: `ws://127.0.0.1:${HTTP_PORT}/ws`,
        token: aliceToken,
        reconnect: { enabled: false },
      });
      const bob = new HeraldClient({
        url: `ws://127.0.0.1:${HTTP_PORT}/ws`,
        token: bobToken,
        reconnect: { enabled: false },
      });
      const bobChat = new HeraldChatClient(bob);

      await alice.connect();
      await bob.connect();
      await alice.subscribe(["general"]);
      await bob.subscribe(["general"]);

      const ack = await alice.publish("general", "alice's message");

      // Bob edits alice's message — should succeed (Sentry not configured = fail-open)
      const editedPromise = waitForEvent<any>(alice, "event.edited");
      const editAck = await bobChat.editEvent("general", ack.id, "bob edited this");
      assert(editAck.id === ack.id, "edit ack id");

      const edited = await editedPromise;
      assert(edited.body === "bob edited this", `edited body: ${edited.body}`);

      alice.disconnect();
      bob.disconnect();
    });

  } finally {
    console.log("\nStopping server...");
    await stopServer();
  }

  console.log(`\n${"=".repeat(50)}`);
  console.log(`integration: ${passed} passed, ${failed} failed`);
  if (failed > 0) process.exit(1);
}

run().catch((e) => {
  console.error("Fatal:", e);
  stopServer().then(() => process.exit(1));
});
