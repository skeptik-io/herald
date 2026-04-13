import { useRef, useEffect, type ReactNode } from "react";
import { ChatCore, type ChatCoreOptions } from "herald-chat";
import type { HeraldClient } from "herald-sdk";
import type { HeraldChatClient } from "herald-chat-sdk";
import type { ChatWriter, LivenessConfig, Middleware } from "herald-chat";
import { ChatCoreContext } from "./context.js";

export interface HeraldChatProviderProps {
  client: HeraldClient;
  chat: HeraldChatClient;
  userId: string;
  liveness?: LivenessConfig;
  scrollIdleMs?: number;
  loadMoreLimit?: number;
  middleware?: Middleware[];
  /** Persist-first write path. See ChatWriter in @skeptik-io/herald-chat. */
  writer?: ChatWriter;
  children: ReactNode;
}

export function HeraldChatProvider({
  client,
  chat,
  userId,
  liveness,
  scrollIdleMs,
  loadMoreLimit,
  middleware,
  writer,
  children,
}: HeraldChatProviderProps) {
  const coreRef = useRef<ChatCore | null>(null);

  if (coreRef.current === null) {
    coreRef.current = new ChatCore({ client, chat, userId, liveness, scrollIdleMs, loadMoreLimit, middleware, writer });
  }

  const mountedRef = useRef(false);
  const disconnectTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    const core = coreRef.current!;
    mountedRef.current = true;

    // Cancel any pending disconnect from a prior strict-mode unmount
    if (disconnectTimer.current) {
      clearTimeout(disconnectTimer.current);
      disconnectTimer.current = null;
    }

    core.attach();
    if (!client.connected) {
      client.connect().catch(() => {
        // Connection errors surface via state change events
      });
    }

    return () => {
      mountedRef.current = false;
      core.detach();
      // Defer disconnect so React strict mode's unmount→remount cycle
      // can cancel it before it fires
      disconnectTimer.current = setTimeout(() => {
        if (!mountedRef.current) client.disconnect();
      }, 50);
    };
  }, []);

  return (
    <ChatCoreContext.Provider value={coreRef.current}>
      {children}
    </ChatCoreContext.Provider>
  );
}
