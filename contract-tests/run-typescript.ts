/**
 * TypeScript Admin SDK — Contract Test Runner
 *
 * Reads the shared contract spec and exercises every operation
 * through the herald-admin TypeScript SDK, validating responses.
 */

import { HeraldAdmin } from "herald-admin";
import {
  loadSpec,
  validateExpect,
  resolveInput,
  type TestCase,
} from "./validate.js";

let client: HeraldAdmin;
let passed = 0;
let failed = 0;
const saved: Record<string, unknown> = {};

function initClient(): void {
  const url = process.env.HERALD_URL || "http://127.0.0.1:16300";
  const apiToken = process.env.HERALD_API_TOKEN;
  if (!apiToken) {
    throw new Error("HERALD_API_TOKEN is required — run via run-all.ts or set it manually");
  }
  client = new HeraldAdmin({ url, token: apiToken, tenantId: "default" });
}

async function executeOperation(op: string, input: Record<string, unknown>): Promise<unknown> {
  switch (op) {
    case "health":
      return client.health();

    // Streams
    case "streams.create":
      return client.streams.create(
        input.id as string,
        input.name as string,
        { meta: input.meta, public: input.public as boolean | undefined },
      );
    case "streams.get":
      return client.streams.get(input.id as string);
    case "streams.list":
      return client.streams.list();
    case "streams.update":
      return client.streams.update(input.id as string, {
        name: input.name as string | undefined,
        meta: input.meta,
        archived: input.archived as boolean | undefined,
      });
    case "streams.delete":
      return client.streams.delete(input.id as string);

    // Members
    case "members.add":
      return client.members.add(
        input.stream_id as string,
        input.user_id as string,
        input.role as string | undefined,
      );
    case "members.list":
      return client.members.list(input.stream_id as string);
    case "members.update":
      return client.members.update(
        input.stream_id as string,
        input.user_id as string,
        input.role as string,
      );
    case "members.remove":
      return client.members.remove(
        input.stream_id as string,
        input.user_id as string,
      );

    // Events
    case "events.publish":
      return client.events.publish(
        input.stream_id as string,
        input.sender as string,
        input.body as string,
        {
          meta: input.meta,
          parentId: input.parent_id as string | undefined,
        },
      );
    case "events.list": {
      const result = await client.events.list(input.stream_id as string, {
        before: input.before as number | undefined,
        after: input.after as number | undefined,
        limit: input.limit as number | undefined,
        thread: input.thread as string | undefined,
      });
      return { events: result.events, has_more: result.has_more };
    }
    case "events.edit":
      return client.events.edit(
        input.stream_id as string,
        input.event_id as string,
        input.body as string,
      );
    case "events.delete":
      return client.events.delete(
        input.stream_id as string,
        input.event_id as string,
      );
    case "events.getReactions":
      return client.events.getReactions(
        input.stream_id as string,
        input.event_id as string,
      );
    case "events.trigger":
      return client.events.trigger(
        input.stream_id as string,
        input.event as string,
        input.data,
      );
    // Presence
    case "presence.getUser":
      return client.presence.getUser(input.user_id as string);
    case "presence.getStream":
      return client.presence.getStream(input.stream_id as string);
    case "presence.getCursors":
      return client.presence.getCursors(input.stream_id as string);
    case "presence.getBulk":
      return client.presence.getBulk(input.user_ids as string[]);
    case "presence.setOverride":
      return client.presence.setOverride(input.user_id as string, { status: input.status as string });

    // Blocks
    case "blocks.block":
      return client.chat.blocks.block(
        input.user_id as string,
        input.blocked_id as string,
      );
    case "blocks.unblock":
      return client.chat.blocks.unblock(
        input.user_id as string,
        input.blocked_id as string,
      );
    case "blocks.list":
      return client.chat.blocks.list(input.user_id as string);

    default:
      throw new Error(`Unknown operation: ${op}`);
  }
}

async function runCase(tc: TestCase): Promise<void> {
  const input = tc.input ? resolveInput(tc.input, saved) : {};
  let result: unknown;
  try {
    result = await executeOperation(tc.operation, input);
  } catch (err: any) {
    failed++;
    console.error(`  FAIL [ts] ${tc.id}: ${err.message ?? err}`);
    return;
  }

  if (tc.save && result && typeof result === "object") {
    for (const [alias, field] of Object.entries(tc.save)) {
      saved[alias] = (result as Record<string, unknown>)[field];
    }
  }

  const errors = validateExpect(result, tc.expect, saved);
  if (errors.length > 0) {
    failed++;
    console.error(`  FAIL [ts] ${tc.id}: ${errors.join("; ")}`);
  } else {
    passed++;
    console.log(`  ok [ts] ${tc.id}`);
  }
}

export async function run(): Promise<{ passed: number; failed: number }> {
  initClient();
  const spec = await loadSpec();
  console.log(`\n  TypeScript Admin SDK Contract Tests (${spec.groups.reduce((n, g) => n + g.cases.length, 0)} cases)`);
  console.log("  " + "─".repeat(50));

  for (const group of spec.groups) {
    for (const tc of group.cases) {
      await runCase(tc);
    }
  }

  console.log(`\n  TypeScript: ${passed} passed, ${failed} failed\n`);
  return { passed, failed };
}

// Allow direct execution
if (process.argv[1]?.endsWith("run-typescript.ts")) {
  const { failed: f } = await run();
  process.exit(f > 0 ? 1 : 0);
}
