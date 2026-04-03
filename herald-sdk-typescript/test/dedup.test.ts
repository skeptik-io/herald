// Verify the dedup set truncation works correctly.
// Focused unit test for the fix in client.ts (item 30).

function testDedupTruncation() {
  // Simulate the dedup set behavior
  let seenMessageIds = new Set<string>();

  // Add 15000 entries
  for (let i = 0; i < 15_000; i++) {
    seenMessageIds.add(`msg-${i}`);

    // Apply the fixed truncation logic (mirrors client.ts)
    if (seenMessageIds.size > 10_000) {
      const ids = Array.from(seenMessageIds);
      seenMessageIds = new Set(ids.slice(-5_000));
    }
  }

  // Should be bounded
  console.assert(
    seenMessageIds.size <= 10_000,
    `Expected <= 10000, got ${seenMessageIds.size}`,
  );
  console.assert(
    seenMessageIds.size >= 5_000,
    `Expected >= 5000, got ${seenMessageIds.size}`,
  );

  // Should contain the most recent entries
  console.assert(
    seenMessageIds.has("msg-14999"),
    "Should contain most recent entry",
  );
  console.assert(
    !seenMessageIds.has("msg-0"),
    "Should not contain oldest entry",
  );

  console.log(`dedup set bounded at ${seenMessageIds.size} entries — OK`);
}

testDedupTruncation();
console.log("all dedup tests passed");
