import { useSyncExternalStore, useCallback } from "react";
import type { PresenceStatus } from "herald-chat";
import { useChatCore } from "../context.js";

const NOOP_SUBSCRIBE = () => () => {};

export interface UsePresenceReturn {
  /** Current effective presence (manual override or liveness-derived). */
  presence: PresenceStatus;
  /** Set a manual presence override. Suspends liveness auto-sync. */
  setPresence: (status: PresenceStatus, until?: string) => void;
  /** Clear manual override, revert to liveness-driven presence. */
  clearOverride: () => void;
}

/**
 * Returns the current user's effective presence and methods to set/clear
 * manual overrides. Supports all four states: online, away, dnd, offline.
 */
export function usePresence(): UsePresenceReturn {
  const core = useChatCore();

  const presence = useSyncExternalStore(
    core ? (cb) => core.subscribe("presence", cb) : NOOP_SUBSCRIBE,
    core ? () => core.getPresence() : () => "online" as PresenceStatus,
    () => "online" as PresenceStatus,
  );

  // Also subscribe to liveness changes for auto-derived presence
  useSyncExternalStore(
    core ? (cb) => core.subscribe("liveness", cb) : NOOP_SUBSCRIBE,
    core ? () => core.getPresence() : () => "online" as PresenceStatus,
    () => "online" as PresenceStatus,
  );

  const setPresence = useCallback(
    (status: PresenceStatus, until?: string) => {
      core?.setPresence(status, until);
    },
    [core],
  );

  const clearOverride = useCallback(() => {
    core?.clearPresenceOverride();
  }, [core]);

  return { presence, setPresence, clearOverride };
}
