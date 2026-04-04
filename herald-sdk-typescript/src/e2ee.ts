/**
 * E2EE manager — maps streams to sessions, handles encrypt/decrypt lifecycle.
 */

import type { E2EESession } from "./crypto.js";

export class E2EEManager {
  private sessions = new Map<string, E2EESession>();

  setSession(stream: string, session: E2EESession): void {
    this.sessions.set(stream, session);
  }

  getSession(stream: string): E2EESession | undefined {
    return this.sessions.get(stream);
  }

  removeSession(stream: string): void {
    const session = this.sessions.get(stream);
    if (session) {
      session.destroy();
      this.sessions.delete(stream);
    }
  }

  encryptOutgoing(
    stream: string,
    body: string,
    meta?: unknown,
  ): { body: string; meta?: unknown } {
    const session = this.sessions.get(stream);
    if (!session) return { body, meta };

    const encrypted = session.encrypt(body, stream);
    const blindTokens = session.blind(body);
    const merged =
      meta && typeof meta === "object"
        ? { ...(meta as Record<string, unknown>), __blind: blindTokens }
        : { __blind: blindTokens };

    return { body: encrypted, meta: merged };
  }

  decryptIncoming(
    stream: string,
    body: string,
    meta?: unknown,
  ): { body: string; meta?: unknown; error?: boolean } {
    const session = this.sessions.get(stream);
    if (!session) return { body, meta };

    try {
      const decrypted = session.decrypt(body, stream);
      // Strip __blind from meta before exposing to consumer
      let cleanMeta = meta;
      if (meta && typeof meta === "object" && "__blind" in (meta as Record<string, unknown>)) {
        const { __blind, ...rest } = meta as Record<string, unknown>;
        cleanMeta = Object.keys(rest).length > 0 ? rest : undefined;
      }
      return { body: decrypted, meta: cleanMeta };
    } catch {
      return { body, meta, error: true };
    }
  }

  clear(): void {
    for (const session of this.sessions.values()) {
      session.destroy();
    }
    this.sessions.clear();
  }
}
