import type { HttpTransport } from "../transport.js";
import type { Stream } from "../types.js";

export class StreamNamespace {
  constructor(private transport: HttpTransport) {}

  async list(): Promise<Stream[]> {
    const res = await this.transport.request<{ streams: Stream[] }>("GET", "/streams");
    return res.streams;
  }

  async create(
    id: string,
    name: string,
    options?: { meta?: unknown; public?: boolean },
  ): Promise<Stream> {
    return this.transport.request<Stream>("POST", "/streams", {
      id,
      name,
      meta: options?.meta,
      public: options?.public,
    });
  }

  async get(id: string): Promise<Stream> {
    return this.transport.request<Stream>("GET", `/streams/${encodeURIComponent(id)}`);
  }

  async update(id: string, options: { name?: string; meta?: unknown; archived?: boolean }): Promise<void> {
    return this.transport.request("PATCH", `/streams/${encodeURIComponent(id)}`, options);
  }

  async delete(id: string): Promise<void> {
    return this.transport.request("DELETE", `/streams/${encodeURIComponent(id)}`);
  }
}
