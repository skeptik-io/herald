export { HeraldAdmin, type HeraldAdminOptions, type EventListOptions, type ErrorListOptions } from "./client.js";
export { HeraldError } from "./errors.js";
export { RoomNamespace } from "./namespaces/rooms.js";
export { MemberNamespace } from "./namespaces/members.js";
export { MessageNamespace } from "./namespaces/messages.js";
export { PresenceNamespace } from "./namespaces/presence.js";
export { TenantNamespace, type Tenant, type CreateTenantOptions, type ApiToken } from "./namespaces/tenants.js";
export { BlockNamespace } from "./namespaces/blocks.js";
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
  ReactionSummary,
  BlockList,
} from "./types.js";
