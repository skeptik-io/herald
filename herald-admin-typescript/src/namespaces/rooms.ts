import type { HttpTransport } from "../transport.js";
import type { Room } from "../types.js";

export class RoomNamespace {
  constructor(private transport: HttpTransport) {}

  async list(): Promise<Room[]> {
    const res = await this.transport.request<{ rooms: Room[] }>("GET", "/rooms");
    return res.rooms;
  }

  async create(
    id: string,
    name: string,
    options?: { meta?: unknown; public?: boolean },
  ): Promise<Room> {
    return this.transport.request<Room>("POST", "/rooms", {
      id,
      name,
      meta: options?.meta,
      public: options?.public,
    });
  }

  async get(id: string): Promise<Room> {
    return this.transport.request<Room>("GET", `/rooms/${encodeURIComponent(id)}`);
  }

  async update(id: string, options: { name?: string; meta?: unknown; archived?: boolean }): Promise<void> {
    return this.transport.request("PATCH", `/rooms/${encodeURIComponent(id)}`, options);
  }

  async delete(id: string): Promise<void> {
    return this.transport.request("DELETE", `/rooms/${encodeURIComponent(id)}`);
  }
}
