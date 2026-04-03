export interface HealthResponse {
  status: string;
  connections: number;
  rooms: number;
  tenants: number;
  uptime_secs: number;
  storage: boolean;
  cipher: boolean;
  veil: boolean;
  sentry: boolean;
}

export interface Tenant {
  id: string;
  name: string;
  plan?: string;
  config?: Record<string, unknown>;
  created_at: number;
}

export interface Room {
  id: string;
  name: string;
  encryption_mode: string;
  meta?: Record<string, unknown>;
  created_at: number;
}

export interface Member {
  user_id: string;
  role: string;
  presence?: string;
  joined_at: number;
}

export interface Message {
  id: string;
  seq: number;
  sender: string;
  body: string;
  meta?: Record<string, unknown>;
  sent_at: number;
}

export interface MessageList {
  messages: Message[];
  has_more: boolean;
}

export interface PresenceEntry {
  user_id: string;
  status: string;
  last_seen_at?: number;
}

export interface UserPresence {
  user_id: string;
  status: string;
  connections: number;
  last_seen_at: number;
}

export interface Cursor {
  user_id: string;
  seq: number;
}

export interface AdminEvent {
  id: number;
  timestamp: number;
  kind: string;
  tenant_id: string | null;
  details: Record<string, unknown>;
}

export interface ErrorEntry {
  timestamp: number;
  category: "client" | "webhook" | "http";
  message: string;
  details: Record<string, unknown>;
}

export interface StatsSnapshot {
  timestamp: number;
  connections: number;
  peak_connections: number;
  messages_sent: number;
  messages_delta: number;
  webhooks_sent: number;
  webhooks_delta: number;
  rooms: number;
  auth_failures: number;
}

export interface TodaySummary {
  peak_connections: number;
  messages_today: number;
  webhooks_today: number;
}

export interface TenantCurrent {
  connections: number;
  messages_sent: number;
  webhooks_sent: number;
  rooms: number;
}

export interface TenantSnapshot {
  timestamp: number;
  connections: number;
  messages_sent: number;
  messages_delta: number;
  webhooks_sent: number;
  webhooks_delta: number;
  rooms: number;
}

export interface ConnectionInfo {
  total: number;
  by_tenant: { tenant_id: string; connections: number }[];
}

export class HeraldApiError extends Error {
  constructor(
    public status: number,
    public code: string,
    message: string,
  ) {
    super(message);
  }
}

export class HeraldClient {
  constructor(
    private baseUrl: string,
    private token: string,
  ) {}

  private async request<T>(method: string, path: string, body?: unknown): Promise<T> {
    const res = await fetch(`${this.baseUrl}${path}`, {
      method,
      headers: {
        Authorization: `Bearer ${this.token}`,
        ...(body !== undefined ? { "Content-Type": "application/json" } : {}),
      },
      body: body !== undefined ? JSON.stringify(body) : undefined,
    });
    if (!res.ok) {
      let code = "UNKNOWN";
      let message = res.statusText;
      try {
        const err = await res.json();
        code = err.code ?? code;
        message = err.message ?? err.error ?? message;
      } catch {
        /* empty */
      }
      throw new HeraldApiError(res.status, code, message);
    }
    if (res.status === 204) return undefined as T;
    return res.json();
  }

  // Health & Metrics
  health() {
    return this.request<HealthResponse>("GET", "/health");
  }
  metrics() {
    return fetch(`${this.baseUrl}/metrics`).then((r) => r.text());
  }

  // Admin - Tenants
  listTenants() {
    return this.request<{ tenants: Tenant[] }>("GET", "/admin/tenants").then((r) => r.tenants);
  }
  getTenant(id: string) {
    return this.request<Tenant>("GET", `/admin/tenants/${enc(id)}`);
  }
  createTenant(data: { id: string; name: string; jwt_secret: string }) {
    return this.request<Tenant>("POST", "/admin/tenants", data);
  }
  updateTenant(id: string, data: { name?: string; plan?: string }) {
    return this.request<Tenant>("PATCH", `/admin/tenants/${enc(id)}`, data);
  }
  deleteTenant(id: string) {
    return this.request<void>("DELETE", `/admin/tenants/${enc(id)}`);
  }
  listTenantTokens(tenantId: string) {
    return this.request<{ tokens: string[] }>("GET", `/admin/tenants/${enc(tenantId)}/tokens`).then(
      (r) => r.tokens,
    );
  }
  createTenantToken(tenantId: string) {
    return this.request<{ token: string }>("POST", `/admin/tenants/${enc(tenantId)}/tokens`);
  }
  deleteTenantToken(tenantId: string, token: string) {
    return this.request<void>("DELETE", `/admin/tenants/${enc(tenantId)}/tokens/${enc(token)}`);
  }
  listTenantRooms(tenantId: string) {
    return this.request<{ rooms: Room[] }>("GET", `/admin/tenants/${enc(tenantId)}/rooms`).then(
      (r) => r.rooms,
    );
  }

