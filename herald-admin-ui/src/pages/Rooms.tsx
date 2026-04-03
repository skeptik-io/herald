import { useEffect, useState, type FormEvent } from "react";
import { Link } from "react-router-dom";
import { useAuth } from "../context/AuthContext";
import type { Room } from "../api/client";
import {
  PageHeader,
  Table,
  Btn,
  Input,
  EmptyState,
  formatTime,
} from "../components/shared";

export default function Rooms() {
  const { client } = useAuth();
  const [rooms, setRooms] = useState<Room[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [showCreate, setShowCreate] = useState(false);

  function load() {
    if (!client) return;
    setLoading(true);
    client
      .listRooms()
      .then(setRooms)
      .catch((e: Error) => setError(e.message))
      .finally(() => setLoading(false));
  }

  useEffect(load, [client]);

  return (
    <div>
      <PageHeader title="Rooms">
        <Btn onClick={() => setShowCreate(true)}>Create Room</Btn>
      </PageHeader>

      {error && <p className="text-red-400 text-sm mb-4">{error}</p>}

      {showCreate && (
        <CreateRoomForm
          onClose={() => setShowCreate(false)}
          onCreated={load}
        />
      )}

      {loading ? (
        <p className="text-zinc-500 text-sm">Loading...</p>
      ) : rooms.length === 0 ? (
        <EmptyState message="No rooms yet" />
      ) : (
        <Table headers={["ID", "Name", "Created", ""]}>
          {rooms.map((r) => (
            <tr key={r.id} className="hover:bg-zinc-800/50">
              <td className="px-4 py-2.5 font-mono text-xs">
                <Link to={`/rooms/${r.id}`} className="text-blue-400 hover:underline">
                  {r.id}
                </Link>
              </td>
              <td className="px-4 py-2.5">{r.name}</td>
              <td className="px-4 py-2.5 text-zinc-500 text-xs">
                {formatTime(r.created_at)}
              </td>
              <td className="px-4 py-2.5 text-right">
                <Btn
                  variant="danger"
                  size="xs"
                  onClick={async () => {
                    if (!confirm(`Delete room "${r.id}"?`)) return;
                    await client!.deleteRoom(r.id);
                    load();
                  }}
                >
                  Delete
                </Btn>
              </td>
            </tr>
          ))}
        </Table>
      )}
    </div>
  );
}

function CreateRoomForm({
  onClose,
  onCreated,
}: {
  onClose: () => void;
  onCreated: () => void;
}) {
  const { client } = useAuth();
  const [id, setId] = useState("");
  const [name, setName] = useState("");
  const [error, setError] = useState("");
  const [saving, setSaving] = useState(false);

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    if (!client) return;
    setSaving(true);
    setError("");
    try {
      await client.createRoom({
        id,
        name,
      });
      onCreated();
      onClose();
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : "Failed to create");
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="mb-6 rounded-lg border border-zinc-800 bg-zinc-900 p-4">
      <h3 className="text-sm font-medium mb-3">New Room</h3>
      <form onSubmit={handleSubmit} className="space-y-3">
        <div className="grid grid-cols-2 gap-3">
          <div>
            <label className="block text-xs text-zinc-400 mb-1">ID</label>
            <Input value={id} onChange={(e) => setId(e.target.value)} required placeholder="general" />
          </div>
          <div>
            <label className="block text-xs text-zinc-400 mb-1">Name</label>
            <Input value={name} onChange={(e) => setName(e.target.value)} required placeholder="General Chat" />
          </div>
        </div>
        {error && <p className="text-sm text-red-400">{error}</p>}
        <div className="flex gap-2">
          <Btn type="submit" disabled={saving}>
            {saving ? "Creating..." : "Create"}
          </Btn>
          <Btn variant="ghost" onClick={onClose} type="button">
            Cancel
          </Btn>
        </div>
      </form>
    </div>
  );
}
