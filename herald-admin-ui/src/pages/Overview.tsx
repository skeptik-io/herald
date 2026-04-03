import { useEffect, useState, useRef } from "react";
import { useAuth } from "../context/AuthContext";
import type { HealthResponse, TenantCurrent, TenantSnapshot } from "../api/client";
import { PageHeader, Stat, Card, Badge, formatUptime } from "../components/shared";
import MiniChart from "../components/MiniChart";

export default function Overview() {
  const { client } = useAuth();
  const [health, setHealth] = useState<HealthResponse | null>(null);
  const [current, setCurrent] = useState<TenantCurrent | null>(null);
  const [snapshots, setSnapshots] = useState<TenantSnapshot[]>([]);
  const [error, setError] = useState("");
  const intervalRef = useRef<ReturnType<typeof setInterval>>(undefined);

  function refresh() {
    if (!client) return;
    client.health().then(setHealth).catch((e: Error) => setError(e.message));
    client
      .getTenantStats(Date.now() - 3600_000)
      .then((r) => {
        setCurrent(r.current);
        setSnapshots(r.snapshots);
      })
      .catch(() => {});
  }

  useEffect(() => {
    refresh();
    intervalRef.current = setInterval(refresh, 5000);
    return () => clearInterval(intervalRef.current);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [client]);

  if (error) return <div className="text-red-400">{error}</div>;

  return (
    <div>
      <PageHeader title="Overview" />

      <div className="grid grid-cols-2 gap-4 mb-6">
        <Card>
          <p className="text-xs text-zinc-500 mb-1">Current connections</p>
          <p className="text-3xl font-bold">{current?.connections ?? health?.connections ?? "-"}</p>
        </Card>
        <Card>
          <p className="text-xs text-zinc-500 mb-1">Total messages sent</p>
          <p className="text-3xl font-bold">
            {current?.messages_sent?.toLocaleString() ?? "-"}
          </p>
        </Card>
      </div>

      <div className="grid grid-cols-2 gap-4 mb-6">
        <Card>
          <p className="text-xs text-zinc-500 mb-3">Connections</p>
          <MiniChart
            data={snapshots.map((s) => ({ t: s.timestamp, v: s.connections }))}
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

      <p className="text-xs text-zinc-600 mb-6">
        Charts update every 60 seconds.
      </p>

      {health && (
        <div className="grid grid-cols-2 lg:grid-cols-4 gap-4 mb-6">
          <Stat label="Status" value={health.status} />
          <Stat label="Connections" value={current?.connections ?? health.connections} />
          <Stat label="Rooms" value={current?.rooms ?? health.rooms} />
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
