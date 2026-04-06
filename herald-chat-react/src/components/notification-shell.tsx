import type { ReactNode } from "react";
import { useSyncExternalStore } from "react";
import { useChatCore } from "../context.js";

export interface NotificationShellProps {
  children: (state: { totalUnread: number }) => ReactNode;
}

export function NotificationShell({ children }: NotificationShellProps) {
  const core = useChatCore();

  const totalUnread = useSyncExternalStore(
    (cb) => core.subscribe("unread:total", cb),
    () => core.getTotalUnreadCount(),
    () => 0,
  );

  return <>{children({ totalUnread })}</>;
}
