import { useSyncExternalStore, useCallback } from "react";
import type { Message, PendingMessage } from "herald-chat";
import { useChatCore } from "../context.js";

export interface UseMessagesReturn {
  messages: Message[];
  send(body: string, opts?: { meta?: unknown; parentId?: string }): Promise<PendingMessage>;
  edit(eventId: string, body: string): Promise<void>;
  deleteEvent(eventId: string): Promise<void>;
  loadMore(): Promise<boolean>;
}

export function useMessages(streamId: string): UseMessagesReturn {
  if (!streamId) throw new Error("useMessages requires a streamId");
  const core = useChatCore();

  const messages = useSyncExternalStore(
    (cb) => core.subscribe(`messages:${streamId}`, cb),
    () => core.getMessages(streamId),
    () => EMPTY,
  );

  const send = useCallback(
    (body: string, opts?: { meta?: unknown; parentId?: string }) =>
      core.send(streamId, body, opts),
    [core, streamId],
  );

  const edit = useCallback(
    (eventId: string, body: string) => core.edit(streamId, eventId, body),
    [core, streamId],
  );

  const deleteEvent = useCallback(
    (eventId: string) => core.deleteEvent(streamId, eventId),
    [core, streamId],
  );

  const loadMore = useCallback(
    () => core.loadMore(streamId),
    [core, streamId],
  );

  return { messages, send, edit, deleteEvent, loadMore };
}

const EMPTY: Message[] = [];
