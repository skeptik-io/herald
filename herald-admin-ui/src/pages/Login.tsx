import { useState, type FormEvent } from "react";
import { useAuth } from "../context/AuthContext";

export default function Login() {
  const { login } = useAuth();
  const [token, setToken] = useState("");
  const [mode, setMode] = useState<"admin" | "tenant">("admin");
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    setError("");
    setLoading(true);

    try {
      const baseUrl = import.meta.env.VITE_HERALD_URL || "/api";
      const res = await fetch(`${baseUrl}/health`, {
        headers: { Authorization: `Bearer ${token}` },
      });
      if (!res.ok) {
        setError("Failed to connect. Check your token.");
        return;
      }
      login(token, mode);
    } catch {
      setError("Cannot reach Herald server.");
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="min-h-screen flex items-center justify-center bg-zinc-950">
      <div className="w-full max-w-sm">
        <h1 className="text-2xl font-bold mb-1">Herald Admin</h1>
        <p className="text-sm text-zinc-500 mb-6">
          Sign in with your Herald API token
        </p>

        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <label className="block text-xs text-zinc-400 mb-1">Mode</label>
            <div className="flex gap-2">
              {(["admin", "tenant"] as const).map((m) => (
                <button
                  key={m}
                  type="button"
                  onClick={() => setMode(m)}
                  className={`flex-1 px-3 py-2 text-sm rounded-md border transition-colors ${
                    mode === m
                      ? "border-blue-500 bg-blue-600/20 text-blue-400"
                      : "border-zinc-700 bg-zinc-800 text-zinc-400 hover:border-zinc-600"
                  }`}
                >
                  {m === "admin" ? "Super Admin" : "Tenant API"}
                </button>
              ))}
            </div>
          </div>

          <div>
            <label className="block text-xs text-zinc-400 mb-1">
              {mode === "admin" ? "Super Admin Token" : "Tenant API Token"}
            </label>
            <input
              type="password"
              value={token}
              onChange={(e) => setToken(e.target.value)}
              placeholder="Bearer token"
              required
              className="w-full rounded-md border border-zinc-700 bg-zinc-800 px-3 py-2 text-sm text-zinc-100 placeholder:text-zinc-500 focus:outline-none focus:ring-1 focus:ring-blue-500"
            />
          </div>

          {error && (
            <p className="text-sm text-red-400">{error}</p>
          )}

          <button
            type="submit"
            disabled={loading || !token}
            className="w-full rounded-md bg-blue-600 hover:bg-blue-500 px-4 py-2 text-sm font-medium text-white transition-colors disabled:opacity-50"
          >
            {loading ? "Connecting..." : "Sign in"}
          </button>
        </form>
      </div>
    </div>
  );
}
