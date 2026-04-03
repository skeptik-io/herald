import { useEffect, useState, type FormEvent } from "react";
import { Link } from "react-router-dom";
import { useAuth } from "../context/AuthContext";
import type { Tenant } from "../api/client";
import {
  PageHeader,
  Table,
  Btn,
  Input,
  EmptyState,
  formatTime,
} from "../components/shared";

export default function Tenants() {
  const { client } = useAuth();
  const [tenants, setTenants] = useState<Tenant[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [showCreate, setShowCreate] = useState(false);

  function load() {
    if (!client) return;
    setLoading(true);
    client
      .listTenants()
      .then(setTenants)
      .catch((e: Error) => setError(e.message))
      .finally(() => setLoading(false));
  }

  useEffect(load, [client]);

  return (
    <div>
      <PageHeader title="Tenants">
        <Btn onClick={() => setShowCreate(true)}>Create Tenant</Btn>
      </PageHeader>

      {error && <p className="text-red-400 text-sm mb-4">{error}</p>}

      {showCreate && (
        <CreateTenantForm
          onClose={() => setShowCreate(false)}
          onCreated={load}
        />
      )}

      {loading ? (
        <p className="text-zinc-500 text-sm">Loading...</p>
      ) : tenants.length === 0 ? (
        <EmptyState message="No tenants yet" />
      ) : (
        <Table headers={["ID", "Name", "Plan", "Created", ""]}>
          {tenants.map((t) => (
            <tr key={t.id} className="hover:bg-zinc-800/50">
              <td className="px-4 py-2.5 font-mono text-xs">
                <Link to={`/tenants/${t.id}`} className="text-blue-400 hover:underline">
                  {t.id}
                </Link>
              </td>
              <td className="px-4 py-2.5">{t.name}</td>
              <td className="px-4 py-2.5 text-zinc-400">{t.plan ?? "-"}</td>
              <td className="px-4 py-2.5 text-zinc-500 text-xs">
                {formatTime(t.created_at)}
              </td>
              <td className="px-4 py-2.5 text-right">
                <Btn
                  variant="danger"
                  size="xs"
                  onClick={async () => {
                    if (!confirm(`Delete tenant "${t.id}"?`)) return;
                    await client!.deleteTenant(t.id);
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

function CreateTenantForm({
  onClose,
  onCreated,
}: {
  onClose: () => void;
  onCreated: () => void;
}) {
  const { client } = useAuth();
  const [id, setId] = useState("");
  const [name, setName] = useState("");
  const [jwtSecret, setJwtSecret] = useState("");
  const [error, setError] = useState("");
  const [saving, setSaving] = useState(false);

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    if (!client) return;
    setSaving(true);
    setError("");
    try {
      await client.createTenant({ id, name, jwt_secret: jwtSecret });
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
      <h3 className="text-sm font-medium mb-3">New Tenant</h3>
      <form onSubmit={handleSubmit} className="space-y-3">
        <div className="grid grid-cols-3 gap-3">
          <div>
            <label className="block text-xs text-zinc-400 mb-1">ID</label>
            <Input value={id} onChange={(e) => setId(e.target.value)} required placeholder="acme" />
          </div>
          <div>
            <label className="block text-xs text-zinc-400 mb-1">Name</label>
            <Input value={name} onChange={(e) => setName(e.target.value)} required placeholder="Acme Corp" />
          </div>
          <div>
            <label className="block text-xs text-zinc-400 mb-1">JWT Secret</label>
            <Input
              type="password"
              value={jwtSecret}
              onChange={(e) => setJwtSecret(e.target.value)}
              required
              placeholder="HMAC-SHA256 secret"
            />
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
