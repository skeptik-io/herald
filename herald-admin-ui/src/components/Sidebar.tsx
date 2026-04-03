import { NavLink } from "react-router-dom";
import { useAuth } from "../context/AuthContext";

const adminLinks = [
  { to: "/", label: "Overview" },
  { to: "/stats", label: "Stats" },
  { to: "/console", label: "Debug Console" },
  { to: "/errors", label: "Error Logs" },
  { to: "/tenants", label: "Tenants" },
  { to: "/rooms", label: "Rooms" },
];

const tenantLinks = [
  { to: "/", label: "Overview" },
  { to: "/rooms", label: "Rooms" },
];

export default function Sidebar() {
  const { auth, logout } = useAuth();
  const links = auth?.mode === "admin" ? adminLinks : tenantLinks;

  return (
    <aside className="w-52 shrink-0 border-r border-zinc-800 bg-zinc-900 flex flex-col">
      <div className="px-4 py-5 border-b border-zinc-800">
        <h1 className="text-base font-semibold tracking-tight">Herald</h1>
      </div>

      <nav className="flex-1 px-2 py-3 space-y-0.5">
        {links.map((l) => (
          <NavLink
            key={l.to}
            to={l.to}
            end={l.to === "/"}
            className={({ isActive }) =>
              `block px-3 py-1.5 rounded text-[13px] transition-colors ${
                isActive
                  ? "bg-zinc-800 text-white font-medium"
                  : "text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800/50"
              }`
            }
          >
            {l.label}
          </NavLink>
        ))}
      </nav>

      <div className="px-2 py-3 border-t border-zinc-800">
        <div className="px-3 mb-2 text-[11px] text-zinc-600 uppercase tracking-wider">
          {auth?.mode === "admin" ? "Super Admin" : "Tenant API"}
        </div>
        <button
          onClick={logout}
          className="w-full px-3 py-1.5 text-[13px] text-zinc-400 hover:text-white hover:bg-zinc-800/50 rounded text-left transition-colors"
        >
          Sign out
        </button>
      </div>
    </aside>
  );
}
