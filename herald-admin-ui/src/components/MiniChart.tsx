interface DataPoint {
  t: number;
  v: number;
}

interface Props {
  data: DataPoint[];
  color?: string;
  height?: number;
  type?: "line" | "bar";
}

export default function MiniChart({ data, color = "#2dd4bf", height = 100, type = "line" }: Props) {
  if (data.length === 0) {
    return (
      <div className="flex items-center justify-center text-zinc-600 text-sm" style={{ height }}>
        No data
      </div>
    );
  }

  const w = 400;
  const h = height;
  const pad = 4;
  const maxV = Math.max(...data.map((d) => d.v), 1);

  if (type === "bar") {
    const barW = Math.max(2, (w - pad * 2) / data.length - 2);
    return (
      <svg viewBox={`0 0 ${w} ${h}`} className="w-full" preserveAspectRatio="none">
        {data.map((d, i) => {
          const barH = (d.v / maxV) * (h - pad * 2);
          const x = pad + (i / data.length) * (w - pad * 2);
          return (
            <rect
              key={i}
              x={x}
              y={h - pad - barH}
              width={barW}
              height={barH}
              fill={color}
              opacity={0.8}
            />
          );
        })}
      </svg>
    );
  }

  const points = data.map((d, i) => {
    const x = pad + (i / (data.length - 1 || 1)) * (w - pad * 2);
    const y = h - pad - (d.v / maxV) * (h - pad * 2);
    return `${x},${y}`;
  });

  const areaPoints = [...points, `${w - pad},${h - pad}`, `${pad},${h - pad}`];

  return (
    <svg viewBox={`0 0 ${w} ${h}`} className="w-full" preserveAspectRatio="none">
      <polygon points={areaPoints.join(" ")} fill={color} opacity={0.1} />
      <polyline points={points.join(" ")} fill="none" stroke={color} strokeWidth={2} />
      {data.length > 0 && (
        <circle
          cx={pad + ((data.length - 1) / (data.length - 1 || 1)) * (w - pad * 2)}
          cy={h - pad - (data[data.length - 1]!.v / maxV) * (h - pad * 2)}
          r={3}
          fill={color}
        />
      )}
    </svg>
  );
}
