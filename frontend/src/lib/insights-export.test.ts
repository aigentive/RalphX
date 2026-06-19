import { describe, it, expect } from "vitest";
import {
  formatCSV,
  formatJSONExport,
  shouldShowTrends,
  shouldShowEme,
  MIN_TASKS_FOR_TRENDS,
  MIN_TASKS_FOR_EME,
} from "./insights-export";
import type { ProjectStats, ProjectTrends } from "@/types/project-stats";

// ─── Fixtures ─────────────────────────────────────────────────────────────────

const emptyTrends: ProjectTrends = {
  weeklyThroughput: [],
  weeklyDeliveryThroughput: [],
  weeklyCycleTime: [],
  weeklyPipelineCycleTime: [],
  weeklySuccessRate: [],
};

const sampleTrends: ProjectTrends = {
  weeklyThroughput: [
    { weekStart: "2026-01-05", value: 3, sampleSize: 3 },
      { weekStart: "2026-01-12", value: 5, sampleSize: 5 },
  ],
  weeklyDeliveryThroughput: [
    {
      weekStart: "2026-01-05",
      unifiedDeliveries: 4,
      taskDeliveries: 3,
      workspaceDeliveries: 1,
      mergedPrs: 2,
      sampleSize: 6,
    },
    {
      weekStart: "2026-01-12",
      unifiedDeliveries: 6,
      taskDeliveries: 5,
      workspaceDeliveries: 1,
      mergedPrs: 3,
      sampleSize: 9,
    },
  ],
  weeklyCycleTime: [
    { weekStart: "2026-01-05", value: 120, sampleSize: 3 }, // 120 min → 2.00 h
    { weekStart: "2026-01-12", value: 90, sampleSize: 5 },  // 90 min → 1.50 h
  ],
  weeklyPipelineCycleTime: [],
  weeklySuccessRate: [
    { weekStart: "2026-01-05", value: 75.5, sampleSize: 3 },
    { weekStart: "2026-01-12", value: 80, sampleSize: 5 },
  ],
};

const baseStats: ProjectStats = {
  taskCount: 12,
  tasksCompletedToday: 1,
  tasksCompletedThisWeek: 5,
  tasksCompletedThisMonth: 12,
  agentSuccessRate: 0.75,
  agentSuccessCount: 9,
  agentTotalCount: 12,
  reviewPassRate: 0.8,
  reviewPassCount: 8,
  reviewTotalCount: 10,
  cycleTimeBreakdown: [],
  columnDwellTimes: [],
  avgPipelineMinutes: null,
  eme: null,
};

const CSV_HEADER =
  "week_start,throughput,unified_deliveries,task_deliveries,workspace_deliveries,merged_prs,cycle_time_hours,success_rate_pct";

// ─── formatCSV ────────────────────────────────────────────────────────────────

describe("formatCSV", () => {
  it("returns only the header row when all trend arrays are empty", () => {
    const csv = formatCSV(emptyTrends);
    expect(csv).toBe(CSV_HEADER);
  });

  it("includes the correct header columns", () => {
    const csv = formatCSV(sampleTrends);
    const [header] = csv.split("\n");
    expect(header).toBe(CSV_HEADER);
  });

  it("produces one data row per unique week_start", () => {
    const csv = formatCSV(sampleTrends);
    const lines = csv.split("\n");
    // header + 2 data rows
    expect(lines).toHaveLength(3);
  });

  it("sorts rows by week_start ascending", () => {
    const trends: ProjectTrends = {
      weeklyThroughput: [
        { weekStart: "2026-01-12", value: 5, sampleSize: 5 },
        { weekStart: "2026-01-05", value: 3, sampleSize: 3 },
      ],
      weeklyDeliveryThroughput: [],
      weeklyCycleTime: [],
      weeklyPipelineCycleTime: [],
      weeklySuccessRate: [],
    };
    const csv = formatCSV(trends);
    const [, firstRow, secondRow] = csv.split("\n");
    expect(firstRow).toMatch(/^2026-01-05,/);
    expect(secondRow).toMatch(/^2026-01-12,/);
  });

  it("converts cycle_time value from minutes to hours with 2 decimal places", () => {
    const csv = formatCSV(sampleTrends);
    const [, firstRow] = csv.split("\n");
    // 120 minutes / 60 = 2.00
    expect(firstRow).toContain(",2,");
    // cycle_time_hours field
    const fields = firstRow.split(",");
    expect(fields[6]).toBe("2");
  });

  it("rounds success_rate_pct to 1 decimal place", () => {
    const csv = formatCSV(sampleTrends);
    const [, firstRow] = csv.split("\n");
    const fields = firstRow.split(",");
    // 75.5 → "75.5"
    expect(fields[7]).toBe("75.5");
  });

  it("uses empty string for missing metric in a week", () => {
    // Only throughput for this week, no cycle time or success rate
    const trends: ProjectTrends = {
      weeklyThroughput: [{ weekStart: "2026-02-02", value: 4, sampleSize: 4 }],
      weeklyDeliveryThroughput: [],
      weeklyCycleTime: [],
      weeklyPipelineCycleTime: [],
      weeklySuccessRate: [],
    };
    const csv = formatCSV(trends);
    const [, dataRow] = csv.split("\n");
    expect(dataRow).toBe("2026-02-02,4,,,,,,");
  });

  it("merges data from all three series into a single row per week", () => {
    const csv = formatCSV(sampleTrends);
    const [, firstRow] = csv.split("\n");
    const fields = firstRow.split(",");
    expect(fields[0]).toBe("2026-01-05"); // week_start
    expect(fields[1]).toBe("3");          // throughput
    expect(fields[2]).toBe("4");          // unified_deliveries
    expect(fields[3]).toBe("3");          // task_deliveries
    expect(fields[4]).toBe("1");          // workspace_deliveries
    expect(fields[5]).toBe("2");          // merged_prs
    expect(fields[6]).toBe("2");          // cycle_time_hours (120/60=2)
    expect(fields[7]).toBe("75.5");       // success_rate_pct
  });

  it("handles weeks that only appear in cycle_time series", () => {
    const trends: ProjectTrends = {
      weeklyThroughput: [],
      weeklyDeliveryThroughput: [],
      weeklyCycleTime: [{ weekStart: "2026-03-01", value: 180, sampleSize: 2 }],
      weeklyPipelineCycleTime: [],
      weeklySuccessRate: [],
    };
    const csv = formatCSV(trends);
    const [, dataRow] = csv.split("\n");
    expect(dataRow).toBe("2026-03-01,,,,,,3,");
  });
});

