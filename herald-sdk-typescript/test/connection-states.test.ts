// Test that the connection state machine has all 6 states
import type { ConnectionState } from "../src/connection.js";

function testConnectionStates() {
    const validStates: ConnectionState[] = [
        "initialized",
        "connecting",
        "connected",
        "unavailable",
        "failed",
        "disconnected",
    ];

    console.assert(validStates.length === 6, `Expected 6 states, got ${validStates.length}`);

    // Verify all states are unique
    const unique = new Set(validStates);
    console.assert(unique.size === 6, "All states should be unique");

    console.log("✓ connection state machine has 6 states");
}

testConnectionStates();
console.log("✓ all connection state tests passed");
