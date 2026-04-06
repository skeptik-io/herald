import { useSyncExternalStore } from "react";
import type { LivenessState } from "herald-chat";
import { useChatCore } from "../context.js";

export function useLiveness(): LivenessState {
  const core = useChatCore();

  return useSyncExternalStore(
    (cb) => core.subscribe("liveness", cb),
    () => core.getLivenessState(),
    () => "active" as LivenessState,
  );
}
