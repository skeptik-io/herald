import { useEffect, useState, type FormEvent } from "react";
import { useParams, useNavigate } from "react-router-dom";
import { useAuth } from "../context/AuthContext";
import type {
  Room,
  Member,
  Message,
  PresenceEntry,
  Cursor,
} from "../api/client";
import {
  PageHeader,
  Card,
  Table,
  Btn,
  Input,
  Select,
  Badge,
  EmptyState,
  formatTime,
  presenceColor,
} from "../components/shared";

type Tab = "members" | "messages" | "presence" | "cursors";

export default function RoomDetail() {
  const { id } = useParams<{ id: string }>();
  const { client } = useAuth();
  const navigate = useNavigate();
  const [room, setRoom] = useState<Room | null>(null);
  const [tab, setTab] = useState<Tab>("members");
  const [error, setError] = useState("");

  useEffect(() => {
    if (!client || !id) return;
    client.getRoom(id).then(setRoom).catch((e: Error) => setError(e.message));
  }, [client, id]);

  if (error) return <p className="text-red-400">{error}</p>;
  if (!room) return <p className="text-zinc-500 text-sm">Loading...</p>;

  const tabs: Tab[] = ["members", "messages", "presence", "cursors"];

  return (
    <div>
      <PageHeader title={room.name}>
        <div className="flex gap-2 items-center">
          <Badge color={room.encryption_mode === "server_encrypted" ? "green" : "zinc"}>
            {room.encryption_mode}
          </Badge>
          <Btn
            variant="danger"
            size="xs"
            onClick={async () => {
              if (!confirm(`Delete room "${room.id}"?`)) return;
              await client!.deleteRoom(room.id);
              navigate("/rooms");
            }}
          >
            Delete
          </Btn>
        </div>
      </PageHeader>

      <Card className="mb-6">
        <dl className="flex gap-6 text-sm">
          <div>
            <dt className="text-zinc-500 text-xs">ID</dt>
            <dd className="font-mono text-xs">{room.id}</dd>
          </div>
          <div>
            <dt className="text-zinc-500 text-xs">Created</dt>
            <dd className="text-xs">{formatTime(room.created_at)}</dd>
          </div>
          {room.meta && Object.keys(room.meta).length > 0 && (
            <div>
              <dt className="text-zinc-500 text-xs">Meta</dt>
              <dd className="font-mono text-xs">{JSON.stringify(room.meta)}</dd>
            </div>
          )}
        </dl>
      </Card>

      <div className="flex gap-1 mb-4 border-b border-zinc-800">
        {tabs.map((t) => (
          <button
            key={t}
            onClick={() => setTab(t)}
            className={`px-3 py-2 text-sm capitalize transition-colors border-b-2 -mb-px ${
              tab === t
                ? "border-blue-500 text-white"
                : "border-transparent text-zinc-400 hover:text-zinc-200"
            }`}
          >
            {t}
          </button>
        ))}
      </div>

      {tab === "members" && <MembersTab roomId={room.id} />}
      {tab === "messages" && <MessagesTab roomId={room.id} />}
      {tab === "presence" && <PresenceTab roomId={room.id} />}
      {tab === "cursors" && <CursorsTab roomId={room.id} />}
    </div>
  );
}

/* ---------- Members ---------- */

function MembersTab({ roomId }: { roomId: string }) {
  const { client } = useAuth();
  const [members, setMembers] = useState<Member[]>([]);
  const [loading, setLoading] = useState(true);
  const [showAdd, setShowAdd] = useState(false);

  function load() {
    if (!client) return;
    setLoading(true);
    client
      .listMembers(roomId)
      .then(setMembers)
      .finally(() => setLoading(false));
  }
  useEffect(load, [client, roomId]);

  if (loading) return <p className="text-zinc-500 text-sm">Loading...</p>;

  return (
    <div>
      <div className="flex justify-end mb-3">
        <Btn size="xs" onClick={() => setShowAdd(!showAdd)}>
          {showAdd ? "Cancel" : "Add Member"}
        </Btn>
      </div>

      {showAdd && <AddMemberForm roomId={roomId} onAdded={load} />}

      {members.length === 0 ? (
        <EmptyState message="No members" />
      ) : (
        <Table headers={["User", "Role", "Presence", "Joined", ""]}>
          {members.map((m) => (
            <tr key={m.user_id} className="hover:bg-zinc-800/50">
              <td className="px-4 py-2.5 font-mono text-xs">{m.user_id}</td>
              <td className="px-4 py-2.5">
                <RoleSelect
                  value={m.role}
                  onChange={async (role) => {
                    await client!.updateMember(roomId, m.user_id, role);
                    load();
                  }}
                />
              </td>
              <td className="px-4 py-2.5">
                {m.presence && (
                  <Badge color={presenceColor(m.presence)}>{m.presence}</Badge>
                )}
              </td>
              <td className="px-4 py-2.5 text-zinc-500 text-xs">
                {formatTime(m.joined_at)}
              </td>
              <td className="px-4 py-2.5 text-right">
                <Btn
                  variant="danger"
                  size="xs"
                  onClick={async () => {
                    if (!confirm(`Remove ${m.user_id}?`)) return;
                    await client!.removeMember(roomId, m.user_id);
                    load();
                  }}
                >
                  Remove
                </Btn>
              </td>
            </tr>
          ))}
        </Table>
      )}
    </div>
  );
}

