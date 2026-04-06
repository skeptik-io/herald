import { useSyncExternalStore } from "react";
import type { Member } from "herald-chat";
import { useChatCore } from "../context.js";

export function useMembers(streamId: string): Member[] {
  const core = useChatCore();

  return useSyncExternalStore(
    (cb) => core.subscribe(`members:${streamId}`, cb),
    () => core.getMembers(streamId),
    () => EMPTY,
  );
}

const EMPTY: Member[] = [];
