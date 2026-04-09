import { useSyncExternalStore } from "react";
import type { LivenessState } from "herald-chat";
import { useChatCore } from "../context.js";

const NOOP_SUBSCRIBE = () => () => {};

/**
 * Returns the current user's derived presence based on liveness state.
 * Maps: active → "online", idle → "away", hidden → "away"
 */
export function usePresence(): "online" | "away" {
  const core = useChatCore();

  const liveness = useSyncExternalStore(
    core ? (cb) => core.subscribe("liveness", cb) : NOOP_SUBSCRIBE,
    core ? () => core.getLivenessState() : () => "active" as LivenessState,
    () => "active" as LivenessState,
  );

  return liveness === "active" ? "online" : "away";
}
