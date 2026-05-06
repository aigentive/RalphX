import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";

import { TrendChart } from "./TrendChart";
import type { WeeklyDataPoint } from "@/types/project-stats";

// Recharts uses ResizeObserver and SVG width measurements that jsdom doesn't
// provide; shimming here keeps the chart container from collapsing to 0×0.
class ResizeObserverShim {
  observe() {}
  unobserve() {}
  disconnect() {}
}
// @ts-expect-error - jsdom fallback for recharts
window.ResizeObserver = ResizeObserverShim;

const data: WeeklyDataPoint[] = [
  { weekStart: "2026-04-07T00:00:00Z", value: 4, sampleSize: 5 },
  { weekStart: "2026-04-14T00:00:00Z", value: 7, sampleSize: 6 },
  { weekStart: "2026-04-21T00:00:00Z", value: 5, sampleSize: 4 },
];

describe("TrendChart", () => {
  it("renders the title and current value", () => {
    render(
      <TrendChart
        title="Throughput"
        data={data}
        currentValue="5 / week"
        timeWindow="last 4 weeks"
        valueFormatter={(v) => `${v}`}
      />,
    );
    expect(screen.getByText("Throughput")).toBeInTheDocument();
    expect(screen.getByText("5 / week")).toBeInTheDocument();
    expect(screen.getByText("last 4 weeks")).toBeInTheDocument();
  });

  it("renders the secondary series when secondaryData is provided", () => {
    const secondary: WeeklyDataPoint[] = [
      { weekStart: "2026-04-07T00:00:00Z", value: 1, sampleSize: 1 },
      { weekStart: "2026-04-14T00:00:00Z", value: 2, sampleSize: 1 },
    ];
    render(
      <TrendChart
        title="Cycle time"
        data={data}
        secondaryData={secondary}
        primaryLabel="Cycle"
        secondaryLabel="Lead"
        secondaryValueFormatter={vi.fn((v) => `${v}h`)}
      />,
    );
    expect(screen.getByText("Cycle time")).toBeInTheDocument();
    expect(screen.getByText(/Lead/i)).toBeInTheDocument();
  });
});
