import {
  createContext,
  useContext,
  useState,
  useCallback,
  useMemo,
  type ReactNode,
} from "react";
import { HeraldClient } from "../api/client";

interface Auth {
  token: string;
  mode: "admin" | "tenant";
}

interface AuthContextValue {
  auth: Auth | null;
  client: HeraldClient | null;
  login: (token: string, mode: "admin" | "tenant") => void;
  logout: () => void;
  baseUrl: string;
}

const AuthContext = createContext<AuthContextValue | null>(null);

declare global {
  interface Window {
    __HERALD_URL__?: string;
  }
}

const STORAGE_KEY = "herald_admin_auth";
const BASE_URL =
  window.__HERALD_URL__ || import.meta.env.VITE_HERALD_URL || "http://localhost:6201";

function loadAuth(): Auth | null {
  try {
    const raw = sessionStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    return JSON.parse(raw);
  } catch {
    return null;
  }
}

export function AuthProvider({ children }: { children: ReactNode }) {
  const [auth, setAuth] = useState<Auth | null>(loadAuth);

  const client = useMemo(
    () => (auth ? new HeraldClient(BASE_URL, auth.token) : null),
    [auth],
  );

  const login = useCallback((token: string, mode: "admin" | "tenant") => {
    const a: Auth = { token, mode };
    sessionStorage.setItem(STORAGE_KEY, JSON.stringify(a));
    setAuth(a);
  }, []);

  const logout = useCallback(() => {
    sessionStorage.removeItem(STORAGE_KEY);
    setAuth(null);
  }, []);

  return (
    <AuthContext.Provider value={{ auth, client, login, logout, baseUrl: BASE_URL }}>
      {children}
    </AuthContext.Provider>
  );
}

export function useAuth() {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error("useAuth must be inside AuthProvider");
  return ctx;
}
