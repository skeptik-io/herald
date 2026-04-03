import {
  ResponsiveContainer,
  LineChart,
  BarChart,
  Line,
  Bar,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
} from "recharts";

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

function formatTime(ms: number) {
  return new Date(ms).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
}

export default function MiniChart({
  data,
  color = "#2dd4bf",
  height = 150,
  type = "line",
}: Props) {
  if (data.length === 0) {
    return (
      <div
        className="flex items-center justify-center text-zinc-600 text-sm"
        style={{ height }}
      >
        No data
      </div>
    );
  }

  const chartData = data.map((d) => ({ time: d.t, value: d.v }));

  const common = {
    data: chartData,
    margin: { top: 5, right: 5, bottom: 5, left: 0 },
  };

  const xAxis = (
    <XAxis
      dataKey="time"
      tickFormatter={formatTime}
      stroke="#52525b"
      fontSize={11}
      tickLine={false}
      axisLine={false}
    />
  );

  const yAxis = (
    <YAxis
      stroke="#52525b"
      fontSize={11}
      tickLine={false}
      axisLine={false}
      width={35}
    />
  );

  const grid = <CartesianGrid strokeDasharray="3 3" stroke="#27272a" />;

  const tooltip = (
    <Tooltip
      contentStyle={{
        background: "#18181b",
        border: "1px solid #3f3f46",
        borderRadius: 6,
        fontSize: 12,
      }}
      labelFormatter={(label) => formatTime(Number(label))}
      formatter={(value) => [Number(value).toLocaleString(), ""]}
    />
  );

  if (type === "bar") {
    return (
      <ResponsiveContainer width="100%" height={height}>
        <BarChart {...common}>
          {grid}
          {xAxis}
          {yAxis}
          {tooltip}
          <Bar dataKey="value" fill={color} opacity={0.8} radius={[2, 2, 0, 0]} />
        </BarChart>
      </ResponsiveContainer>
    );
  }

  return (
    <ResponsiveContainer width="100%" height={height}>
      <LineChart {...common}>
        {grid}
        {xAxis}
        {yAxis}
        {tooltip}
        <Line
          type="monotone"
          dataKey="value"
          stroke={color}
          strokeWidth={2}
          dot={{ r: 3, fill: color }}
          activeDot={{ r: 5 }}
        />
      </LineChart>
    </ResponsiveContainer>
  );
}
