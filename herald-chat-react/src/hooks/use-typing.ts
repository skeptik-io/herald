import { useSyncExternalStore, useCallback } from "react";
import { useChatCore } from "../context.js";

export interface UseTypingReturn {
  typing: string[];
  sendTyping(): void;
}

const NOOP_SUBSCRIBE = () => () => {};
const NOOP = () => {};

export function useTyping(streamId: string): UseTypingReturn {
  if (!streamId) throw new Error("useTyping requires a streamId");
  const core = useChatCore();

  const typing = useSyncExternalStore(
    core ? (cb) => core.subscribe(`typing:${streamId}`, cb) : NOOP_SUBSCRIBE,
    core ? () => core.getTypingUsers(streamId) : () => EMPTY,
    () => EMPTY,
  );

  const sendTyping = useCallback(
    () => { if (core) core.startTyping(streamId); },
    [core, streamId],
  );

  return { typing, sendTyping };
}

const EMPTY: string[] = [];
