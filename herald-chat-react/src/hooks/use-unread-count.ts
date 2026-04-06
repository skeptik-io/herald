import { useSyncExternalStore } from "react";
import { useChatCore } from "../context.js";

export function useUnreadCount(streamId: string): number {
  const core = useChatCore();

  return useSyncExternalStore(
    (cb) => core.subscribe(`unread:${streamId}`, cb),
    () => core.getUnreadCount(streamId),
    () => 0,
  );
}

export function useTotalUnreadCount(): number {
  const core = useChatCore();

  return useSyncExternalStore(
    (cb) => core.subscribe("unread:total", cb),
    () => core.getTotalUnreadCount(),
    () => 0,
  );
}
