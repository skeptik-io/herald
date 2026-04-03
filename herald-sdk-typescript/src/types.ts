// ---------------------------------------------------------------------------
// Client options
// ---------------------------------------------------------------------------

export interface HeraldClientOptions {
  /** WebSocket URL, e.g. wss://herald.example.com/ws or ws://localhost:6201/ws */
  url: string;
  /** JWT token minted by the app backend */
  token: string;
  /** Called when the server sends system.token_expiring. Return a fresh JWT. */
  onTokenExpiring?: () => Promise<string>;
  /** Reconnect backoff config */
  reconnect?: {
    enabled?: boolean;
    maxDelay?: number;
  };
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
  room: string;
  members: MemberPresence[];
  cursor: number;
  latest_seq: number;
}

export interface MessageNew {
  room: string;
  id: string;
  seq: number;
  sender: string;
  body: string;
  meta?: unknown;
  sent_at: number;
  parent_id?: string;
  edited_at?: number;
}

export interface MessageEdited {
  room: string;
  id: string;
  seq: number;
  body: string;
  edited_at: number;
}

export interface ReactionChanged {
  room: string;
  message_id: string;
  emoji: string;
  user_id: string;
  action: "add" | "remove";
}

export interface MessageAck {
  id: string;
  seq: number;
  sent_at: number;
}

export interface MessagesBatch {
  room: string;
  messages: MessageNew[];
  has_more: boolean;
}

export interface PresenceChanged {
  user_id: string;
  presence: string;
}

export interface CursorMoved {
  room: string;
  user_id: string;
  seq: number;
}

export interface MemberEvent {
  room: string;
  user_id: string;
  role: string;
}

export interface TypingEvent {
  room: string;
  user_id: string;
  active: boolean;
}

export interface RoomEvent {
  room: string;
}

export interface ErrorPayload {
  code: string;
  message: string;
}

export interface EventReceived {
  room: string;
  event: string;
  sender: string;
  data?: unknown;
}

export interface WatchlistEvent {
  user_ids: string[];
}

export interface RoomSubscriberCount {
  room: string;
  count: number;
}

export interface MessageDeleted {
  room: string;
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

export type HeraldEventMap = {
  message: MessageNew;
  "message.deleted": MessageDeleted;
  "message.edited": MessageEdited;
  "reaction.changed": ReactionChanged;
  "event.received": EventReceived;
  presence: PresenceChanged;
  cursor: CursorMoved;
  "member.joined": MemberEvent;
  "member.left": MemberEvent;
  typing: TypingEvent;
  "room.updated": RoomEvent;
  "room.deleted": RoomEvent;
  "room.subscriber_count": RoomSubscriberCount;
  "watchlist.online": WatchlistEvent;
  "watchlist.offline": WatchlistEvent;
  connected: void;
  disconnected: void;
  reconnecting: void;
  error: ErrorPayload;
};

export type HeraldEvent = keyof HeraldEventMap;
