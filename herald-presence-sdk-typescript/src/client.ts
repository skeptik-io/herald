import { type HeraldClient } from "herald-sdk";

export type PresenceStatus = "online" | "away" | "dnd" | "offline";

/**
 * Herald Presence WebSocket client — adds presence frame methods
 * (set presence, clear override) on top of HeraldClient.
 */
export class HeraldPresenceClient {
  constructor(private client: HeraldClient) {}

  /**
   * Set a manual presence override.
   *
   * - `"online"` clears any override (reverts to connection-derived presence).
   * - `"away"`, `"dnd"`, `"offline"` set a manual override.
   * - `until` is an optional ISO 8601 datetime for when the override expires
   *   (e.g. `"2026-04-14T09:00:00Z"` for "away until Monday 9am").
   */
  setPresence(status: PresenceStatus, until?: string): void {
    this.client.sendFrame({
      type: "presence.set",
      payload: { status, ...(until ? { until } : {}) },
    });
  }

  /**
   * Clear any manual presence override, reverting to connection-derived
   * presence (online if connected, offline if not).
   */
  clearOverride(): void {
    this.setPresence("online");
  }
}
