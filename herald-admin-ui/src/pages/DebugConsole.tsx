import { useEffect, useState, useRef, useCallback, type FormEvent } from "react";
import { useAuth } from "../context/AuthContext";
import type { AdminEvent } from "../api/client";
import { PageHeader, Card, Btn, Input, Badge } from "../components/shared";

const EVENT_KINDS = [
  "connection",
  "disconnection",
  "message",
  "subscribe",
  "unsubscribe",
  "auth_failure",
  "client_error",
  "webhook_error",
  "http_error",
] as const;

const KIND_LABELS: Record<string, string> = {
  connection: "Connection",
  disconnection: "Disconnection",
  message: "Message",
  subscribe: "Subscribed",
  unsubscribe: "Unsubscribed",
  auth_failure: "Auth failure",
  client_error: "Client error",
  webhook_error: "Webhook error",
  http_error: "HTTP error",
};

const KIND_COLORS: Record<string, string> = {
  connection: "green",
  disconnection: "red",
  message: "blue",
  subscribe: "blue",
  unsubscribe: "zinc",
  auth_failure: "red",
  client_error: "red",
  webhook_error: "yellow",
  http_error: "yellow",
};

export default function DebugConsole() {
  const { client, auth } = useAuth();
  const [events, setEvents] = useState<AdminEvent[]>([]);
  const [paused, setPaused] = useState(false);
  const [showFilters, setShowFilters] = useState(false);
  const [enabledKinds, setEnabledKinds] = useState<Set<string>>(
    () => new Set(EVENT_KINDS),
  );
  const pausedRef = useRef(false);
  const eventsRef = useRef<AdminEvent[]>([]);
  const eventSourceRef = useRef<EventSource | null>(null);

  useEffect(() => {
    pausedRef.current = paused;
  }, [paused]);
  useEffect(() => {
    eventsRef.current = events;
  }, [events]);

  const addEvent = useCallback((event: AdminEvent) => {
    if (pausedRef.current) return;
    eventsRef.current = [event, ...eventsRef.current].slice(0, 200);
    setEvents(eventsRef.current);
  }, []);

  useEffect(() => {
    if (!client || !auth) return;

    client.listEvents(100).then((initial) => {
      setEvents(initial);
      eventsRef.current = initial;
    });

    const es = new EventSource(
      `/api/admin/events/stream?token=${encodeURIComponent(auth.token)}`,
    );
    eventSourceRef.current = es;

    es.onmessage = (e) => {
      try {
        const event: AdminEvent = JSON.parse(e.data);
        addEvent(event);
      } catch {
        /* ignore */
      }
    };

    return () => {
      es.close();
      eventSourceRef.current = null;
    };
  }, [client, auth, addEvent]);

  function toggleKind(kind: string) {
    setEnabledKinds((prev) => {
      const next = new Set(prev);
      if (next.has(kind)) next.delete(kind);
      else next.add(kind);
      return next;
    });
  }

  const filtered = events.filter((e) => enabledKinds.has(e.kind));

  return (
    <div>
      <PageHeader title="Debug Console">
        <div className="flex gap-2 items-center">
          <Btn
            variant={paused ? "primary" : "ghost"}
            onClick={() => setPaused(!paused)}
          >
            {paused ? "Resume" : "Pause"}
          </Btn>
          <Btn
            variant="ghost"
            onClick={() => {
              setEvents([]);
              eventsRef.current = [];
            }}
          >
            Clear logs
          </Btn>
          <div className="relative">
            <Btn
              variant={showFilters ? "primary" : "ghost"}
              onClick={() => setShowFilters(!showFilters)}
            >
              Filter {showFilters ? "▲" : "▼"}
            </Btn>
            {showFilters && (
              <div className="absolute right-0 top-full mt-1 z-10 w-72 rounded-lg border border-zinc-700 bg-zinc-800 p-4 shadow-xl">
                <p className="text-xs text-zinc-400 mb-3">
                  Toggle which events the debug console shows.
                </p>
                <div className="grid grid-cols-2 gap-2">
                  {EVENT_KINDS.map((kind) => (
                    <label
                      key={kind}
                      className="flex items-center gap-2 text-sm cursor-pointer"
                    >
                      <button
                        type="button"
                        onClick={() => toggleKind(kind)}
                        className={`w-8 h-4 rounded-full transition-colors relative ${
                          enabledKinds.has(kind)
                            ? "bg-teal-500"
                            : "bg-zinc-600"
                        }`}
                      >
                        <span
                          className={`absolute top-0.5 w-3 h-3 rounded-full bg-white transition-transform ${
                            enabledKinds.has(kind)
                              ? "translate-x-4"
                              : "translate-x-0.5"
                          }`}
                        />
                      </button>
                      <span className="text-zinc-300 text-xs">
                        {KIND_LABELS[kind]}
                      </span>
                    </label>
                  ))}
                </div>
                <div className="mt-3 border-t border-zinc-700 pt-2">
                  <Btn
                    variant="ghost"
                    size="xs"
                    onClick={() => setEnabledKinds(new Set(EVENT_KINDS))}
                  >
                    Reset filters
                  </Btn>
                </div>
              </div>
            )}
          </div>
        </div>
      </PageHeader>

      <EventCreator />

      <Card className="!p-0">
        <table className="w-full text-sm">
          <thead className="text-zinc-400 text-left text-xs uppercase">
            <tr>
              <th className="px-4 py-2.5 w-36">Event</th>
              <th className="px-4 py-2.5">Details</th>
              <th className="px-4 py-2.5 w-24 text-right">Time</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-zinc-800">
            {filtered.length === 0 ? (
              <tr>
                <td colSpan={3} className="px-4 py-12 text-center text-zinc-500">
                  {events.length === 0
                    ? "Waiting for new events..."
                    : "No events match the current filters."}
                </td>
              </tr>
            ) : (
              filtered.map((e) => (
                <tr key={e.id} className="hover:bg-zinc-800/30">
                  <td className="px-4 py-2">
                    <Badge color={KIND_COLORS[e.kind] ?? "zinc"}>
                      {KIND_LABELS[e.kind] ?? e.kind}
                    </Badge>
                  </td>
                  <td className="px-4 py-2 text-zinc-300 text-xs font-mono truncate max-w-md">
                    {formatDetails(e)}
                  </td>
                  <td className="px-4 py-2 text-zinc-500 text-xs text-right whitespace-nowrap">
                    {new Date(e.timestamp).toLocaleTimeString()}
                  </td>
                </tr>
              ))
            )}
          </tbody>
        </table>
      </Card>
    </div>
  );
}

