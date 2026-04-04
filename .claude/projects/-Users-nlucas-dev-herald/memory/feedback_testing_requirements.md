---
name: Testing must be live integration, not stubs
description: User requires all audit items to have integration tests against a running Herald server — no mocks, stubs, or unwired placeholders.
type: feedback
---

All audit items must be verified with integration tests against a live Herald server. No stubs, no placeholders, no code that compiles but isn't wired to anything.

**Why:** User explicitly stated "Testing includes integration tests against a live server to ensure things are not just stubbed/implemented not wired to anything/placeholders." This is a hard requirement, not a preference.

**How to apply:** When implementing any AUDIT.md item, always write the integration test first or alongside the implementation. The test must start a real Herald instance (like the existing `TestServer` harness in `tests/integration.rs`), exercise the feature end-to-end, and assert on observable behavior. Mark items complete only after both code and test pass.