// ─── formatJSONExport ─────────────────────────────────────────────────────────

describe("formatJSONExport", () => {
  it("contains a stats field matching the input", () => {
    const result = formatJSONExport(baseStats, emptyTrends);
    expect(result.stats).toEqual(baseStats);
  });

  it("contains a trends field matching the input", () => {
    const result = formatJSONExport(baseStats, sampleTrends);
    expect(result.trends).toEqual(sampleTrends);
  });

  it("contains an exported_at field that is a valid ISO string", () => {
    const before = new Date().toISOString();
    const result = formatJSONExport(baseStats, emptyTrends);
    const after = new Date().toISOString();

    expect(result.exported_at).toBeDefined();
    // Should parse as a valid date
    const parsed = new Date(result.exported_at);
    expect(parsed.toISOString()).toBe(result.exported_at);
    // Should be within the test execution window
    expect(result.exported_at >= before).toBe(true);
    expect(result.exported_at <= after).toBe(true);
  });

  it("has the expected shape with exactly three top-level keys", () => {
    const result = formatJSONExport(baseStats, emptyTrends);
    const keys = Object.keys(result).sort();
    expect(keys).toEqual(["exported_at", "stats", "trends"]);
  });

  it("does not mutate the input stats or trends objects", () => {
    const statsCopy = structuredClone(baseStats);
    const trendsCopy = structuredClone(emptyTrends);
    formatJSONExport(baseStats, emptyTrends);
    expect(baseStats).toEqual(statsCopy);
    expect(emptyTrends).toEqual(trendsCopy);
  });
});

// ─── Threshold logic ──────────────────────────────────────────────────────────

describe("shouldShowTrends", () => {
  it("returns false below the threshold", () => {
    expect(shouldShowTrends(MIN_TASKS_FOR_TRENDS - 1)).toBe(false);
  });

  it("returns true exactly at the threshold", () => {
    expect(shouldShowTrends(MIN_TASKS_FOR_TRENDS)).toBe(true);
  });

  it("returns true above the threshold", () => {
    expect(shouldShowTrends(MIN_TASKS_FOR_TRENDS + 5)).toBe(true);
  });

  it("returns false for 0 tasks", () => {
    expect(shouldShowTrends(0)).toBe(false);
  });
});

describe("shouldShowEme", () => {
  it("returns false when hasEme is false regardless of task count", () => {
    expect(shouldShowEme(100, false)).toBe(false);
  });

  it("returns false when task count is below threshold even with EME", () => {
    expect(shouldShowEme(MIN_TASKS_FOR_EME - 1, true)).toBe(false);
  });

  it("returns true exactly at the threshold with EME present", () => {
    expect(shouldShowEme(MIN_TASKS_FOR_EME, true)).toBe(true);
  });

  it("returns true above the threshold with EME present", () => {
    expect(shouldShowEme(MIN_TASKS_FOR_EME + 10, true)).toBe(true);
  });

  it("returns false for 0 tasks", () => {
    expect(shouldShowEme(0, true)).toBe(false);
  });
});

describe("constants", () => {
  it("MIN_TASKS_FOR_TRENDS is 10", () => {
    expect(MIN_TASKS_FOR_TRENDS).toBe(10);
  });

  it("MIN_TASKS_FOR_EME is 5", () => {
    expect(MIN_TASKS_FOR_EME).toBe(5);
  });
});
