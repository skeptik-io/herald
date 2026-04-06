import { HttpTransport } from "./transport.js";
import { MemberNamespace } from "./namespaces/members.js";
import { EventNamespace } from "./namespaces/messages.js";
import { PresenceNamespace } from "./namespaces/presence.js";
import { StreamNamespace } from "./namespaces/rooms.js";
import { TenantNamespace } from "./namespaces/tenants.js";
import { BlockNamespace } from "./namespaces/blocks.js";
import type { HealthResponse } from "./types.js";

export interface HeraldAdminOptions {
  /** Herald HTTP API URL, e.g. http://localhost:6201 */
  url: string;
  /** API bearer token (must match auth.api.tokens in herald.toml) */
  token: string;
}

export interface AdminEventListOptions {
  limit?: number;
}

export interface ErrorListOptions {
  limit?: number;
  category?: string;
}

/**
 * Herald HTTP admin client for backend services.
 *
 * Provides namespaced access to Herald's HTTP API for stream management,
 * member management, event publishing, and presence queries.
 */
export class HeraldAdmin {
  // --- Core namespaces (event transport) ---
  public readonly streams: StreamNamespace;
  public readonly members: MemberNamespace;
  public readonly events: EventNamespace;
  public readonly tenants: TenantNamespace;

  // --- Chat namespaces (conversational layer) ---

  /** @deprecated Use `chat.presence` instead. */
  public readonly presence: PresenceNamespace;
  /** @deprecated Use `chat.blocks` instead. */
  public readonly blocks: BlockNamespace;

  /**
   * Chat-specific namespaces (conversational layer).
   * Groups presence and block operations that are part of the Herald Chat product.
   */
  public readonly chat: {
    readonly presence: PresenceNamespace;
    readonly blocks: BlockNamespace;
  };

  private transport: HttpTransport;

  constructor(options: HeraldAdminOptions) {
    this.transport = new HttpTransport(options.url, options.token);
    this.streams = new StreamNamespace(this.transport);
    this.members = new MemberNamespace(this.transport);
    this.events = new EventNamespace(this.transport);
    this.presence = new PresenceNamespace(this.transport);
    this.tenants = new TenantNamespace(this.transport);
    this.blocks = new BlockNamespace(this.transport);
    this.chat = { presence: this.presence, blocks: this.blocks };
  }

  async health(): Promise<HealthResponse> {
    return this.transport.request<HealthResponse>("GET", "/health");
  }

  async connections(): Promise<unknown> {
    return this.transport.request("GET", "/admin/connections");
  }

  async adminEvents(opts?: AdminEventListOptions): Promise<unknown> {
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
