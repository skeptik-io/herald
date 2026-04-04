---
name: Herald production audit initiative
description: 42-item audit tracked in AUDIT.md — security, hardening, features, SDKs. All items must be implemented + integration tested against live server before marking complete.
type: project
---

Full production audit of Herald completed 2026-04-03. Tracked in `/AUDIT.md` at project root.

**Why:** Herald is not production-ready. Critical security gaps (timing attacks, plaintext leaks to webhooks, no rate limiting), correctness issues (silent message loss, presence race conditions), missing hardening (no pagination, no request tracing), incomplete features (no edit/delete, no threads, no reactions), and SDK gaps (no tenant management, zero tests, bugs).

**How to apply:** Every implementation session should reference AUDIT.md. Items are only complete when both code AND integration tests (against live Herald, not mocks) are passing. Phase 1 blocks deployment. Update the progress table as items complete.
