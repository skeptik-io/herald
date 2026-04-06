import type { MemberPresence } from "herald-sdk";
import type { Member } from "../types.js";
import { Notifier } from "../notifier.js";

export class MemberStore {
  private streams = new Map<string, Member[]>();
  private versions = new Map<string, number>();

  constructor(private notifier: Notifier) {}

  setMembers(streamId: string, members: MemberPresence[]): void {
    this.streams.set(
      streamId,
      members.map((m) => ({
        userId: m.user_id,
        role: m.role,
        presence: m.presence,
      })),
    );
    this.bumpVersion(streamId);
  }

  updatePresence(streamId: string, userId: string, presence: string): void {
    const members = this.streams.get(streamId);
    if (!members) return;
    const member = members.find((m) => m.userId === userId);
    if (member) {
      member.presence = presence;
      this.streams.set(streamId, [...members]);
      this.bumpVersion(streamId);
    }
  }

  addMember(streamId: string, userId: string, role: string): void {
    const members = this.streams.get(streamId) ?? [];
    if (members.some((m) => m.userId === userId)) return;
    this.streams.set(streamId, [
      ...members,
      { userId, role, presence: "online" },
    ]);
    this.bumpVersion(streamId);
  }

  removeMember(streamId: string, userId: string): void {
    const members = this.streams.get(streamId);
    if (!members) return;
    const filtered = members.filter((m) => m.userId !== userId);
    if (filtered.length !== members.length) {
      this.streams.set(streamId, filtered);
      this.bumpVersion(streamId);
    }
  }

  getMembers(streamId: string): Member[] {
    return this.streams.get(streamId) ?? EMPTY;
  }

  getVersion(streamId: string): number {
    return this.versions.get(streamId) ?? 0;
  }

  clear(streamId: string): void {
    this.streams.delete(streamId);
    this.versions.delete(streamId);
  }

  clearAll(): void {
    this.streams.clear();
    this.versions.clear();
  }

  /** Return all stream IDs that contain a given user. */
  streamsForUser(userId: string): string[] {
    const result: string[] = [];
    for (const [streamId, members] of this.streams) {
      if (members.some((m) => m.userId === userId)) {
        result.push(streamId);
      }
    }
    return result;
  }

  private bumpVersion(streamId: string): void {
    this.versions.set(streamId, (this.versions.get(streamId) ?? 0) + 1);
    this.notifier.notify(`members:${streamId}`);
  }
}

const EMPTY: Member[] = [];