function RoleSelect({
  value,
  onChange,
}: {
  value: string;
  onChange: (v: string) => void;
}) {
  return (
    <Select value={value} onChange={(e) => onChange(e.target.value)}>
      <option value="owner">owner</option>
      <option value="admin">admin</option>
      <option value="member">member</option>
    </Select>
  );
}

function AddMemberForm({
  roomId,
  onAdded,
}: {
  roomId: string;
  onAdded: () => void;
}) {
  const { client } = useAuth();
  const [userId, setUserId] = useState("");
  const [role, setRole] = useState("member");
  const [error, setError] = useState("");

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    if (!client) return;
    setError("");
    try {
      await client.addMember(roomId, userId, role);
      setUserId("");
      onAdded();
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : "Failed");
    }
  }

  return (
    <form onSubmit={handleSubmit} className="mb-4 flex gap-2 items-end">
      <div>
        <label className="block text-xs text-zinc-400 mb-1">User ID</label>
        <Input
          value={userId}
          onChange={(e) => setUserId(e.target.value)}
          required
          placeholder="alice"
        />
      </div>
      <div>
        <label className="block text-xs text-zinc-400 mb-1">Role</label>
        <Select value={role} onChange={(e) => setRole(e.target.value)}>
          <option value="member">member</option>
          <option value="admin">admin</option>
          <option value="owner">owner</option>
        </Select>
      </div>
      <Btn type="submit">Add</Btn>
      {error && <p className="text-sm text-red-400">{error}</p>}
    </form>
  );
}

/* ---------- Messages ---------- */

