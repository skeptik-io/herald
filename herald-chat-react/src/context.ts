import { createContext, useContext } from "react";
import type { ChatCore } from "herald-chat";

export const ChatCoreContext = createContext<ChatCore | null>(null);

export function useChatCore(): ChatCore {
  const core = useContext(ChatCoreContext);
  if (!core) {
    throw new Error("useChatCore must be used within a <HeraldChatProvider>");
  }
  return core;
}
