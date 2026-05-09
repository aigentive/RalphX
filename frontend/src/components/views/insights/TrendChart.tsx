import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
} from "recharts";
import type { WeeklyDataPoint } from "@/types/project-stats";

const ACCENT = "var(--accent-primary)";
const SECONDARY_DEFAULT = "var(--text-muted)";

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

interface TrendChartProps {
  title: string;
  data: WeeklyDataPoint[];
  valueFormatter?: (v: number) => string;
  color?: string;
  currentValue?: string;
  timeWindow?: string;
  secondaryData?: WeeklyDataPoint[];
  secondaryLabel?: string;
  secondaryColor?: string;
  primaryLabel?: string;
  secondaryValueFormatter?: (v: number) => string;
}

export function TrendChart({
  title,
  data,
  valueFormatter,
  color = ACCENT,
  currentValue,
  timeWindow,
  secondaryData,
  secondaryLabel,
  secondaryColor = SECONDARY_DEFAULT,
  primaryLabel,
  secondaryValueFormatter,
}: TrendChartProps) {
  const hasSecondary = secondaryData !== undefined && secondaryData.length > 0;
  const secondaryMap = new Map(
    hasSecondary ? secondaryData.map((pt) => [formatWeek(pt.weekStart), pt.value]) : [],
  );

  const chartData = data.map((pt) => {
    const week = formatWeek(pt.weekStart);
    const point: Record<string, string | number> = { week, value: pt.value };
    if (hasSecondary) {
      const sec = secondaryMap.get(week);
      if (sec !== undefined) point.secondary = sec;
    }
    return point;
  });

  const fmt = valueFormatter ?? ((v: number) => String(v));
  const fmtSecondary = secondaryValueFormatter ?? ((v: number) => String(v));

  const header = (
    <div className={timeWindow !== undefined ? "mb-2" : "mb-3"}>
      <div className="flex items-center justify-between">
        <p className="text-[0.75rem] font-medium text-text-secondary">
          {title}
        </p>
        {currentValue !== undefined && (
          <span className="text-[0.75rem] text-text-secondary">
            {currentValue}
          </span>
        )}
      </div>
      {timeWindow !== undefined && (
        <p className="text-[0.625rem] mt-0.5 text-text-muted">
          {timeWindow}
        </p>
      )}
    </div>
  );

  if (chartData.length === 0) {
    return (
      <div>
        {header}
        <p className="text-[0.75rem] text-text-muted">
          No data yet
        </p>
      </div>
    );
  }

  const pLabel = primaryLabel ?? title;
  const sLabel = secondaryLabel ?? "Secondary";

  return (
    <div>
      {header}
      <ResponsiveContainer width="100%" height={160}>
        <LineChart data={chartData} margin={{ top: 4, right: 4, left: -20, bottom: 0 }}>
          <CartesianGrid
            strokeDasharray="3 3"
            stroke="var(--overlay-weak)"
            vertical={false}
          />
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
            tickFormatter={fmt}
          />
          <Tooltip
            contentStyle={tooltipStyle}
            formatter={(val, name) => {
              if (name === "secondary") {
                return typeof val === "number"
                  ? [fmtSecondary(val), sLabel]
                  : [String(val), sLabel];
              }
              return typeof val === "number"
                ? [fmt(val), pLabel]
                : [String(val), pLabel];
            }}
            labelStyle={{ color: "var(--text-secondary)" }}
          />
          <Line
            type="monotone"
            dataKey="value"
            stroke={color}
            strokeWidth={2}
            dot={false}
            activeDot={{ r: 4, fill: color }}
          />
          {hasSecondary && (
            <Line
              type="monotone"
              dataKey="secondary"
              stroke={secondaryColor}
              strokeWidth={1.5}
              dot={false}
              activeDot={{ r: 3, fill: secondaryColor }}
              strokeDasharray="4 3"
            />
          )}
        </LineChart>
      </ResponsiveContainer>
      {hasSecondary && (
        <div className="w-full flex items-center justify-center gap-4 mt-2">
          <span className="flex items-center gap-1.5">
            <span
              className="inline-block w-[8px] h-[8px] rounded-full"
              style={{ backgroundColor: color }}
            />
            <span className="text-[0.75rem] text-text-secondary">
              {pLabel}
            </span>
          </span>
          <span className="flex items-center gap-1.5">
            <span
              className="inline-block w-[8px] h-[8px] rounded-full"
              style={{ backgroundColor: secondaryColor }}
            />
            <span className="text-[0.75rem] text-text-secondary">
              {sLabel}
            </span>
          </span>
        </div>
      )}
    </div>
  );
}
