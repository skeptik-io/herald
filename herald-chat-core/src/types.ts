import type { HeraldClient, EventNew, EventEdited, EventDeleted,
  ReactionChanged, PresenceChanged, CursorMoved, MemberEvent, TypingEvent,
  EventReceived, EventDelivered } from "herald-sdk";
import type { HeraldChatClient } from "herald-chat-sdk";

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
// ChatCore options
// ---------------------------------------------------------------------------

export interface ChatCoreOptions {
  client: HeraldClient;
  chat: HeraldChatClient;
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
}
