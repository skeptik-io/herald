export { HeraldAdmin, type HeraldAdminOptions } from "./client.js";
export { HeraldError } from "./errors.js";
export { RoomNamespace } from "./namespaces/rooms.js";
export { MemberNamespace } from "./namespaces/members.js";
export { MessageNamespace } from "./namespaces/messages.js";
export { PresenceNamespace } from "./namespaces/presence.js";
export type {
  Room,
  Member,
  Message,
  MessageList,
  UserPresence,
  MemberPresenceEntry,
  Cursor,
  HealthResponse,
  MessageSendResult,
} from "./types.js";
