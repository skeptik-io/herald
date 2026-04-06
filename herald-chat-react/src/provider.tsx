import { useRef, useEffect, type ReactNode } from "react";
import { ChatCore, type ChatCoreOptions } from "herald-chat";
import type { HeraldClient } from "herald-sdk";
import type { HeraldChatClient } from "herald-chat-sdk";
import type { LivenessConfig } from "herald-chat";
import { ChatCoreContext } from "./context.js";

export interface HeraldChatProviderProps {
  client: HeraldClient;
  chat: HeraldChatClient;
  userId: string;
  liveness?: LivenessConfig;
  scrollIdleMs?: number;
  children: ReactNode;
}

export function HeraldChatProvider({
  client,
  chat,
  userId,
  liveness,
  scrollIdleMs,
  children,
}: HeraldChatProviderProps) {
  const coreRef = useRef<ChatCore | null>(null);

  if (coreRef.current === null) {
    coreRef.current = new ChatCore({ client, chat, userId, liveness, scrollIdleMs });
  }

  useEffect(() => {
    const core = coreRef.current!;
    core.attach();
    return () => {
      core.detach();
    };
  }, []);

  return (
    <ChatCoreContext.Provider value={coreRef.current}>
      {children}
    </ChatCoreContext.Provider>
  );
}
