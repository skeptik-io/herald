import { useSyncExternalStore } from "react";
import { useChatCore } from "../context.js";

const NOOP_SUBSCRIBE = () => () => {};
const ZERO = () => 0;

export function useUnreadCount(streamId: string): number {
  if (!streamId) throw new Error("useUnreadCount requires a streamId");
  const core = useChatCore();

  return useSyncExternalStore(
    core ? (cb) => core.subscribe(`unread:${streamId}`, cb) : NOOP_SUBSCRIBE,
    core ? () => core.getUnreadCount(streamId) : ZERO,
    ZERO,
  );
}

export function useTotalUnreadCount(): number {
  const core = useChatCore();

  return useSyncExternalStore(
    core ? (cb) => core.subscribe("unread:total", cb) : NOOP_SUBSCRIBE,
    core ? () => core.getTotalUnreadCount() : ZERO,
    ZERO,
  );
}
