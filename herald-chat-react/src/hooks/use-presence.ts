import { useSyncExternalStore } from "react";
import type { LivenessState } from "herald-chat";
import { useChatCore } from "../context.js";

/**
 * Returns the current user's derived presence based on liveness state.
 * Maps: active → "online", idle → "away", hidden → "away"
 */
export function usePresence(): "online" | "away" {
  const core = useChatCore();

  const liveness = useSyncExternalStore(
    (cb) => core.subscribe("liveness", cb),
    () => core.getLivenessState(),
    () => "active" as LivenessState,
  );

  return liveness === "active" ? "online" : "away";
}
