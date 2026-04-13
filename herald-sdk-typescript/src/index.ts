export { HeraldClient, nextRef } from "./client.js";
export { HeraldError, ErrorCode } from "./errors.js";
export type { ConnectionState, StateChangeEvent } from "./connection.js";
export type {
  HeraldClientOptions,
  SubscribedPayload,
  EventNew,
  EventAck,
  EventsBatch,
  EventDeleted,
  EventEdited,
  ReactionChanged,
  PresenceChanged,
  CursorMoved,
  MemberEvent,
  TypingEvent,
  StreamEvent,
  StreamSubscriberCount,
  EventReceived,
  EventDelivered,
  WatchlistEvent,
  MemberPresence,
  CatchupComplete,
  CatchupError,
  HeraldEvent,
  HeraldEventMap,
} from "./types.js";
