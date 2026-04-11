import { useMemo, type ReactNode } from "react";
import type { Member } from "herald-chat";
import { useMembers } from "../hooks/use-members.js";

export interface PresenceIndicatorProps {
  streamId: string;
  children: (state: PresenceIndicatorState) => ReactNode;
}

export interface PresenceIndicatorState {
  members: Member[];
  /** Members with presence === "online" */
  online: Member[];
  /** Members with presence === "away" */
  away: Member[];
  /** Members with presence === "dnd" */
  dnd: Member[];
  /** Members with presence === "offline" */
  offline: Member[];
  /** Count of online members */
  onlineCount: number;
}

export function PresenceIndicator({ streamId, children }: PresenceIndicatorProps) {
  const members = useMembers(streamId);

  const state = useMemo(() => {
    const online = members.filter((m) => m.presence === "online");
    const away = members.filter((m) => m.presence === "away");
    const dnd = members.filter((m) => m.presence === "dnd");
    const offline = members.filter((m) => m.presence === "offline");
    return {
      members,
      online,
      away,
      dnd,
      offline,
      onlineCount: online.length,
    };
  }, [members]);

  return <>{children(state)}</>;
}
