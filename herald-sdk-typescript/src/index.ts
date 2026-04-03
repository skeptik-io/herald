export { HeraldClient } from "./client.js";
export { HeraldError, ErrorCode } from "./errors.js";
export type { ConnectionState, StateChangeEvent } from "./connection.js";
export type {
  HeraldClientOptions,
  SubscribedPayload,
  MessageNew,
  MessageAck,
  MessagesBatch,
  MessageDeleted,
  PresenceChanged,
  CursorMoved,
  MemberEvent,
  TypingEvent,
  RoomEvent,
  RoomSubscriberCount,
  EventReceived,
  WatchlistEvent,
  MemberPresence,
  HeraldEvent,
  HeraldEventMap,
} from "./types.js";
