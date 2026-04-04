import type { HttpTransport } from "../transport.js";
import type { Cursor, MemberPresenceEntry, UserPresence } from "../types.js";

export class PresenceNamespace {
  constructor(private transport: HttpTransport) {}

  async getUser(userId: string): Promise<UserPresence> {
    return this.transport.request<UserPresence>(
      "GET",
      `/presence/${encodeURIComponent(userId)}`,
    );
  }

  async getStream(streamId: string): Promise<MemberPresenceEntry[]> {
    const resp = await this.transport.request<{ members: MemberPresenceEntry[] }>(
      "GET",
      `/streams/${encodeURIComponent(streamId)}/presence`,
    );
    return resp.members;
  }

  async getCursors(streamId: string): Promise<Cursor[]> {
    const resp = await this.transport.request<{ cursors: Cursor[] }>(
      "GET",
      `/streams/${encodeURIComponent(streamId)}/cursors`,
    );
    return resp.cursors;
  }
}
