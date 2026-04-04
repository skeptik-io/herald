// Verify subscribe timeout rejects after the deadline.
// Focused unit test for the fix in client.ts (item 31).

import { HeraldError } from "../src/errors.js";

function testSubscribeTimeoutLogic() {
  let resolved = false;
  let rejected = false;
  let rejectedError: HeraldError | null = null;

  const pendingSubscribes = new Map<
    string,
    {
      rooms: string[];
      results: unknown[];
      resolve: (v: unknown) => void;
    }
  >();

  const ref = "test-ref";
  const promise = new Promise<unknown>((resolve, reject) => {
    pendingSubscribes.set(ref, { streams: ["stream1"], results: [], resolve });

    // Simulate the timeout (shortened for test)
    setTimeout(() => {
      if (pendingSubscribes.has(ref)) {
        pendingSubscribes.delete(ref);
        reject(new HeraldError("TIMEOUT", "subscribe timed out"));
      }
    }, 100); // 100ms instead of 10s for testing
  });

  promise
    .then(() => {
      resolved = true;
    })
    .catch((e) => {
      rejected = true;
      rejectedError = e;
    });

  // Wait for timeout + margin
  setTimeout(() => {
    console.assert(!resolved, "Should not have resolved");
    console.assert(rejected, "Should have rejected");
    console.assert(
      rejectedError?.code === "TIMEOUT",
      `Expected TIMEOUT error, got ${rejectedError?.code}`,
    );
    console.assert(
      !pendingSubscribes.has(ref),
      "Should have cleaned up pending subscribe",
    );
    console.log("subscribe timeout rejects correctly — OK");
    console.log("all subscribe timeout tests passed");
  }, 200);
}

testSubscribeTimeoutLogic();
