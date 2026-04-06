import { useMemo, type ReactNode } from "react";
import type { Member } from "herald-chat";
import { useMembers } from "../hooks/use-members.js";

export interface PresenceIndicatorProps {
  streamId: string;
  children: (state: PresenceIndicatorState) => ReactNode;
}

export interface PresenceIndicatorState {
  members: Member[];
  onlineCount: number;
}

export function PresenceIndicator({ streamId, children }: PresenceIndicatorProps) {
  const members = useMembers(streamId);

  const onlineCount = useMemo(
    () => members.filter((m) => m.presence === "online").length,
    [members],
  );

  return <>{children({ members, onlineCount })}</>;
}
