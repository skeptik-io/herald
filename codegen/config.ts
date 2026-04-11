// Configuration for mapping OpenAPI operations to SDK namespaces/methods.

/** Maps an OpenAPI operation (by path + method) to an SDK namespace and method. */
export interface OperationMapping {
  namespace: string;
  method: string;
  /** If set, response is unwrapped by extracting this key from the JSON body. */
  unwrapKey?: string;
  /** TS element type when unwrapping arrays. */
  unwrapElementType?: string;
}

/**
 * Operation map keyed by "HTTP_METHOD /path" to avoid operationId collisions.
 * Self-service (/self/*), liveness, readiness, and metrics are excluded.
 */
export const OPERATION_MAP: Record<string, OperationMapping> = {
  // --- Streams ---
  "POST /streams": {
    namespace: "streams",
    method: "create",
  },
  "GET /streams": {
    namespace: "streams",
    method: "list",
    unwrapKey: "streams",
    unwrapElementType: "Stream",
  },
  "GET /streams/{id}": {
    namespace: "streams",
    method: "get",
  },
  "PATCH /streams/{id}": {
    namespace: "streams",
    method: "update",
  },
  "DELETE /streams/{id}": {
    namespace: "streams",
    method: "delete",
  },

  // --- Members ---
  "POST /streams/{id}/members": {
    namespace: "members",
    method: "add",
  },
  "GET /streams/{id}/members": {
    namespace: "members",
    method: "list",
    unwrapKey: "members",
    unwrapElementType: "Member",
  },
  "PATCH /streams/{id}/members/{user_id}": {
    namespace: "members",
    method: "update",
  },
  "DELETE /streams/{id}/members/{user_id}": {
    namespace: "members",
    method: "remove",
  },

  // --- Events ---
  "POST /streams/{id}/events": {
    namespace: "events",
    method: "publish",
  },
  "GET /streams/{id}/events": {
    namespace: "events",
    method: "list",
  },
  "PATCH /streams/{id}/events/{event_id}": {
    namespace: "events",
    method: "edit",
  },
  "DELETE /streams/{id}/events/{event_id}": {
    namespace: "events",
    method: "delete",
  },
  "GET /streams/{id}/events/{event_id}/reactions": {
    namespace: "events",
    method: "getReactions",
    unwrapKey: "reactions",
    unwrapElementType: "ReactionSummary",
  },
  "POST /streams/{id}/trigger": {
    namespace: "events",
    method: "trigger",
  },

  // --- Presence ---
  "GET /presence/{user_id}": {
    namespace: "presence",
    method: "getUser",
  },
  "GET /presence": {
    namespace: "presence",
    method: "getBulk",
    unwrapKey: "users",
    unwrapElementType: "UserPresence",
  },
  "POST /presence/{user_id}": {
    namespace: "presence",
    method: "setOverride",
  },
  "GET /streams/{id}/presence": {
    namespace: "presence",
    method: "getStream",
    unwrapKey: "members",
    unwrapElementType: "MemberPresenceEntry",
  },

  // --- Cursors ---
  "GET /streams/{id}/cursors": {
    namespace: "presence",
    method: "getCursors",
    unwrapKey: "cursors",
    unwrapElementType: "Cursor",
  },

  // --- Blocks (chat) ---
  "POST /blocks": {
    namespace: "blocks",
    method: "block",
  },
  "DELETE /blocks": {
    namespace: "blocks",
    method: "unblock",
  },
  "GET /blocks/{user_id}": {
    namespace: "blocks",
    method: "list",
    unwrapKey: "blocked",
  },

  // --- Tenants (admin) ---
  "POST /admin/tenants": {
    namespace: "tenants",
    method: "create",
  },
  "GET /admin/tenants": {
    namespace: "tenants",
    method: "list",
    unwrapKey: "tenants",
    unwrapElementType: "Tenant",
  },
  "GET /admin/tenants/{id}": {
    namespace: "tenants",
    method: "get",
  },
  "PATCH /admin/tenants/{id}": {
    namespace: "tenants",
    method: "update",
  },
  "DELETE /admin/tenants/{id}": {
    namespace: "tenants",
    method: "delete",
  },
  "POST /admin/tenants/{id}/tokens": {
    namespace: "tenants",
    method: "createToken",
  },
  "GET /admin/tenants/{id}/tokens": {
    namespace: "tenants",
    method: "listTokens",
    unwrapKey: "tokens",
    unwrapElementType: "ApiToken",
  },
  "DELETE /admin/tenants/{id}/tokens/{token}": {
    namespace: "tenants",
    method: "deleteToken",
  },
  "GET /admin/tenants/{id}/streams": {
    namespace: "tenants",
    method: "listTenantStreams",
    unwrapKey: "streams",
    unwrapElementType: "Stream",
  },
  "DELETE /admin/tenants/{id}/data": {
    namespace: "tenants",
    method: "purgeData",
  },

  // --- Audit (admin) ---
  "GET /admin/tenants/{id}/audit": {
    namespace: "audit",
    method: "query",
  },
  "GET /admin/tenants/{id}/audit/count": {
    namespace: "audit",
    method: "count",
  },

  // --- Root methods ---
  "GET /health": {
    namespace: "_root",
    method: "health",
  },
  "GET /admin/connections": {
    namespace: "_root",
    method: "connections",
  },
  "GET /admin/events": {
    namespace: "_root",
    method: "adminEvents",
  },
  "GET /admin/errors": {
    namespace: "_root",
    method: "errors",
  },
  "GET /admin/stats": {
    namespace: "_root",
    method: "stats",
  },
  "GET /stats": {
    namespace: "_root",
    method: "tenantStats",
  },
};

/** Namespaces nested under client.chat. */
export const CHAT_NAMESPACES = new Set(["blocks"]);

/** Namespace ID → class name. */
export const NAMESPACE_CLASSES: Record<string, string> = {
  streams: "StreamNamespace",
  members: "MemberNamespace",
  events: "EventNamespace",
  tenants: "TenantNamespace",
  audit: "AuditNamespace",
  presence: "PresenceNamespace",
  blocks: "BlockNamespace",
};

/** Paths that should be excluded from SDK generation. */
export const EXCLUDED_PATH_PREFIXES = [
  "/self/",
  "/health/live",
  "/health/ready",
  "/metrics",
  "/admin/events/stream",
  "/openapi.json",
];

/**
 * Paths where DELETE uses a request body instead of path params.
 * Most DELETE operations use path params, but blocks and events-purge are exceptions.
 */
export const DELETE_WITH_BODY = new Set([
  "DELETE /blocks",
  "DELETE /streams/{id}/events",
]);
