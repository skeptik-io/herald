import { useEffect, useState, type FormEvent } from "react";
import { useParams, useNavigate, Link } from "react-router-dom";
import { useAuth } from "../context/AuthContext";
import type { Tenant, Room } from "../api/client";
import { PageHeader, Card, Table, Btn, Input, EmptyState, formatTime } from "../components/shared";

export default function TenantDetail() {
  const { id } = useParams<{ id: string }>();
  const { client } = useAuth();
  const navigate = useNavigate();
  const [tenant, setTenant] = useState<Tenant | null>(null);
  const [tokens, setTokens] = useState<string[]>([]);
  const [rooms, setRooms] = useState<Room[]>([]);
  const [error, setError] = useState("");
  const [editing, setEditing] = useState(false);

  useEffect(() => {
    if (!client || !id) return;
    client.getTenant(id).then(setTenant).catch((e: Error) => setError(e.message));
    client.listTenantTokens(id).then(setTokens).catch(() => {});
    client.listTenantRooms(id).then(setRooms).catch(() => {});
  }, [client, id]);

  if (error) return <p className="text-red-400">{error}</p>;
  if (!tenant) return <p className="text-zinc-500 text-sm">Loading...</p>;

  return (
    <div>
      <PageHeader title={tenant.name}>
        <div className="flex gap-2">
          <Btn variant="ghost" onClick={() => setEditing(!editing)}>
            {editing ? "Cancel" : "Edit"}
          </Btn>
          <Btn
            variant="danger"
            onClick={async () => {
              if (!confirm(`Delete tenant "${tenant.id}"?`)) return;
              await client!.deleteTenant(tenant.id);
              navigate("/tenants");
            }}
          >
            Delete
          </Btn>
        </div>
      </PageHeader>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-4 mb-6">
        <Card>
          <dl className="space-y-2 text-sm">
            <Row label="ID" value={tenant.id} mono />
            <Row label="Name" value={tenant.name} />
            <Row label="Plan" value={tenant.plan ?? "-"} />
            <Row label="Created" value={formatTime(tenant.created_at)} />
          </dl>
        </Card>

        {editing && (
          <EditForm
            tenant={tenant}
            onSaved={(t) => {
              setTenant(t);
              setEditing(false);
            }}
          />
        )}
      </div>

      <div className="mb-6">
        <div className="flex items-center justify-between mb-3">
          <h3 className="text-sm font-medium text-zinc-300">API Tokens</h3>
          <Btn
            size="xs"
            onClick={async () => {
              if (!client || !id) return;
              const r = await client.createTenantToken(id);
              setTokens((prev) => [...prev, r.token]);
            }}
          >
            Generate Token
          </Btn>
        </div>

        {tokens.length === 0 ? (
          <EmptyState message="No API tokens" />
        ) : (
          <Table headers={["Token", ""]}>
            {tokens.map((t) => (
              <tr key={t} className="hover:bg-zinc-800/50">
                <td className="px-4 py-2.5 font-mono text-xs">
                  <code className="select-all">{t}</code>
                </td>
                <td className="px-4 py-2.5 text-right">
                  <Btn
                    variant="danger"
                    size="xs"
                    onClick={async () => {
                      if (!confirm("Revoke this token?")) return;
                      await client!.deleteTenantToken(id!, t);
                      setTokens((prev) => prev.filter((x) => x !== t));
                    }}
                  >
                    Revoke
                  </Btn>
                </td>
              </tr>
            ))}
          </Table>
        )}
      </div>

      <div>
        <h3 className="text-sm font-medium text-zinc-300 mb-3">
          Rooms ({rooms.length})
        </h3>
        {rooms.length === 0 ? (
          <EmptyState message="No rooms for this tenant" />
        ) : (
          <Table headers={["ID", "Name", "Created"]}>
            {rooms.map((r) => (
              <tr key={r.id} className="hover:bg-zinc-800/50">
                <td className="px-4 py-2.5 font-mono text-xs">
                  <Link to={`/rooms/${r.id}`} className="text-blue-400 hover:underline">
                    {r.id}
                  </Link>
                </td>
                <td className="px-4 py-2.5">{r.name}</td>
                <td className="px-4 py-2.5 text-zinc-500 text-xs">{formatTime(r.created_at)}</td>
              </tr>
            ))}
          </Table>
        )}
      </div>
    </div>
  );
}

function Row({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="flex justify-between">
      <dt className="text-zinc-400">{label}</dt>
      <dd className={mono ? "font-mono text-xs" : ""}>{value}</dd>
    </div>
  );
}

function EditForm({ tenant, onSaved }: { tenant: Tenant; onSaved: (t: Tenant) => void }) {
  const { client } = useAuth();
  const [name, setName] = useState(tenant.name);
  const [plan, setPlan] = useState(tenant.plan ?? "");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    if (!client) return;
    setSaving(true);
    setError("");
    try {
      const updated = await client.updateTenant(tenant.id, { name, plan: plan || undefined });
      onSaved(updated);
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : "Failed to update");
    } finally {
      setSaving(false);
    }
  }

  return (
    <Card>
      <h3 className="text-sm font-medium mb-3">Edit Tenant</h3>
      <form onSubmit={handleSubmit} className="space-y-3">
        <div>
          <label className="block text-xs text-zinc-400 mb-1">Name</label>
          <Input value={name} onChange={(e) => setName(e.target.value)} required />
        </div>
        <div>
          <label className="block text-xs text-zinc-400 mb-1">Plan</label>
          <Input value={plan} onChange={(e) => setPlan(e.target.value)} placeholder="free" />
        </div>
        {error && <p className="text-sm text-red-400">{error}</p>}
        <Btn type="submit" disabled={saving}>
          {saving ? "Saving..." : "Save"}
        </Btn>
      </form>
    </Card>
  );
}
