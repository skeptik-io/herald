import { useEffect, useState, useRef, useCallback, type FormEvent } from "react";
import { useAuth } from "../context/AuthContext";
import type { AdminEvent } from "../api/client";
import { PageHeader, Card, Btn, Input, Badge } from "../components/shared";

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
  const [filter, setFilter] = useState("");
  const [filterText, setFilterText] = useState("");
  const pausedRef = useRef(false);
  const eventsRef = useRef<AdminEvent[]>([]);
  const eventSourceRef = useRef<EventSource | null>(null);

  // Keep refs in sync
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

  // Load initial events + connect SSE
  useEffect(() => {
    if (!client || !auth) return;

    client.listEvents(100).then((initial) => {
      setEvents(initial);
      eventsRef.current = initial;
    });

    // SSE connection via server-side proxy (token as query param since EventSource can't set headers)
    const es = new EventSource(`/api/admin/events/stream?token=${encodeURIComponent(auth.token)}`);
    eventSourceRef.current = es;

    es.onmessage = (e) => {
      try {
        const event: AdminEvent = JSON.parse(e.data);
        addEvent(event);
      } catch {
        /* ignore parse errors */
      }
    };

    return () => {
      es.close();
      eventSourceRef.current = null;
    };
  }, [client, auth, addEvent]);

  const filtered = filter
    ? events.filter(
        (e) =>
          e.kind.includes(filter) ||
          JSON.stringify(e.details).toLowerCase().includes(filter.toLowerCase()),
      )
    : events;

  function handleFilter(e: FormEvent) {
    e.preventDefault();
    setFilter(filterText);
  }

  return (
    <div>
      <PageHeader title="Debug Console">
        <div className="flex gap-2">
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
        </div>
      </PageHeader>

      <div className="flex gap-2 mb-4">
        <form onSubmit={handleFilter} className="flex gap-2 flex-1">
          <Input
            value={filterText}
            onChange={(e) => setFilterText(e.target.value)}
            placeholder="Search for events"
          />
          <Btn type="submit" variant="ghost">
            Filter
          </Btn>
        </form>
      </div>

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
                <td colSpan={3} className="px-4 py-8 text-center text-zinc-500">
                  No events yet. Connect clients to see activity.
                </td>
              </tr>
            ) : (
              filtered.map((e) => (
                <tr key={e.id} className="hover:bg-zinc-800/30">
                  <td className="px-4 py-2">
                    <Badge color={KIND_COLORS[e.kind] ?? "zinc"}>
                      {e.kind.replace("_", " ")}
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
            <Input value={room} onChange={(e) => setRoom(e.target.value)} required placeholder="general" />
          </div>
          <div className="w-24">
            <label className="block text-xs text-zinc-400 mb-1">Sender</label>
            <Input value={sender} onChange={(e) => setSender(e.target.value)} />
          </div>
          <div className="flex-1">
            <label className="block text-xs text-zinc-400 mb-1">Body</label>
            <Input value={body} onChange={(e) => setBody(e.target.value)} required placeholder="Hello..." />
          </div>
          <Btn type="submit">Send</Btn>
        </form>
      )}
    </div>
  );
}
