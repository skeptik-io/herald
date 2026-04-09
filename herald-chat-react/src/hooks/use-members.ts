import { useSyncExternalStore } from "react";
import type { Member } from "herald-chat";
import { useChatCore } from "../context.js";

const NOOP_SUBSCRIBE = () => () => {};

export function useMembers(streamId: string): Member[] {
  if (!streamId) throw new Error("useMembers requires a streamId");
  const core = useChatCore();

  return useSyncExternalStore(
    core ? (cb) => core.subscribe(`members:${streamId}`, cb) : NOOP_SUBSCRIBE,
    core ? () => core.getMembers(streamId) : () => EMPTY,
    () => EMPTY,
  );
}

const EMPTY: Member[] = [];
