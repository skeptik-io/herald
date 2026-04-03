import { useEffect, useState, useRef } from "react";
import { useAuth } from "../context/AuthContext";
import type { HealthResponse, StatsSnapshot } from "../api/client";
import { PageHeader, Stat, Card, Badge, formatUptime } from "../components/shared";
import MiniChart from "../components/MiniChart";

export default function Overview() {
  const { client, auth } = useAuth();
  const [health, setHealth] = useState<HealthResponse | null>(null);
  const [metrics, setMetrics] = useState<Record<string, number>>({});
  const [snapshots, setSnapshots] = useState<StatsSnapshot[]>([]);
  const [error, setError] = useState("");
  const intervalRef = useRef<ReturnType<typeof setInterval>>(undefined);

  function refresh() {
    if (!client) return;
    client.health().then(setHealth).catch((e: Error) => setError(e.message));
    client
      .metrics()
      .then((raw) => setMetrics(parseMetrics(raw)))
      .catch(() => {});
    if (auth?.mode === "admin") {
      client
        .getStats(Date.now() - 3600_000)
        .then((r) => setSnapshots(r.snapshots))
        .catch(() => {});
    }
  }

  useEffect(() => {
    refresh();
    intervalRef.current = setInterval(refresh, 5000);
    return () => clearInterval(intervalRef.current);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [client]);

  if (error) return <div className="text-red-400">{error}</div>;

  const connections = health?.connections ?? 0;
  const messagesSent = metrics.herald_messages_sent_total ?? 0;

  return (
    <div>
      <PageHeader title="Overview" />

      <div className="grid grid-cols-2 gap-4 mb-6">
        <Card>
          <p className="text-xs text-zinc-500 mb-1">Current connections</p>
          <p className="text-3xl font-bold">{connections}</p>
        </Card>
        <Card>
          <p className="text-xs text-zinc-500 mb-1">Total messages sent</p>
          <p className="text-3xl font-bold">{messagesSent.toLocaleString()}</p>
        </Card>
      </div>

      {snapshots.length > 0 && (
        <div className="grid grid-cols-2 gap-4 mb-6">
          <Card>
            <p className="text-xs text-zinc-500 mb-3">Peak connections</p>
            <MiniChart
              data={snapshots.map((s) => ({ t: s.timestamp, v: s.peak_connections }))}
              color="#2dd4bf"
              height={120}
            />
          </Card>
          <Card>
            <p className="text-xs text-zinc-500 mb-3">Messages</p>
            <MiniChart
              data={snapshots.map((s) => ({ t: s.timestamp, v: s.messages_delta }))}
              color="#2dd4bf"
              height={120}
              type="bar"
            />
          </Card>
        </div>
      )}

      {health && (
        <div className="grid grid-cols-2 md:grid-cols-4 gap-4 mb-6">
          <Stat label="Status" value={health.status} />
          <Stat label="Connections" value={health.connections} />
          <Stat label="Rooms" value={health.rooms} />
          <Stat label="Uptime" value={formatUptime(health.uptime_secs)} />
        </div>
      )}

      {health && (
        <Card>
          <p className="text-xs text-zinc-500 uppercase tracking-wider mb-3">Engines</p>
          <div className="flex gap-4 text-sm">
            <span>
              Storage{" "}
              <Badge color={health.storage ? "green" : "red"}>
                {health.storage ? "ok" : "degraded"}
              </Badge>
            </span>
            <span>
              Cipher{" "}
              <Badge color={health.cipher ? "green" : "zinc"}>
                {health.cipher ? "enabled" : "disabled"}
              </Badge>
            </span>
            <span>
              Veil{" "}
              <Badge color={health.veil ? "green" : "zinc"}>
                {health.veil ? "enabled" : "disabled"}
              </Badge>
            </span>
            <span>
              Sentry{" "}
              <Badge color={health.sentry ? "green" : "zinc"}>
                {health.sentry ? "enabled" : "disabled"}
              </Badge>
            </span>
          </div>
        </Card>
      )}
    </div>
  );
}

function parseMetrics(raw: string): Record<string, number> {
  const result: Record<string, number> = {};
  for (const line of raw.split("\n")) {
    if (line.startsWith("#") || !line.trim()) continue;
    const parts = line.split(" ");
    if (parts.length >= 2) {
      const name = parts[0]!;
      const value = parseFloat(parts[1]!);
      if (!isNaN(value)) result[name] = value;
    }
  }
  return result;
}
