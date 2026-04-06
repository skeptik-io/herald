import type { HttpTransport } from "../transport.js";
import type { EventList, EventPublishResult, ReactionSummary } from "../types.js";

export class EventNamespace {
  constructor(private transport: HttpTransport) {}

  async publish(
    streamId: string,
    sender: string,
    body: string,
    options?: { meta?: unknown; parentId?: string; excludeConnection?: string },
  ): Promise<EventPublishResult> {
    const headers: Record<string, string> = {};
    if (options?.excludeConnection) {
      headers["X-Exclude-Connection"] = options.excludeConnection;
    }
    return this.transport.request<EventPublishResult>(
      "POST",
      `/streams/${encodeURIComponent(streamId)}/events`,
      { sender, body, meta: options?.meta, parent_id: options?.parentId },
      headers,
    );
  }

  async list(
    streamId: string,
    options?: { before?: number; after?: number; limit?: number; thread?: string },
  ): Promise<EventList> {
    const params = new URLSearchParams();
    if (options?.before !== undefined) params.set("before", String(options.before));
    if (options?.after !== undefined) params.set("after", String(options.after));
    if (options?.limit !== undefined) params.set("limit", String(options.limit));
    if (options?.thread !== undefined) params.set("thread", options.thread);
    const qs = params.toString();
    const path = `/streams/${encodeURIComponent(streamId)}/events${qs ? `?${qs}` : ""}`;
    return this.transport.request<EventList>("GET", path);
  }

  /** @chat Chat-specific operation — event deletion. */
  async delete(streamId: string, eventId: string): Promise<void> {
    await this.transport.request(
      "DELETE",
      `/streams/${encodeURIComponent(streamId)}/events/${encodeURIComponent(eventId)}`,
    );
  }

  /** @chat Chat-specific operation — event editing. */
  async edit(streamId: string, eventId: string, body: string): Promise<void> {
    await this.transport.request(
      "PATCH",
      `/streams/${encodeURIComponent(streamId)}/events/${encodeURIComponent(eventId)}`,
      { body },
    );
  }

  /** @chat Chat-specific operation — reaction queries. */
  async getReactions(streamId: string, eventId: string): Promise<ReactionSummary[]> {
    const res = await this.transport.request<{ reactions: ReactionSummary[] }>(
      "GET",
      `/streams/${encodeURIComponent(streamId)}/events/${encodeURIComponent(eventId)}/reactions`,
    );
    return res.reactions;
  }

  async trigger(streamId: string, event: string, data?: unknown, excludeConnection?: number): Promise<void> {
    const body: Record<string, unknown> = { event };
    if (data !== undefined) body.data = data;
    if (excludeConnection !== undefined) body.exclude_connection = excludeConnection;
    await this.transport.request("POST", `/streams/${encodeURIComponent(streamId)}/trigger`, body);
  }

  async search(
    streamId: string,
    query: string,
    limit?: number,
  ): Promise<EventList> {
    const params = new URLSearchParams({ q: query });
    if (limit !== undefined) params.set("limit", String(limit));
    const path = `/streams/${encodeURIComponent(streamId)}/events/search?${params}`;
    return this.transport.request<EventList>("GET", path);
  }
}
