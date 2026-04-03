import { useEffect, useState } from "react";
import { useAuth } from "../context/AuthContext";
import type { StatsSnapshot } from "../api/client";
import { PageHeader, Card } from "../components/shared";
import MiniChart from "../components/MiniChart";

type Period = "day" | "week" | "month";

const PERIOD_MS: Record<Period, number> = {
  day: 86_400_000,
  week: 7 * 86_400_000,
  month: 30 * 86_400_000,
};

export default function Stats() {
  const { client } = useAuth();
  const [period, setPeriod] = useState<Period>("day");
  const [snapshots, setSnapshots] = useState<StatsSnapshot[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    if (!client) return;
    setLoading(true);
    const now = Date.now();
    client
      .getStats(now - PERIOD_MS[period], now)
      .then((r) => setSnapshots(r.snapshots))
      .catch(() => {})
      .finally(() => setLoading(false));
  }, [client, period]);

  const periodLabel = (() => {
    const now = new Date();
    const from = new Date(now.getTime() - PERIOD_MS[period]);
    return `${from.toLocaleDateString("en-US", { month: "short", day: "numeric" })} - ${now.toLocaleDateString("en-US", { month: "short", day: "numeric" })}`;
  })();

  return (
    <div>
      <PageHeader title={`Historical usage: ${periodLabel}`}>
        <div className="flex gap-1 bg-zinc-800 rounded-lg p-0.5">
          {(["day", "week", "month"] as const).map((p) => (
            <button
              key={p}
              onClick={() => setPeriod(p)}
              className={`px-3 py-1 text-xs rounded-md capitalize transition-colors ${
                period === p
                  ? "bg-blue-600 text-white"
                  : "text-zinc-400 hover:text-white"
              }`}
            >
              {p}
            </button>
          ))}
        </div>
      </PageHeader>

      {loading ? (
        <p className="text-zinc-500 text-sm">Loading...</p>
      ) : (
        <div className="space-y-4">
          <Card>
            <p className="text-xs text-zinc-500 mb-3">
              Average connections / Peak connections
            </p>
            <div style={{ height: 200 }}>
              <MiniChart
                data={snapshots.map((s) => ({ t: s.timestamp, v: s.peak_connections }))}
                color="#38bdf8"
                height={200}
              />
            </div>
            <div className="flex gap-4 mt-2 text-xs text-zinc-500">
              <span className="flex items-center gap-1">
                <span className="w-3 h-0.5 bg-teal-400 inline-block rounded" /> Average connections
              </span>
              <span className="flex items-center gap-1">
                <span className="w-3 h-0.5 bg-sky-400 inline-block rounded" /> Peak connections
              </span>
            </div>
          </Card>

          <Card>
            <p className="text-xs text-zinc-500 mb-3">Messages / Webhooks</p>
            <div style={{ height: 200 }}>
              <MiniChart
                data={snapshots.map((s) => ({ t: s.timestamp, v: s.messages_delta }))}
                color="#2dd4bf"
                height={200}
                type="bar"
              />
            </div>
            <div className="flex gap-4 mt-2 text-xs text-zinc-500">
              <span className="flex items-center gap-1">
                <span className="w-3 h-2 bg-teal-400 inline-block rounded" /> Messages
              </span>
              <span className="flex items-center gap-1">
                <span className="w-3 h-0.5 bg-sky-400 inline-block rounded" /> Webhooks
              </span>
            </div>
          </Card>

          <Card>
            <p className="text-xs text-zinc-500 mb-3">Messages rate</p>
            <div style={{ height: 200 }}>
              <MiniChart
                data={snapshots.map((s) => ({ t: s.timestamp, v: s.messages_delta }))}
                color="#2dd4bf"
                height={200}
              />
            </div>
          </Card>

          <p className="text-xs text-zinc-600">All times are UTC</p>
        </div>
      )}
    </div>
  );
}
