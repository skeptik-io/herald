import { useEffect, useState } from "react";
import { useAuth } from "../context/AuthContext";
import type { ErrorEntry } from "../api/client";
import { PageHeader, Card, EmptyState, formatTime } from "../components/shared";

type Tab = "client" | "webhook" | "http";

export default function ErrorLogs() {
  const { client } = useAuth();
  const [tab, setTab] = useState<Tab>("client");
  const [errors, setErrors] = useState<ErrorEntry[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    if (!client) return;
    setLoading(true);
    client
      .listErrors(tab, 200)
      .then(setErrors)
      .catch(() => setErrors([]))
      .finally(() => setLoading(false));
  }, [client, tab]);

  const tabs: Tab[] = ["client", "webhook", "http"];
  const tabLabels: Record<Tab, string> = {
    client: "Client errors",
    webhook: "Webhook errors",
    http: "HTTP errors",
  };
  const tabDescriptions: Record<Tab, string> = {
    client:
      "This page shows the most recent client errors, which occur when the Channels API is used incorrectly. You can use this list to find problems with your implementation.",
    webhook: "Webhook delivery failures and errors.",
    http: "HTTP API errors from invalid requests.",
  };

  return (
    <div>
      <PageHeader title="Error Logs" />

      <div className="flex gap-4 mb-4 border-b border-zinc-800">
        {tabs.map((t) => (
          <button
            key={t}
            onClick={() => setTab(t)}
            className={`pb-2 text-sm transition-colors border-b-2 -mb-px ${
              tab === t
                ? "border-blue-500 text-white"
                : "border-transparent text-zinc-400 hover:text-zinc-200"
            }`}
          >
            {tabLabels[t]}
          </button>
        ))}
      </div>

      <p className="text-sm text-zinc-400 mb-4">{tabDescriptions[tab]}</p>

      {loading ? (
        <p className="text-zinc-500 text-sm">Loading...</p>
      ) : errors.length === 0 ? (
        <Card>
          <EmptyState message={`No ${tab} errors have been logged in the last 24 hours`} />
        </Card>
      ) : (
        <Card className="!p-0">
          <table className="w-full text-sm">
            <thead className="text-zinc-400 text-left text-xs uppercase">
              <tr>
                <th className="px-4 py-2.5">Error</th>
                <th className="px-4 py-2.5 w-40 text-right">Time</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-zinc-800">
              {errors.map((e, i) => (
                <tr key={i} className="hover:bg-zinc-800/30">
                  <td className="px-4 py-2.5">
                    <p className="text-zinc-200 text-sm">{e.message}</p>
                    {Object.keys(e.details).length > 0 && (
                      <p className="text-xs text-zinc-500 font-mono mt-0.5">
                        {JSON.stringify(e.details)}
                      </p>
                    )}
                  </td>
                  <td className="px-4 py-2.5 text-zinc-500 text-xs text-right whitespace-nowrap">
                    {formatTime(e.timestamp)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      )}
    </div>
  );
}
