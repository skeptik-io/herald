import { useSyncExternalStore } from "react";
import type { EventReceived } from "herald-sdk";
import { useChatCore } from "../context.js";

const NOOP_SUBSCRIBE = () => () => {};

export function useEphemeral(streamId: string, eventType?: string): EventReceived | undefined {
  if (!streamId) throw new Error("useEphemeral requires a streamId");
  const core = useChatCore();

  return useSyncExternalStore(
    core ? (cb) => core.subscribe(`ephemeral:${streamId}`, cb) : NOOP_SUBSCRIBE,
    core ? () => core.getLastEphemeral(streamId, eventType) : () => undefined,
    () => undefined,
  );
}
