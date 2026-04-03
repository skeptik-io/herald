import type { HttpTransport } from "../transport.js";

export class BlockNamespace {
  constructor(private transport: HttpTransport) {}

  async block(userId: string, blockedId: string): Promise<void> {
    await this.transport.request("POST", "/blocks", { user_id: userId, blocked_id: blockedId });
  }

  async unblock(userId: string, blockedId: string): Promise<void> {
    await this.transport.request("DELETE", "/blocks", { user_id: userId, blocked_id: blockedId });
  }

  async list(userId: string): Promise<string[]> {
    const res = await this.transport.request<{ blocked: string[] }>("GET", `/blocks/${encodeURIComponent(userId)}`);
    return res.blocked;
  }
}
