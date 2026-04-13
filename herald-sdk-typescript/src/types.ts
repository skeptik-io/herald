// ---------------------------------------------------------------------------
// Client options
// ---------------------------------------------------------------------------

export interface HeraldClientOptions {
  /** WebSocket URL, e.g. wss://herald.example.com/ws or ws://localhost:6200/ws */
  url: string;
  /** Tenant public key (from tenant creation) */
  key: string;
  /** HMAC-SHA256 signature over "user_id:sorted_streams:sorted_watchlist" */
  token: string;
  /** User ID for this connection */
  userId: string;
  /** Stream IDs the user is authorized to access */
  streams: string[];
  /** User IDs to watch for presence changes (optional) */
  watchlist?: string[];
  /** Reconnect backoff config */
  reconnect?: {
    enabled?: boolean;
    maxDelay?: number;
  };
  /** Enable E2EE. Events are encrypted/decrypted transparently per-stream. */
  e2ee?: boolean;
  /**
   * Enable at-least-once delivery. When true, the client sends `event.ack`
   * frames to the server with per-stream sequence high-water-marks. On
   * reconnect, the server replays from the last acked sequence instead of
   * using timestamp-based catchup.
   */
  ackMode?: boolean;
  /**
   * Enable timestamp-based catch-up on reconnect. When true (default), the
   * client sends `last_seen_at` on reconnect and the server replays missed
   * events. Set to false when the application uses its own database as the
   * primary message store and seeds history via `seedHistory()`.
   */
  catchUp?: boolean;
}

// ---------------------------------------------------------------------------
// Server → Client payloads
// ---------------------------------------------------------------------------

export interface MemberPresence {
  user_id: string;
  role: string;
  presence: string;
}

export interface SubscribedPayload {
  stream: string;
  members: MemberPresence[];
  cursor: number;
  latest_seq: number;
}

export interface EventNew {
  stream: string;
  id: string;
  seq: number;
  sender: string;
  body: string;
  meta?: unknown;
  sent_at: number;
  parent_id?: string;
  edited_at?: number;
}

export interface EventEdited {
  stream: string;
  id: string;
  seq: number;
  body: string;
  edited_at: number;
}

export interface ReactionChanged {
  stream: string;
  event_id: string;
  emoji: string;
  user_id: string;
  action: "add" | "remove";
}

export interface EventAck {
  id: string;
  seq: number;
  sent_at: number;
}

export interface EventsBatch {
  stream: string;
  events: EventNew[];
  has_more: boolean;
}

export interface PresenceChanged {
  user_id: string;
  presence: string;
  /** ISO 8601 expiry for the override, if set (e.g. "Away until Monday 9am"). */
  until?: string;
}

export interface CursorMoved {
  stream: string;
  user_id: string;
  seq: number;
}

export interface EventDelivered {
  stream: string;
  user_id: string;
  seq: number;
}

export interface MemberEvent {
  stream: string;
  user_id: string;
  role: string;
}

export interface TypingEvent {
  stream: string;
  user_id: string;
  active: boolean;
}

export interface StreamEvent {
  stream: string;
}

export interface ErrorPayload {
  code: string;
  message: string;
}

export interface EventReceived {
  stream: string;
  event: string;
  sender: string;
  data?: unknown;
}

export interface WatchlistEvent {
  user_ids: string[];
}

export interface StreamSubscriberCount {
  stream: string;
  count: number;
}

export interface EventDeleted {
  stream: string;
  id: string;
  seq: number;
}

// ---------------------------------------------------------------------------
// Wire frame
// ---------------------------------------------------------------------------

export interface ServerFrame {
  type: string;
  ref?: string;
  payload?: unknown;
}

/** Fired when a server-pushed catchup batch has drained successfully. */
export interface CatchupComplete {
  stream: string;
  eventsReceived: number;
}

/** Fired when catchup pagination has exhausted its retries and given up.
 *  The client has a partial view of the catchup range — events between
 *  `resumeFrom` and the live edge are missing until the application
 *  either retries fetch manually, reconnects, or rehydrates from its DB. */
export interface CatchupError {
  stream: string;
  error: Error;
  attempts: number;
  eventsReceived: number;
  /** Seq to resume from: `await client.fetch(stream, { after: resumeFrom })`
   *  will continue where catchup left off. */
  resumeFrom: number;
}

export type HeraldEventMap = {
  event: EventNew;
  "event.deleted": EventDeleted;
  "event.edited": EventEdited;
  "reaction.changed": ReactionChanged;
  "event.received": EventReceived;
  "event.delivered": EventDelivered;
  presence: PresenceChanged;
  cursor: CursorMoved;
  "member.joined": MemberEvent;
  "member.left": MemberEvent;
  typing: TypingEvent;
  "stream.updated": StreamEvent;
  "stream.deleted": StreamEvent;
  "stream.subscriber_count": StreamSubscriberCount;
  "watchlist.online": WatchlistEvent;
  "watchlist.offline": WatchlistEvent;
  connected: void;
  disconnected: void;
  reconnecting: void;
  error: ErrorPayload;
  "catchup.complete": CatchupComplete;
  "catchup.error": CatchupError;
};

export type HeraldEvent = keyof HeraldEventMap;
