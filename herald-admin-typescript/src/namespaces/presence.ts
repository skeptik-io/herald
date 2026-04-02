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

  async getRoom(roomId: string): Promise<MemberPresenceEntry[]> {
    const resp = await this.transport.request<{ members: MemberPresenceEntry[] }>(
      "GET",
      `/rooms/${encodeURIComponent(roomId)}/presence`,
    );
    return resp.members;
  }

  async getCursors(roomId: string): Promise<Cursor[]> {
    const resp = await this.transport.request<{ cursors: Cursor[] }>(
      "GET",
      `/rooms/${encodeURIComponent(roomId)}/cursors`,
    );
    return resp.cursors;
  }
}
