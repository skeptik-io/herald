---
name: Herald vs Herald Chat product separation
description: Chat-specific features (editing, reactions, threads, blocking, attachments) belong in a separate Herald Chat product, not in Herald core transport.
type: project
---

As of 2026-04-03, user decided chat-specific features should NOT go into Herald core. Herald is a generic message transport (pub/sub, notifications, activity feeds, live updates, IoT — not just chat).

**Why:** Adding chat features (editing, reactions, threads, blocking, attachments) would confuse devs into thinking Herald is chat-only. Similar to Pusher's product split (Channels, Beams, etc.), Herald Chat would be a separate product layer on top.

**How to apply:**
- Herald core: rooms, messages, presence, cursors, fan-out, webhooks. Message body is opaque via `meta`.
- Transport-level features stay in Herald: message redaction (compliance), room archival, webhook filtering, API key scoping.
- Chat features deferred to Herald Chat: editing, reactions, threads, read receipts, blocking, attachments, pinning.
- Phase 3 of AUDIT.md is tagged accordingly — items 21, 23-26 marked chat-specific, items 22, 27-29 marked transport.
