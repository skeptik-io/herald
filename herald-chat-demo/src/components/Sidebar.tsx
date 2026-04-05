import { streams, users, CURRENT_USER_ID } from "../data/seed";
import { usePresence, useUnreadCounts } from "../hooks/useHerald";
import { PresenceIndicator } from "./PresenceIndicator";

interface SidebarProps {
  activeStream: string;
  onSelectStream: (id: string) => void;
}

export function Sidebar({ activeStream, onSelectStream }: SidebarProps) {
  const presence = usePresence();
  const unreadCounts = useUnreadCounts();

  // Total unread badge
  const totalUnread = Object.values(unreadCounts).reduce((a, b) => a + b, 0);

  return (
    <aside className="sidebar">
      <div className="sidebar-header">
        <h1 className="sidebar-title">Herald Chat</h1>
        {totalUnread > 0 && (
          <span className="badge badge-total">{totalUnread}</span>
        )}
      </div>

      <div className="sidebar-section-label">Direct Messages</div>
      {streams
        .filter((s) => s.kind === "dm")
        .map((stream) => {
          const otherId = stream.members.find((m) => m !== CURRENT_USER_ID)!;
          const other = users[otherId];
          const unread = unreadCounts[stream.id] ?? 0;
          const isActive = stream.id === activeStream;

          return (
            <button
              key={stream.id}
              className={`sidebar-item ${isActive ? "active" : ""}`}
              onClick={() => onSelectStream(stream.id)}
            >
              <div className="sidebar-avatar">
                <span className="avatar-text">{other.avatar}</span>
                <PresenceIndicator status={presence[otherId] ?? "offline"} />
              </div>
              <div className="sidebar-item-content">
                <span className="sidebar-item-name">{other.name}</span>
              </div>
              {unread > 0 && <span className="badge">{unread}</span>}
            </button>
          );
        })}

      <div className="sidebar-section-label">Groups</div>
      {streams
        .filter((s) => s.kind === "group")
        .map((stream) => {
          const unread = unreadCounts[stream.id] ?? 0;
          const isActive = stream.id === activeStream;
          const onlineCount = stream.members.filter(
            (m) => m !== CURRENT_USER_ID && (presence[m] === "online"),
          ).length;

          return (
            <button
              key={stream.id}
              className={`sidebar-item ${isActive ? "active" : ""}`}
              onClick={() => onSelectStream(stream.id)}
            >
              <div className="sidebar-avatar group-avatar">
                <span className="avatar-text">#</span>
              </div>
              <div className="sidebar-item-content">
                <span className="sidebar-item-name">{stream.name}</span>
                <span className="sidebar-item-meta">
                  {onlineCount} online
                </span>
              </div>
              {unread > 0 && <span className="badge">{unread}</span>}
            </button>
          );
        })}

      <div className="sidebar-footer">
        <div className="sidebar-avatar current-user-avatar">
          <span className="avatar-text">{users[CURRENT_USER_ID].avatar}</span>
          <PresenceIndicator status={presence[CURRENT_USER_ID] ?? "online"} />
        </div>
        <div className="sidebar-item-content">
          <span className="sidebar-item-name">{users[CURRENT_USER_ID].name}</span>
          <span className="sidebar-item-meta">Online</span>
        </div>
      </div>
    </aside>
  );
}
