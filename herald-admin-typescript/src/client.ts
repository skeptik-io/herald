import { HttpTransport } from "./transport.js";
import { MemberNamespace } from "./namespaces/members.js";
import { MessageNamespace } from "./namespaces/messages.js";
import { PresenceNamespace } from "./namespaces/presence.js";
import { RoomNamespace } from "./namespaces/rooms.js";
import type { HealthResponse } from "./types.js";

export interface HeraldAdminOptions {
  /** Herald HTTP API URL, e.g. http://localhost:6201 */
  url: string;
  /** API bearer token (must match auth.api.tokens in herald.toml) */
  token: string;
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

  private transport: HttpTransport;

  constructor(options: HeraldAdminOptions) {
    this.transport = new HttpTransport(options.url, options.token);
    this.rooms = new RoomNamespace(this.transport);
    this.members = new MemberNamespace(this.transport);
    this.messages = new MessageNamespace(this.transport);
    this.presence = new PresenceNamespace(this.transport);
  }

  async health(): Promise<HealthResponse> {
    return this.transport.request<HealthResponse>("GET", "/health");
  }
}
