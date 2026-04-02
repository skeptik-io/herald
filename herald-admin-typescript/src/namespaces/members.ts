import type { HttpTransport } from "../transport.js";
import type { Member } from "../types.js";

export class MemberNamespace {
  constructor(private transport: HttpTransport) {}

  async add(roomId: string, userId: string, role?: string): Promise<Member> {
    return this.transport.request<Member>(
      "POST",
      `/rooms/${encodeURIComponent(roomId)}/members`,
      { user_id: userId, role },
    );
  }

  async list(roomId: string): Promise<Member[]> {
    const resp = await this.transport.request<{ members: Member[] }>(
      "GET",
      `/rooms/${encodeURIComponent(roomId)}/members`,
    );
    return resp.members;
  }

  async remove(roomId: string, userId: string): Promise<void> {
    return this.transport.request(
      "DELETE",
      `/rooms/${encodeURIComponent(roomId)}/members/${encodeURIComponent(userId)}`,
    );
  }

  async update(roomId: string, userId: string, role: string): Promise<void> {
    return this.transport.request(
      "PATCH",
      `/rooms/${encodeURIComponent(roomId)}/members/${encodeURIComponent(userId)}`,
      { role },
    );
  }
}
