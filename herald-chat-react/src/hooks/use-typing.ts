import { useSyncExternalStore, useCallback } from "react";
import { useChatCore } from "../context.js";

export interface UseTypingReturn {
  typing: string[];
  sendTyping(): void;
}

export function useTyping(streamId: string): UseTypingReturn {
  const core = useChatCore();

  const typing = useSyncExternalStore(
    (cb) => core.subscribe(`typing:${streamId}`, cb),
    () => core.getTypingUsers(streamId),
    () => EMPTY,
  );

  const sendTyping = useCallback(
    () => core.startTyping(streamId),
    [core, streamId],
  );

  return { typing, sendTyping };
}

const EMPTY: string[] = [];