function MessagesTab({ roomId }: { roomId: string }) {
  const { client } = useAuth();
  const [messages, setMessages] = useState<Message[]>([]);
  const [hasMore, setHasMore] = useState(false);
  const [loading, setLoading] = useState(true);
  const [search, setSearch] = useState("");
  const [searching, setSearching] = useState(false);
  const [showSend, setShowSend] = useState(false);

  function load(before?: number) {
    if (!client) return;
    setLoading(true);
    client
      .listMessages(roomId, { before, limit: 50 })
      .then((r) => {
        const sorted = [...r.messages].reverse();
        if (before) {
          setMessages((prev) => [...prev, ...sorted]);
        } else {
          setMessages(sorted);
        }
        setHasMore(r.has_more);
      })
      .finally(() => setLoading(false));
  }

  useEffect(() => {
    load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [client, roomId]);

  async function handleSearch(e: FormEvent) {
    e.preventDefault();
    if (!client || !search.trim()) return;
    setSearching(true);
    try {
      const r = await client.searchMessages(roomId, search);
      setMessages([...r.messages].reverse());
      setHasMore(false);
    } catch {
      /* search unavailable */
    } finally {
      setSearching(false);
    }
  }

  return (
    <div>
      <div className="flex gap-2 mb-4">
        <form onSubmit={handleSearch} className="flex gap-2 flex-1">
          <Input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search messages..."
          />
          <Btn type="submit" disabled={searching}>
            {searching ? "..." : "Search"}
          </Btn>
          {search && (
            <Btn
              variant="ghost"
              type="button"
              onClick={() => {
                setSearch("");
                load();
              }}
            >
              Clear
            </Btn>
          )}
        </form>
        <Btn size="xs" onClick={() => setShowSend(!showSend)}>
          {showSend ? "Cancel" : "Send Message"}
        </Btn>
      </div>

      {showSend && (
        <SendMessageForm
          roomId={roomId}
          onSent={() => {
            load();
          }}
        />
      )}

      {loading && messages.length === 0 ? (
        <p className="text-zinc-500 text-sm">Loading...</p>
      ) : messages.length === 0 ? (
        <EmptyState message="No messages" />
      ) : (
        <div className="space-y-2">
          {messages.map((m) => (
            <div
              key={m.id}
              className={`rounded-lg border px-4 py-3 overflow-hidden ${
                m.sender === "system"
                  ? "border-zinc-800/50 bg-zinc-900/50"
                  : "border-zinc-800 bg-zinc-900"
              }`}
            >
              <div className="flex items-center gap-2 mb-1 flex-wrap">
                <span
                  className={`text-sm font-medium ${
                    m.sender === "system" ? "text-zinc-500" : "text-zinc-200"
                  }`}
                >
                  {m.sender}
                </span>
                <span className="text-xs text-zinc-600">seq {m.seq}</span>
                <span className="text-xs text-zinc-600">
                  {formatTime(m.sent_at)}
                </span>
              </div>
              {m.body && (
                <p className="text-sm text-zinc-300 break-words">{m.body}</p>
              )}
              {m.meta && Object.keys(m.meta).length > 0 && (
                <pre className="mt-1 text-xs text-zinc-500 font-mono overflow-x-auto max-w-full whitespace-pre-wrap break-all">
                  {JSON.stringify(m.meta)}
                </pre>
              )}
              <div className="mt-1">
                <span className="text-[10px] text-zinc-700 font-mono break-all">
                  {m.id}
                </span>
              </div>
            </div>
          ))}

          {hasMore && (
            <div className="text-center pt-2">
              <Btn
                variant="ghost"
                onClick={() => {
                  const last = messages[messages.length - 1];
                  if (last) load(last.seq);
                }}
                disabled={loading}
              >
                {loading ? "Loading..." : "Load More"}
              </Btn>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function SendMessageForm({
  roomId,
  onSent,
}: {
  roomId: string;
  onSent: () => void;
}) {
  const { client } = useAuth();
  const [sender, setSender] = useState("system");
  const [body, setBody] = useState("");
  const [error, setError] = useState("");

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    if (!client) return;
    setError("");
    try {
      await client.sendMessage(roomId, sender, body);
      setBody("");
      onSent();
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : "Failed");
    }
  }

  return (
    <form onSubmit={handleSubmit} className="mb-4 flex gap-2 items-end">
      <div className="w-32">
        <label className="block text-xs text-zinc-400 mb-1">Sender</label>
        <Input value={sender} onChange={(e) => setSender(e.target.value)} required />
      </div>
      <div className="flex-1">
        <label className="block text-xs text-zinc-400 mb-1">Body</label>
        <Input value={body} onChange={(e) => setBody(e.target.value)} required placeholder="Message..." />
      </div>
      <Btn type="submit">Send</Btn>
      {error && <p className="text-sm text-red-400">{error}</p>}
    </form>
  );
}

/* ---------- Presence ---------- */

function PresenceTab({ roomId }: { roomId: string }) {
  const { client } = useAuth();
  const [entries, setEntries] = useState<PresenceEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [userLookup, setUserLookup] = useState("");
  const [userResult, setUserResult] = useState<string>("");

  useEffect(() => {
    if (!client) return;
    client
      .getRoomPresence(roomId)
      .then(setEntries)
      .finally(() => setLoading(false));
  }, [client, roomId]);

  async function lookupUser(e: FormEvent) {
    e.preventDefault();
    if (!client || !userLookup) return;
    try {
      const p = await client.getUserPresence(userLookup);
      setUserResult(
        `${p.user_id}: ${p.status} (${p.connections} connections, last seen ${formatTime(p.last_seen_at)})`,
      );
    } catch {
      setUserResult("User not found");
    }
  }

  if (loading) return <p className="text-zinc-500 text-sm">Loading...</p>;

  return (
    <div>
      <form onSubmit={lookupUser} className="flex gap-2 mb-4">
        <Input
          value={userLookup}
          onChange={(e) => setUserLookup(e.target.value)}
          placeholder="Look up user presence..."
        />
        <Btn type="submit">Lookup</Btn>
      </form>
      {userResult && (
        <p className="text-sm text-zinc-300 mb-4 font-mono">{userResult}</p>
      )}

      {entries.length === 0 ? (
        <EmptyState message="No presence data" />
      ) : (
        <Table headers={["User", "Status", "Last Seen"]}>
          {entries.map((e) => (
            <tr key={e.user_id} className="hover:bg-zinc-800/50">
              <td className="px-4 py-2.5 font-mono text-xs">{e.user_id}</td>
              <td className="px-4 py-2.5">
                <Badge color={presenceColor(e.status)}>{e.status}</Badge>
              </td>
              <td className="px-4 py-2.5 text-zinc-500 text-xs">
                {e.last_seen_at ? formatTime(e.last_seen_at) : "-"}
              </td>
            </tr>
          ))}
        </Table>
      )}
    </div>
  );
}

/* ---------- Cursors ---------- */

function CursorsTab({ roomId }: { roomId: string }) {
  const { client } = useAuth();
  const [cursors, setCursors] = useState<Cursor[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    if (!client) return;
    client
      .getRoomCursors(roomId)
      .then(setCursors)
      .finally(() => setLoading(false));
  }, [client, roomId]);

  if (loading) return <p className="text-zinc-500 text-sm">Loading...</p>;

  return cursors.length === 0 ? (
    <EmptyState message="No cursor data" />
  ) : (
    <Table headers={["User", "Last Read Seq"]}>
      {cursors.map((c) => (
        <tr key={c.user_id} className="hover:bg-zinc-800/50">
          <td className="px-4 py-2.5 font-mono text-xs">{c.user_id}</td>
          <td className="px-4 py-2.5 font-mono">{c.seq}</td>
        </tr>
      ))}
    </Table>
  );
}
