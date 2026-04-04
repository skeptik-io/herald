export { HeraldClient } from "./client.js";
export { HeraldError, ErrorCode } from "./errors.js";
export {
  initE2EE,
  generateKeyPair,
  deriveSharedSecret,
  createSession,
  restoreSession,
} from "./crypto.js";
export type { ConnectionState, StateChangeEvent } from "./connection.js";
export type { E2EEKeyPair, E2EESession } from "./crypto.js";
export type {
  HeraldClientOptions,
  SubscribedPayload,
  MessageNew,
  MessageAck,
  MessagesBatch,
  MessageDeleted,
  MessageEdited,
  ReactionChanged,
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
