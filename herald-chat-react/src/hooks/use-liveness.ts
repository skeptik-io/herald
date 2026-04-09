import { useSyncExternalStore } from "react";
import type { LivenessState } from "herald-chat";
import { useChatCore } from "../context.js";

const NOOP_SUBSCRIBE = () => () => {};

export function useLiveness(): LivenessState {
  const core = useChatCore();

  return useSyncExternalStore(
    core ? (cb) => core.subscribe("liveness", cb) : NOOP_SUBSCRIBE,
    core ? () => core.getLivenessState() : () => "active" as LivenessState,
    () => "active" as LivenessState,
  );
}
