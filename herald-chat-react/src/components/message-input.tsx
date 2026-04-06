import type { ReactNode } from "react";
import { useMessages } from "../hooks/use-messages.js";
import { useTyping } from "../hooks/use-typing.js";

export interface MessageInputProps {
  streamId: string;
  children: (state: MessageInputState) => ReactNode;
}

export interface MessageInputState {
  send(body: string, opts?: { meta?: unknown; parentId?: string }): Promise<string>;
  sendTyping(): void;
}

export function MessageInput({ streamId, children }: MessageInputProps) {
  const { send } = useMessages(streamId);
  const { sendTyping } = useTyping(streamId);

  return <>{children({ send, sendTyping })}</>;
}
