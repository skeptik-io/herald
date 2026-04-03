import { type ReactNode, type ButtonHTMLAttributes } from "react";

export function PageHeader({
  title,
  children,
}: {
  title: string;
  children?: ReactNode;
}) {
  return (
    <div className="flex items-center justify-between mb-6">
      <h2 className="text-xl font-semibold">{title}</h2>
      {children}
    </div>
  );
}

export function Card({ children, className = "" }: { children: ReactNode; className?: string }) {
  return (
    <div className={`rounded-lg border border-zinc-800 bg-zinc-900 p-4 ${className}`}>
      {children}
    </div>
  );
}

export function Stat({ label, value }: { label: string; value: string | number }) {
  return (
    <Card>
      <p className="text-xs text-zinc-500 uppercase tracking-wider">{label}</p>
      <p className="mt-1 text-2xl font-semibold">{value}</p>
    </Card>
  );
}

export function Badge({ children, color = "zinc" }: { children: ReactNode; color?: string }) {
  const colors: Record<string, string> = {
    green: "bg-green-900/50 text-green-400 border-green-800",
    yellow: "bg-yellow-900/50 text-yellow-400 border-yellow-800",
    red: "bg-red-900/50 text-red-400 border-red-800",
    blue: "bg-blue-900/50 text-blue-400 border-blue-800",
    zinc: "bg-zinc-800 text-zinc-400 border-zinc-700",
  };
  return (
    <span
      className={`inline-block px-2 py-0.5 text-xs rounded border ${colors[color] ?? colors.zinc}`}
    >
      {children}
    </span>
  );
}

export function Btn({
  variant = "primary",
  size = "sm",
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "danger" | "ghost";
  size?: "sm" | "xs";
}) {
  const base = "rounded-md font-medium transition-colors disabled:opacity-50";
  const sizes = {
    sm: "px-3 py-1.5 text-sm",
    xs: "px-2 py-1 text-xs",
  };
  const variants = {
    primary: "bg-blue-600 hover:bg-blue-500 text-white",
    danger: "bg-red-600 hover:bg-red-500 text-white",
    ghost: "text-zinc-400 hover:text-white hover:bg-zinc-800",
  };
  return (
    <button
      {...props}
      className={`${base} ${sizes[size]} ${variants[variant]} ${props.className ?? ""}`}
    />
  );
}

export function Input(props: React.InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      {...props}
      className={`w-full rounded-md border border-zinc-700 bg-zinc-800 px-3 py-1.5 text-sm text-zinc-100 placeholder:text-zinc-500 focus:outline-none focus:ring-1 focus:ring-blue-500 ${props.className ?? ""}`}
    />
  );
}

export function Select(props: React.SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <select
      {...props}
      className={`rounded-md border border-zinc-700 bg-zinc-800 px-3 py-1.5 text-sm text-zinc-100 focus:outline-none focus:ring-1 focus:ring-blue-500 ${props.className ?? ""}`}
    />
  );
}

export function Table({
  headers,
  children,
}: {
  headers: string[];
  children: ReactNode;
}) {
  return (
    <div className="overflow-x-auto rounded-lg border border-zinc-800">
      <table className="w-full text-sm">
        <thead className="bg-zinc-900 text-zinc-400 text-left">
          <tr>
            {headers.map((h) => (
              <th key={h} className="px-4 py-2.5 font-medium">
                {h}
              </th>
            ))}
          </tr>
        </thead>
        <tbody className="divide-y divide-zinc-800">{children}</tbody>
      </table>
    </div>
  );
}

export function EmptyState({ message }: { message: string }) {
  return (
    <div className="text-center py-12 text-zinc-500 text-sm">{message}</div>
  );
}

export function formatTime(ms: number) {
  return new Date(ms).toLocaleString();
}

export function formatUptime(secs: number) {
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (d > 0) return `${d}d ${h}h ${m}m`;
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}

export function presenceColor(status?: string) {
  switch (status) {
    case "online":
      return "green";
    case "away":
      return "yellow";
    case "dnd":
      return "red";
    default:
      return "zinc";
  }
}
