import type { HttpTransport } from "../transport.js";
import type { MessageList, MessageSendResult, ReactionSummary } from "../types.js";

export class MessageNamespace {
  constructor(private transport: HttpTransport) {}

  async send(
    roomId: string,
    sender: string,
    body: string,
    options?: { meta?: unknown; parentId?: string; excludeConnection?: string },
  ): Promise<MessageSendResult> {
    const headers: Record<string, string> = {};
    if (options?.excludeConnection) {
      headers["X-Exclude-Connection"] = options.excludeConnection;
    }
    return this.transport.request<MessageSendResult>(
      "POST",
      `/rooms/${encodeURIComponent(roomId)}/messages`,
      { sender, body, meta: options?.meta, parent_id: options?.parentId },
      headers,
    );
  }

  async list(
    roomId: string,
    options?: { before?: number; after?: number; limit?: number; thread?: string },
  ): Promise<MessageList> {
    const params = new URLSearchParams();
    if (options?.before !== undefined) params.set("before", String(options.before));
    if (options?.after !== undefined) params.set("after", String(options.after));
    if (options?.limit !== undefined) params.set("limit", String(options.limit));
    if (options?.thread !== undefined) params.set("thread", options.thread);
    const qs = params.toString();
    const path = `/rooms/${encodeURIComponent(roomId)}/messages${qs ? `?${qs}` : ""}`;
    return this.transport.request<MessageList>("GET", path);
  }

  async delete(roomId: string, messageId: string): Promise<void> {
    await this.transport.request(
      "DELETE",
      `/rooms/${encodeURIComponent(roomId)}/messages/${encodeURIComponent(messageId)}`,
    );
  }

  async edit(roomId: string, messageId: string, body: string): Promise<void> {
    await this.transport.request(
      "PATCH",
      `/rooms/${encodeURIComponent(roomId)}/messages/${encodeURIComponent(messageId)}`,
      { body },
    );
  }

  async getReactions(roomId: string, messageId: string): Promise<ReactionSummary[]> {
    const res = await this.transport.request<{ reactions: ReactionSummary[] }>(
      "GET",
      `/rooms/${encodeURIComponent(roomId)}/messages/${encodeURIComponent(messageId)}/reactions`,
    );
    return res.reactions;
  }

  async search(
    roomId: string,
    query: string,
    limit?: number,
  ): Promise<MessageList> {
    const params = new URLSearchParams({ q: query });
    if (limit !== undefined) params.set("limit", String(limit));
    const path = `/rooms/${encodeURIComponent(roomId)}/messages/search?${params}`;
    return this.transport.request<MessageList>("GET", path);
  }
}
