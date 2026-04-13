import type { HeraldClient, EventNew,
  PresenceChanged, MemberEvent, TypingEvent,
  EventReceived, EventDelivered } from "herald-sdk";
import type { HeraldChatClient } from "herald-chat-sdk";
import type { HeraldPresenceClient, PresenceStatus } from "herald-presence-sdk";

// ---------------------------------------------------------------------------
// Synthesized ChatEvent payloads
// ---------------------------------------------------------------------------
//
// These shapes are no longer delivered as typed server-to-client frames.
// Chat-core's envelope dispatcher synthesizes them from the typed `body`
// payload inside an incoming `event` so existing middleware consumers can
// continue to subscribe to `"reaction.changed"`, `"event.edited"`, etc.
// without needing to decode envelopes themselves.

export interface EventEdited {
  stream: string;
  id: string;
  seq: number;
  body: string;
  edited_at: number;
}

export interface EventDeleted {
  stream: string;
  id: string;
  seq: number;
}

export interface ReactionChanged {
  stream: string;
  event_id: string;
  emoji: string;
  user_id: string;
  action: "add" | "remove";
}

export interface CursorMoved {
  stream: string;
  user_id: string;
  seq: number;
}

// ---------------------------------------------------------------------------
// Message
// ---------------------------------------------------------------------------

export type MessageStatus = "sending" | "sent" | "delivered" | "failed" | "read";

export interface Message {
  id: string;
  localId?: string;
  seq: number;
  stream: string;
  sender: string;
  body: string;
  meta?: unknown;
  parentId?: string;
  sentAt: number;
  editedAt?: number;
  deleted: boolean;
  status: MessageStatus;
  reactions: Map<string, Set<string>>; // emoji → user_ids
}

export interface PendingMessage {
  localId: string;
  readonly status: MessageStatus;
  retry(): Promise<void>;
  cancel(): void;
}

// ---------------------------------------------------------------------------
// Members
// ---------------------------------------------------------------------------

export interface Member {
  userId: string;
  role: string;
  presence: string;
}

// ---------------------------------------------------------------------------
// Scroll
// ---------------------------------------------------------------------------

export interface ScrollStateSnapshot {
  atLiveEdge: boolean;
  pendingCount: number;
  isLoadingMore: boolean;
}

// ---------------------------------------------------------------------------
// Liveness
// ---------------------------------------------------------------------------

export type LivenessState = "active" | "idle" | "hidden";

export interface LivenessConfig {
  /** Milliseconds before idle → away (default 120_000) */
  idleTimeoutMs?: number;
  /** Throttle activity events (default 5_000) */
  throttleMs?: number;
}

/**
 * Abstraction over DOM APIs so liveness can be tested without a browser.
 */
export interface LivenessEnvironment {
  addEventListener(target: "document" | "window", event: string, handler: () => void): void;
  removeEventListener(target: "document" | "window", event: string, handler: () => void): void;
  getVisibilityState(): "visible" | "hidden";
  setTimeout(fn: () => void, ms: number): number;
  clearTimeout(id: number): void;
}

// ---------------------------------------------------------------------------
// Event middleware
// ---------------------------------------------------------------------------

export type ChatEvent =
  | { type: "event"; data: EventNew }
  | { type: "event.edited"; data: EventEdited }
  | { type: "event.deleted"; data: EventDeleted }
  | { type: "reaction.changed"; data: ReactionChanged }
  | { type: "presence"; data: PresenceChanged }
  | { type: "cursor"; data: CursorMoved }
  | { type: "typing"; data: TypingEvent }
  | { type: "member.joined"; data: MemberEvent }
  | { type: "member.left"; data: MemberEvent }
  | { type: "event.delivered"; data: EventDelivered }
  | { type: "ephemeral"; data: EventReceived };

