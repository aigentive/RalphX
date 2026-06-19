import {
  CartesianGrid,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import type { DeliveryWeeklyThroughputPoint } from "@/types/project-stats";

const tooltipStyle = {
  backgroundColor: "var(--bg-surface)",
  border: "1px solid var(--overlay-faint)",
  borderRadius: "8px",
  fontSize: "0.75rem",
  color: "var(--text-primary)",
};

function formatWeek(weekStart: string): string {
  const date = new Date(weekStart + "T00:00:00");
  return date.toLocaleDateString("en-US", { month: "short", day: "numeric" });
}

interface DeliveryThroughputChartProps {
  data: DeliveryWeeklyThroughputPoint[];
  currentValue?: string;
}

export function DeliveryThroughputChart({ data, currentValue }: DeliveryThroughputChartProps) {
  const chartData = data.map((point) => ({
    week: formatWeek(point.weekStart),
    unified: point.unifiedDeliveries,
    tasks: point.taskDeliveries,
    workspaces: point.workspaceDeliveries,
    mergedPrs: point.mergedPrs,
  }));

  const header = (
    <div className="mb-2">
      <div className="flex items-center justify-between">
        <p className="text-[0.75rem] font-medium text-text-secondary">
          Weekly Delivery Throughput
        </p>
        {currentValue !== undefined && (
          <span className="text-[0.75rem] text-text-secondary">{currentValue}</span>
        )}
      </div>
      <p className="text-[0.625rem] mt-0.5 text-text-muted">Last 12 weeks</p>
    </div>
  );

  if (chartData.length === 0) {
    return (
      <div>
        {header}
        <p className="text-[0.75rem] text-text-muted">No data yet</p>
      </div>
    );
  }

  return (
    <div>
      {header}
      <ResponsiveContainer width="100%" height={160}>
        <LineChart data={chartData} margin={{ top: 4, right: 4, left: -20, bottom: 0 }}>
          <CartesianGrid strokeDasharray="3 3" stroke="var(--overlay-weak)" vertical={false} />
          <XAxis
            dataKey="week"
            tick={{ fontSize: 11, fill: "var(--text-muted)" }}
            axisLine={false}
            tickLine={false}
          />
          <YAxis
            tick={{ fontSize: 11, fill: "var(--text-muted)" }}
            axisLine={false}
            tickLine={false}
            allowDecimals={false}
          />
          <Tooltip
            contentStyle={tooltipStyle}
            formatter={(val, name) => {
              const label =
                name === "tasks"
                  ? "Tasks"
                  : name === "workspaces"
                    ? "Agent workspaces"
                    : name === "mergedPrs"
                      ? "Merged PRs"
                      : "Unified deliveries";
              return [typeof val === "number" ? String(val) : String(val), label];
            }}
            labelStyle={{ color: "var(--text-secondary)" }}
          />
          <Line
            type="monotone"
            dataKey="unified"
            stroke="var(--accent-primary)"
            strokeWidth={2}
            dot={false}
            activeDot={{ r: 4, fill: "var(--accent-primary)" }}
          />
          <Line
            type="monotone"
            dataKey="tasks"
            stroke="var(--status-success)"
            strokeWidth={1.5}
            dot={false}
            activeDot={{ r: 3, fill: "var(--status-success)" }}
          />
          <Line
            type="monotone"
            dataKey="workspaces"
            stroke="var(--status-warning)"
            strokeWidth={1.5}
            dot={false}
            activeDot={{ r: 3, fill: "var(--status-warning)" }}
          />
          <Line
            type="monotone"
            dataKey="mergedPrs"
            stroke="var(--text-muted)"
            strokeWidth={1.5}
            strokeDasharray="4 3"
            dot={false}
            activeDot={{ r: 3, fill: "var(--text-muted)" }}
          />
        </LineChart>
      </ResponsiveContainer>
      <div className="w-full flex flex-wrap items-center justify-center gap-3 mt-2">
        {[
          ["Unified", "var(--accent-primary)"],
          ["Tasks", "var(--status-success)"],
          ["Workspaces", "var(--status-warning)"],
          ["Merged PRs", "var(--text-muted)"],
        ].map(([label, color]) => (
          <span key={label} className="flex items-center gap-1.5">
            <span
              className="inline-block w-[8px] h-[8px] rounded-full"
              style={{ backgroundColor: color }}
            />
            <span className="text-[0.75rem] text-text-secondary">{label}</span>
          </span>
        ))}
      </div>
    </div>
  );
}
