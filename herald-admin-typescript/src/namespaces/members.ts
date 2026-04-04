import type { HttpTransport } from "../transport.js";
import type { Member } from "../types.js";

export class MemberNamespace {
  constructor(private transport: HttpTransport) {}

  async add(streamId: string, userId: string, role?: string): Promise<Member> {
    return this.transport.request<Member>(
      "POST",
      `/streams/${encodeURIComponent(streamId)}/members`,
      { user_id: userId, role },
    );
  }

  async list(streamId: string): Promise<Member[]> {
    const resp = await this.transport.request<{ members: Member[] }>(
      "GET",
      `/streams/${encodeURIComponent(streamId)}/members`,
    );
    return resp.members;
  }

  async remove(streamId: string, userId: string): Promise<void> {
    return this.transport.request(
      "DELETE",
      `/streams/${encodeURIComponent(streamId)}/members/${encodeURIComponent(userId)}`,
    );
  }

  async update(streamId: string, userId: string, role: string): Promise<void> {
    return this.transport.request(
      "PATCH",
      `/streams/${encodeURIComponent(streamId)}/members/${encodeURIComponent(userId)}`,
      { role },
    );
  }
}
