import { createContext, useContext } from "react";
import type { ChatCore } from "herald-chat";

export const ChatCoreContext = createContext<ChatCore | null>(null);

/**
 * Returns the ChatCore instance from the nearest HeraldChatProvider,
 * or `null` if no provider is mounted above this component.
 */
export function useChatCore(): ChatCore | null {
  return useContext(ChatCoreContext);
}