  // Admin - Events, Errors, Stats, Connections
  listEvents(limit?: number, afterId?: number) {
    const params = new URLSearchParams();
    if (limit) params.set("limit", String(limit));
    if (afterId) params.set("after_id", String(afterId));
    const qs = params.toString();
    return this.request<{ events: AdminEvent[] }>("GET", `/admin/events${qs ? `?${qs}` : ""}`).then(
      (r) => r.events,
    );
  }
  listErrors(category?: string, limit?: number) {
    const params = new URLSearchParams();
    if (category) params.set("category", category);
    if (limit) params.set("limit", String(limit));
    return this.request<{ errors: ErrorEntry[] }>("GET", `/admin/errors?${params}`).then(
      (r) => r.errors,
    );
  }
  getAdminStats(from?: number, to?: number) {
    const params = new URLSearchParams();
    if (from) params.set("from", String(from));
    if (to) params.set("to", String(to));
    return this.request<{ today: TodaySummary; snapshots: StatsSnapshot[] }>(
      "GET",
      `/admin/stats?${params}`,
    );
  }
  listConnections() {
    return this.request<ConnectionInfo>("GET", "/admin/connections");
  }

  // Tenant-scoped stats
  getTenantStats(from?: number, to?: number) {
    const params = new URLSearchParams();
    if (from) params.set("from", String(from));
    if (to) params.set("to", String(to));
    return this.request<{ current: TenantCurrent; snapshots: TenantSnapshot[] }>(
      "GET",
      `/stats?${params}`,
    );
  }

  // Tenant-scoped - Rooms
  listRooms() {
    return this.request<{ rooms: Room[] }>("GET", "/rooms").then((r) => r.rooms);
  }
  getRoom(id: string) {
    return this.request<Room>("GET", `/rooms/${enc(id)}`);
  }
  createRoom(data: { id: string; name: string; encryption_mode?: string; meta?: Record<string, unknown> }) {
    return this.request<Room>("POST", "/rooms", data);
  }
  updateRoom(id: string, data: { name?: string; meta?: Record<string, unknown> }) {
    return this.request<void>("PATCH", `/rooms/${enc(id)}`, data);
  }
  deleteRoom(id: string) {
    return this.request<void>("DELETE", `/rooms/${enc(id)}`);
  }

  // Members
  listMembers(roomId: string) {
    return this.request<{ members: Member[] }>("GET", `/rooms/${enc(roomId)}/members`).then(
      (r) => r.members,
    );
  }
  addMember(roomId: string, userId: string, role?: string) {
    return this.request<Member>("POST", `/rooms/${enc(roomId)}/members`, {
      user_id: userId,
      role: role ?? "member",
    });
  }
  updateMember(roomId: string, userId: string, role: string) {
    return this.request<void>("PATCH", `/rooms/${enc(roomId)}/members/${enc(userId)}`, { role });
  }
  removeMember(roomId: string, userId: string) {
    return this.request<void>("DELETE", `/rooms/${enc(roomId)}/members/${enc(userId)}`);
  }

  // Messages
  listMessages(roomId: string, opts?: { before?: number; after?: number; limit?: number }) {
    const params = new URLSearchParams();
    if (opts?.before !== undefined) params.set("before", String(opts.before));
    if (opts?.after !== undefined) params.set("after", String(opts.after));
    if (opts?.limit !== undefined) params.set("limit", String(opts.limit));
    const qs = params.toString();
    return this.request<MessageList>("GET", `/rooms/${enc(roomId)}/messages${qs ? `?${qs}` : ""}`);
  }
  sendMessage(roomId: string, sender: string, body: string, meta?: Record<string, unknown>) {
    return this.request<{ id: string; seq: number }>("POST", `/rooms/${enc(roomId)}/messages`, {
      sender,
      body,
      meta,
    });
  }
  searchMessages(roomId: string, query: string, limit?: number) {
    const params = new URLSearchParams({ q: query });
    if (limit !== undefined) params.set("limit", String(limit));
    return this.request<MessageList>("GET", `/rooms/${enc(roomId)}/messages/search?${params}`);
  }

  // Presence & Cursors
  getRoomPresence(roomId: string) {
    return this.request<{ members: PresenceEntry[] }>("GET", `/rooms/${enc(roomId)}/presence`).then(
      (r) => r.members,
    );
  }
  getUserPresence(userId: string) {
    return this.request<UserPresence>("GET", `/presence/${enc(userId)}`);
  }
  getRoomCursors(roomId: string) {
    return this.request<{ cursors: Cursor[] }>("GET", `/rooms/${enc(roomId)}/cursors`).then(
      (r) => r.cursors,
    );
  }
}

function enc(s: string) {
  return encodeURIComponent(s);
}
