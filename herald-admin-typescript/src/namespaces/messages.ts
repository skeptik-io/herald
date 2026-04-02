import type { HttpTransport } from "../transport.js";
import type { MessageList, MessageSendResult } from "../types.js";

export class MessageNamespace {
  constructor(private transport: HttpTransport) {}

  async send(
    roomId: string,
    sender: string,
    body: string,
    meta?: unknown,
  ): Promise<MessageSendResult> {
    return this.transport.request<MessageSendResult>(
      "POST",
      `/rooms/${encodeURIComponent(roomId)}/messages`,
      { sender, body, meta },
    );
  }

  async list(
    roomId: string,
    options?: { before?: number; after?: number; limit?: number },
  ): Promise<MessageList> {
    const params = new URLSearchParams();
    if (options?.before !== undefined) params.set("before", String(options.before));
    if (options?.after !== undefined) params.set("after", String(options.after));
    if (options?.limit !== undefined) params.set("limit", String(options.limit));
    const qs = params.toString();
    const path = `/rooms/${encodeURIComponent(roomId)}/messages${qs ? `?${qs}` : ""}`;
    return this.transport.request<MessageList>("GET", path);
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
