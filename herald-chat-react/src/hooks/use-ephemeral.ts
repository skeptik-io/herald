import { useSyncExternalStore } from "react";
import type { EventReceived } from "herald-sdk";
import { useChatCore } from "../context.js";

export function useEphemeral(streamId: string): EventReceived | undefined {
  if (!streamId) throw new Error("useEphemeral requires a streamId");
  const core = useChatCore();

  return useSyncExternalStore(
    (cb) => core.subscribe(`ephemeral:${streamId}`, cb),
    () => core.getLastEphemeral(streamId),
    () => undefined,
  );
}
