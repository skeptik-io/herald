import { HttpTransport } from "./transport.js";
import { MemberNamespace } from "./namespaces/members.js";
import { MessageNamespace } from "./namespaces/messages.js";
import { PresenceNamespace } from "./namespaces/presence.js";
import { RoomNamespace } from "./namespaces/rooms.js";
import { TenantNamespace } from "./namespaces/tenants.js";
import { BlockNamespace } from "./namespaces/blocks.js";
import type { HealthResponse } from "./types.js";

export interface HeraldAdminOptions {
  /** Herald HTTP API URL, e.g. http://localhost:6201 */
  url: string;
  /** API bearer token (must match auth.api.tokens in herald.toml) */
  token: string;
}

export interface EventListOptions {
  limit?: number;
}

export interface ErrorListOptions {
  limit?: number;
  category?: string;
}

/**
 * Herald HTTP admin client for backend services.
 *
 * Provides namespaced access to Herald's HTTP API for room management,
 * member management, message injection, and presence queries.
 */
export class HeraldAdmin {
  public readonly rooms: RoomNamespace;
  public readonly members: MemberNamespace;
  public readonly messages: MessageNamespace;
  public readonly presence: PresenceNamespace;
  public readonly tenants: TenantNamespace;
  public readonly blocks: BlockNamespace;

  private transport: HttpTransport;

  constructor(options: HeraldAdminOptions) {
    this.transport = new HttpTransport(options.url, options.token);
    this.rooms = new RoomNamespace(this.transport);
    this.members = new MemberNamespace(this.transport);
    this.messages = new MessageNamespace(this.transport);
    this.presence = new PresenceNamespace(this.transport);
    this.tenants = new TenantNamespace(this.transport);
    this.blocks = new BlockNamespace(this.transport);
  }

  async health(): Promise<HealthResponse> {
    return this.transport.request<HealthResponse>("GET", "/health");
  }

  async connections(): Promise<unknown> {
    return this.transport.request("GET", "/admin/connections");
  }

  async events(opts?: EventListOptions): Promise<unknown> {
    let path = "/admin/events";
    if (opts?.limit != null) {
      path += `?limit=${opts.limit}`;
    }
    return this.transport.request("GET", path);
  }

  async errors(opts?: ErrorListOptions): Promise<unknown> {
    const params = new URLSearchParams();
    if (opts?.limit != null) params.set("limit", String(opts.limit));
    if (opts?.category) params.set("category", opts.category);
    const qs = params.toString();
    return this.transport.request("GET", `/admin/errors${qs ? `?${qs}` : ""}`);
  }

  async stats(): Promise<unknown> {
    return this.transport.request("GET", "/admin/stats");
  }
}