/**
 * Middleware intercepts events in the ChatCore pipeline. Call `next()` to
 * continue the chain — the store mutation runs at the end of the chain.
 *
 * Code **before** `next()` runs before the store is updated.
 * Code **after** `next()` runs after the store is updated (and after all
 * downstream middleware has completed).
 *
 * ```ts
 * const logger: Middleware = (event, next) => {
 *   console.log("before store:", event.type);
 *   next();
 *   console.log("after store:", event.type);
 * };
 * ```
 *
 * Omitting the `next()` call swallows the event — the store will not be
 * updated and downstream middleware will not run.
 */
export type Middleware = (event: ChatEvent, next: () => void) => void;

// ---------------------------------------------------------------------------
// Writer seam — persist-first integration
// ---------------------------------------------------------------------------

/** Draft passed to `writer.send`. The app's API should treat `localId` as
 *  an idempotency key — if the client retries, the same localId arrives,
 *  and the server should dedupe to avoid double-publishing. */
export interface MessageDraft {
  localId: string;
  streamId: string;
  body: string;
  meta?: unknown;
  parentId?: string;
  senderId: string;
}

/** Canonical message returned by `writer.send`. `id`, `seq`, and `sent_at`
 *  are required so the optimistic entry can be reconciled with the server's
 *  authoritative identifiers. `body` and `meta` are optional — if the app
 *  server canonicalized the input (sanitized, expanded attachment URLs,
 *  resolved mentions), return the canonical values here and the UI will
 *  replace the optimistic version; otherwise the optimistic values persist. */
export interface MessageAck {
  id: string;
  seq: number;
  sent_at: number;
  body?: string;
  meta?: unknown;
}

/** Persist-first write path. Each slot is optional: when provided, the
 *  corresponding ChatCore method routes the write through the app's API
 *  (app DB commits, then app server fans out via Herald); when omitted,
 *  ChatCore falls back to the default Herald WebSocket path.
 *
 *  Intended shape:
 *  - `send/edit/delete`: app API persists to DB, then the app backend
 *    calls Herald's HTTP API to inject the resulting event for fanout.
 *  - `addReaction/removeReaction/updateCursor`: app API commits to DB,
 *    then Herald fanout is triggered (typically by calling the chat-sdk
 *    WS frame — those HTTP endpoints don't exist yet). */
export interface ChatWriter {
  send?: (draft: MessageDraft) => Promise<MessageAck>;
  edit?: (streamId: string, eventId: string, body: string) => Promise<void>;
  delete?: (streamId: string, eventId: string) => Promise<void>;
  addReaction?: (streamId: string, eventId: string, emoji: string) => Promise<void>;
  removeReaction?: (streamId: string, eventId: string, emoji: string) => Promise<void>;
  updateCursor?: (streamId: string, seq: number) => Promise<void>;
}

// ---------------------------------------------------------------------------
// ChatCore options
// ---------------------------------------------------------------------------

export interface ChatCoreOptions {
  client: HeraldClient;
  chat: HeraldChatClient;
  /** Optional presence client for manual overrides. If omitted, liveness
   *  still tracks state locally but doesn't send presence frames. */
  presence?: HeraldPresenceClient;
  /** Current user ID (from JWT). Needed for unread/cursor logic. */
  userId: string;
  /** Omit to disable automatic presence management. */
  liveness?: LivenessConfig;
  /** Delay (ms) before auto-marking read when at live edge. Default 1000. */
  scrollIdleMs?: number;
  /** Number of events to fetch per loadMore() call. Default 50. */
  loadMoreLimit?: number;
  /** Optional middleware chain. Runs before/after store mutations — see {@link Middleware}. */
  middleware?: Middleware[];
  /** Persist-first write path. When a slot is provided, that mutation is
   *  routed through the app's API (which commits to the app DB and then
   *  fans out via Herald) instead of publishing directly over the Herald
   *  WebSocket. Any slot left empty falls back to the Herald WS path. */
  writer?: ChatWriter;
}

export type { PresenceStatus } from "herald-presence-sdk";