function formatDetails(e: AdminEvent): string {
  const d = e.details;
  switch (e.kind) {
    case "connection":
    case "disconnection":
      return `User: ${d.user_id ?? "?"} Socket ID: ${d.conn_id ?? "?"}`;
    case "message":
      return `Room: ${d.room ?? "?"} Sender: ${d.sender ?? "?"} Seq: ${d.seq ?? "?"}`;
    case "auth_failure":
      return `${d.error ?? "Unknown error"} Socket ID: ${d.conn_id ?? "?"}`;
    case "subscribe":
    case "unsubscribe":
      return `Room: ${d.room ?? "?"} User: ${d.user_id ?? "?"}`;
    default:
      return JSON.stringify(d);
  }
}

function EventCreator() {
  const [open, setOpen] = useState(false);
  const { client } = useAuth();
  const [room, setRoom] = useState("");
  const [sender, setSender] = useState("system");
  const [body, setBody] = useState("");

  async function send(e: FormEvent) {
    e.preventDefault();
    if (!client || !room || !body) return;
    try {
      await client.sendMessage(room, sender, body);
      setBody("");
    } catch {
      /* ignore */
    }
  }

  return (
    <div className="mb-4">
      <button
        onClick={() => setOpen(!open)}
        className="flex items-center gap-2 text-sm text-blue-400 hover:text-blue-300 mb-2"
      >
        <span>{open ? "▼" : "▶"}</span> Event creator
      </button>
      {open && (
        <form onSubmit={send} className="flex gap-2 items-end">
          <div className="w-32">
            <label className="block text-xs text-zinc-400 mb-1">Room</label>
            <Input
              value={room}
              onChange={(e) => setRoom(e.target.value)}
              required
              placeholder="general"
            />
          </div>
          <div className="w-24">
            <label className="block text-xs text-zinc-400 mb-1">Sender</label>
            <Input value={sender} onChange={(e) => setSender(e.target.value)} />
          </div>
          <div className="flex-1">
            <label className="block text-xs text-zinc-400 mb-1">Body</label>
            <Input
              value={body}
              onChange={(e) => setBody(e.target.value)}
              required
              placeholder="Hello..."
            />
          </div>
          <Btn type="submit">Send</Btn>
        </form>
      )}
    </div>
  );
}
