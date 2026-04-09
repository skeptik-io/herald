import type { ReactNode } from "react";
import type { Message, PendingMessage } from "herald-chat";
import { useMessages } from "../hooks/use-messages.js";

export interface MessageListProps {
  streamId: string;
  children: (state: MessageListState) => ReactNode;
}

export interface MessageListState {
  messages: Message[];
  send(body: string, opts?: { meta?: unknown; parentId?: string }): Promise<PendingMessage>;
  edit(eventId: string, body: string): Promise<void>;
  deleteEvent(eventId: string): Promise<void>;
  loadMore(): Promise<boolean>;
}

export function MessageList({ streamId, children }: MessageListProps) {
  const { messages, send, edit, deleteEvent, loadMore } = useMessages(streamId);

  return <>{children({ messages, send, edit, deleteEvent, loadMore })}</>;
}
