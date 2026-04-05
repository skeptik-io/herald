interface PresenceIndicatorProps {
  status: string;
  size?: "sm" | "md";
}

const statusColors: Record<string, string> = {
  online: "#22c55e",
  away: "#f59e0b",
  dnd: "#ef4444",
  offline: "#9ca3af",
};

export function PresenceIndicator({ status, size = "sm" }: PresenceIndicatorProps) {
  const px = size === "sm" ? 8 : 12;
  return (
    <span
      className="presence-dot"
      style={{
        display: "inline-block",
        width: px,
        height: px,
        borderRadius: "50%",
        backgroundColor: statusColors[status] ?? statusColors.offline,
        border: "2px solid var(--bg-primary)",
        flexShrink: 0,
      }}
      title={status}
    />
  );
}
