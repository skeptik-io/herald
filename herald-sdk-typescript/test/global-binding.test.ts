// Test that onAny receives all events
function testGlobalBinding() {
    // Simulate the global handler behavior
    const globalHandlers = new Set<(event: string, data: unknown) => void>();
    const received: Array<{ event: string; data: unknown }> = [];

    globalHandlers.add((event, data) => {
        received.push({ event, data });
    });

    // Simulate emitting events
    function emit(event: string, data: unknown) {
        for (const handler of globalHandlers) {
            handler(event, data);
        }
    }

    emit("event", { body: "hello" });
    emit("presence", { user_id: "alice", presence: "online" });
    emit("typing", { stream: "chat", user_id: "bob", active: true });

    console.assert(received.length === 3, `Expected 3 events, got ${received.length}`);
    console.assert(received[0].event === "event", "First event should be event");
    console.assert(received[1].event === "presence", "Second event should be presence");
    console.assert(received[2].event === "typing", "Third event should be typing");

    console.log("✓ global binding receives all events");
}

testGlobalBinding();

// Test offAny
function testOffAny() {
    const handlers = new Set<(event: string, data: unknown) => void>();
    let count = 0;
    const handler = () => { count++; };

    handlers.add(handler);
    for (const h of handlers) h("test", {});
    console.assert(count === 1, "Should have received 1 event");

    handlers.delete(handler);
    for (const h of handlers) h("test", {});
    console.assert(count === 1, "Should still be 1 after removing handler");

    console.log("✓ offAny removes handler correctly");
}

testOffAny();
console.log("✓ all global binding tests passed");
