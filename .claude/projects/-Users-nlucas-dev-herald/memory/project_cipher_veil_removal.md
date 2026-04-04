---
name: Cipher and Veil removal from Herald
description: User is removing Cipher (encryption) and Veil (encrypted search) from Herald — encryption is a client concern. Audit items need re-scoping after removal.
type: project
---

As of 2026-04-03, user decided to remove Cipher and Veil integrations from Herald. Encryption and encrypted search are now client-side concerns, not message transport concerns.

**Why:** Encryption belongs at the client layer, not in the chat server. Herald is a message transport.

**How to apply:** After removal is complete, re-scope AUDIT.md:
- Item 2 (plaintext leak to webhooks) — already implemented but may need reverting/simplifying
- Item 9 (circuit breaker half-open) — re-scope to remaining integrations (Sentry, Courier, Chronicle)
- Phase 3 Item 32 (E2E encryption) — remove or reframe as client SDK concern
- Remove all audit findings about zeroization, Veil blind index leaks, mock cipher in prod
- Remove `encryption_mode` from rooms or make it informational
- Phase 1 items 1, 3, 4 are already implemented and committed; items 5-12 are paused waiting for removal

User will signal when removal is complete before we resume.
