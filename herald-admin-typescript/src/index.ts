export { HeraldAdmin, type HeraldAdminOptions, type AdminEventListOptions, type ErrorListOptions } from "./client.js";
export { HeraldError } from "./errors.js";
export { AuditNamespace } from "./namespaces/audit.js";
export { StreamNamespace } from "./namespaces/rooms.js";
export { MemberNamespace } from "./namespaces/members.js";
export { EventNamespace } from "./namespaces/messages.js";
export { PresenceNamespace } from "./namespaces/presence.js";
export { TenantNamespace, type Tenant, type CreateTenantOptions, type ApiToken } from "./namespaces/tenants.js";
export { BlockNamespace } from "./namespaces/blocks.js";
export type {
  Stream,
  Member,
  Event,
  EventList,
  UserPresence,
  MemberPresenceEntry,
  Cursor,
  HealthResponse,
  EventPublishResult,
  ReactionSummary,
  BlockList,
  AuditEvent,
  AuditQueryResponse,
  AuditCountResponse,
  AuditQueryOptions,
  AuditCountOptions,
  ConnectionInfo,
  AdminEventEntry,
  AdminEventList,
  AdminErrorEntry,
  AdminErrorList,
  StatsSnapshot,
  AdminStats,
  TenantStats,
} from "./types.js";
