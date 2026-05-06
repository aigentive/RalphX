/**
 * InsightsView tests — orchestration of metrics panels, conditional rendering
 * (no project / loading / error / data), week-start toggle, and exports.
 * Heavy child panels are stubbed.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";

import { InsightsView } from "./InsightsView";
import { useProjectStats } from "@/hooks/useProjectStats";
import { useProjectChatUsageStats } from "@/hooks/useProjectChatUsageStats";
import { useProjectTrends } from "@/hooks/useProjectTrends";
import { useProjectStore } from "@/stores/projectStore";
import type {
  ProjectStats,
  ProjectTrends,
  WeeklyDataPoint,
} from "@/types/project-stats";

// ---------------------------------------------------------------------------
// Hook & module mocks
// ---------------------------------------------------------------------------

vi.mock("@/hooks/useProjectStats", () => ({
  useProjectStats: vi.fn(),
}));
vi.mock("@/hooks/useProjectChatUsageStats", () => ({
  useProjectChatUsageStats: vi.fn(),
}));
vi.mock("@/hooks/useProjectTrends", () => ({
  useProjectTrends: vi.fn(),
}));
vi.mock("@/stores/projectStore", async () => {
  const actual = await vi.importActual<typeof import("@/stores/projectStore")>(
    "@/stores/projectStore",
  );
  return {
    ...actual,
    useProjectStore: vi.fn(),
  };
});

// Stub heavy child components — keep them lightweight identifiable shells.
vi.mock("./insights/EffortEstimationPanel", () => ({
  EffortEstimationPanel: (props: { projectId: string; lowHours: number }) => (
    <div data-testid="eme-panel" data-project={props.projectId}>
      EME {props.lowHours}
    </div>
  ),
}));
vi.mock("./insights/StatCard", () => ({
  StatCard: ({ label, value, sub }: { label: string; value: string; sub?: string }) => (
    <div data-testid="stat-card" data-label={label}>
      <span>{label}</span>
      <span>{value}</span>
      {sub ? <span>{sub}</span> : null}
    </div>
  ),
}));
vi.mock("./insights/TrendChart", () => ({
  TrendChart: ({ title }: { title: string }) => (
    <div data-testid="trend-chart" data-title={title}>
      {title}
    </div>
  ),
}));
vi.mock("./insights/MetricsDetails", () => ({
  CycleTimeBreakdown: () => <div data-testid="cycle-breakdown" />,
  ColumnDwellTimeBreakdown: () => <div data-testid="column-dwell" />,
  CopyMarkdownButton: () => <button data-testid="copy-md">Copy</button>,
}));
vi.mock("./insights/UsageInsightsCard", () => ({
  UsageInsightsCard: () => <div data-testid="usage-insights" />,
}));
vi.mock("@/components/tasks/detail-views/shared/DetailCard", () => ({
  DetailCard: ({ children }: { children: React.ReactNode }) => (
    <div data-testid="detail-card">{children}</div>
  ),
}));

const mockedStats = vi.mocked(useProjectStats);
const mockedTrends = vi.mocked(useProjectTrends);
const mockedUsage = vi.mocked(useProjectChatUsageStats);
const mockedStore = vi.mocked(useProjectStore);

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeStats(overrides: Partial<ProjectStats> = {}): ProjectStats {
  return {
    taskCount: 20,
    tasksCompletedToday: 2,
    tasksCompletedThisWeek: 5,
    tasksCompletedThisMonth: 18,
    agentSuccessRate: 0.85,
    agentSuccessCount: 17,
    agentTotalCount: 20,
    reviewPassRate: 0.9,
    reviewPassCount: 9,
    reviewTotalCount: 10,
    cycleTimeBreakdown: [],
    columnDwellTimes: [],
    avgPipelineMinutes: 120,
    eme: {
      lowHours: 100,
      highHours: 200,
      taskCount: 20,
      earliestTaskDate: "2026-01-01",
      latestTaskDate: "2026-04-01",
    },
    ...overrides,
  };
}

function makeWeekly(values: number[]): WeeklyDataPoint[] {
  return values.map((value, idx) => ({
    weekStart: `2026-0${idx + 1}-01`,
    value,
    sampleSize: 5,
  }));
}

function makeTrends(overrides: Partial<ProjectTrends> = {}): ProjectTrends {
  return {
    weeklyThroughput: makeWeekly([3, 5, 7]),
    weeklyCycleTime: makeWeekly([60, 70, 50]),
    weeklyPipelineCycleTime: makeWeekly([90, 100, 80]),
    weeklySuccessRate: makeWeekly([0.8, 0.85, 0.9]),
    ...overrides,
  };
}

function setProject(id: string | null) {
  // useProjectStore is called as a selector → invoke selector with stub state.
  mockedStore.mockImplementation((selector: unknown) => {
    const state = {
      activeProjectId: id,
      projects: id ? { [id]: { id, name: "P", path: "/p" } } : {},
    };
    return typeof selector === "function"
      ? (selector as (s: unknown) => unknown)(state)
      : state;
  });
}

function mockSuccess(
  stats: ProjectStats = makeStats(),
  trends: ProjectTrends = makeTrends(),
) {
  mockedStats.mockReturnValue({
    data: stats,
    isLoading: false,
    error: null,
  } as ReturnType<typeof useProjectStats>);
  mockedTrends.mockReturnValue({
    data: trends,
    isLoading: false,
    error: null,
  } as ReturnType<typeof useProjectTrends>);
  mockedUsage.mockReturnValue({
    data: undefined,
    isLoading: false,
    error: null,
  } as ReturnType<typeof useProjectChatUsageStats>);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  setProject("proj-1");
  mockSuccess();
});

afterEach(() => {
  localStorage.clear();
});

describe("InsightsView — empty/loading/error states", () => {
  it("renders empty state when no active project", () => {
    setProject(null);
    render(<InsightsView />);
    expect(screen.getByText(/select a project/i)).toBeInTheDocument();
  });

  it("renders loading state when stats query is loading", () => {
    mockedStats.mockReturnValue({
      data: undefined,
      isLoading: true,
      error: null,
    } as ReturnType<typeof useProjectStats>);
    mockedTrends.mockReturnValue({
      data: undefined,
      isLoading: false,
      error: null,
    } as ReturnType<typeof useProjectTrends>);
    mockedUsage.mockReturnValue({
      data: undefined,
      isLoading: false,
      error: null,
    } as ReturnType<typeof useProjectChatUsageStats>);
    render(<InsightsView />);
    expect(screen.getByText(/loading insights/i)).toBeInTheDocument();
  });

  it("renders loading state when trends query is loading", () => {
    mockedStats.mockReturnValue({
      data: undefined,
      isLoading: false,
      error: null,
    } as ReturnType<typeof useProjectStats>);
    mockedTrends.mockReturnValue({
      data: undefined,
      isLoading: true,
      error: null,
    } as ReturnType<typeof useProjectTrends>);
    mockedUsage.mockReturnValue({
      data: undefined,
      isLoading: false,
      error: null,
    } as ReturnType<typeof useProjectChatUsageStats>);
    render(<InsightsView />);
    expect(screen.getByText(/loading insights/i)).toBeInTheDocument();
  });

  it("renders error state when stats query has error", () => {
    mockedStats.mockReturnValue({
      data: undefined,
      isLoading: false,
      error: new Error("boom"),
    } as ReturnType<typeof useProjectStats>);
    mockedTrends.mockReturnValue({
      data: undefined,
      isLoading: false,
      error: null,
    } as ReturnType<typeof useProjectTrends>);
    mockedUsage.mockReturnValue({
      data: undefined,
      isLoading: false,
      error: null,
    } as ReturnType<typeof useProjectChatUsageStats>);
    render(<InsightsView />);
    expect(screen.getByText(/failed to load insights/i)).toBeInTheDocument();
  });

  it("returns null when both queries resolve without data", () => {
    mockedStats.mockReturnValue({
      data: undefined,
      isLoading: false,
      error: null,
    } as ReturnType<typeof useProjectStats>);
    mockedTrends.mockReturnValue({
      data: undefined,
      isLoading: false,
      error: null,
    } as ReturnType<typeof useProjectTrends>);
    mockedUsage.mockReturnValue({
      data: undefined,
      isLoading: false,
      error: null,
    } as ReturnType<typeof useProjectChatUsageStats>);
    const { container } = render(<InsightsView />);
    expect(container.querySelector('[data-testid="insights-view"]')).toBeNull();
  });
});

describe("InsightsView — full render with stats + trends", () => {
  it("renders header, stat cards, EME panel, trend charts, and breakdowns", () => {
    mockSuccess();
    render(<InsightsView />);
    expect(screen.getByTestId("insights-view")).toBeInTheDocument();
    expect(screen.getByText("Insights")).toBeInTheDocument();
    expect(screen.getByText(/project analytics/i)).toBeInTheDocument();
    // Stat cards
    expect(screen.getAllByTestId("stat-card").length).toBeGreaterThanOrEqual(4);
    // EME panel renders (showEme=true: taskCount>=5 + eme not null) — appears in
    // both the inline (medium) and right-column (large) slots.
    expect(screen.getAllByTestId("eme-panel").length).toBeGreaterThanOrEqual(1);
    // Trend charts (>=10 tasks unlocks trends)
    expect(screen.getAllByTestId("trend-chart").length).toBeGreaterThanOrEqual(2);
    // Breakdowns
    expect(screen.getByTestId("cycle-breakdown")).toBeInTheDocument();
    expect(screen.getByTestId("column-dwell")).toBeInTheDocument();
    // Copy markdown button
    expect(screen.getByTestId("copy-md")).toBeInTheDocument();
  });

  it("renders trends-locked message when taskCount < 10", () => {
    mockSuccess(makeStats({ taskCount: 4, eme: null }));
    render(<InsightsView />);
    expect(screen.getByText(/trend charts unlock after 10/i)).toBeInTheDocument();
    expect(screen.getByText(/4 of 10 tasks completed/)).toBeInTheDocument();
    // No trend charts
    expect(screen.queryAllByTestId("trend-chart").length).toBe(0);
  });

  it("renders EME-locked message when fewer than 5 tasks completed", () => {
    mockSuccess(makeStats({ taskCount: 3, eme: null }));
    render(<InsightsView />);
    // Locked card renders in both inline (medium) and sticky (large) slots.
    expect(
      screen.getAllByText(/effort estimation unlocks after 5/i).length,
    ).toBeGreaterThanOrEqual(1);
    expect(
      screen.getAllByText(/3 of 5 tasks completed/).length,
    ).toBeGreaterThanOrEqual(1);
  });

  it("renders UsageInsightsCard when usage data is present", () => {
    mockSuccess();
    mockedUsage.mockReturnValue({
      data: {
        scopeType: "project",
        scopeId: "proj-1",
        conversationCount: 1,
      },
      isLoading: false,
      error: null,
    } as ReturnType<typeof useProjectChatUsageStats>);
    render(<InsightsView />);
    expect(screen.getByTestId("usage-insights")).toBeInTheDocument();
  });

  it("renders Avg Pipeline Time as em dash when avgPipelineMinutes is null", () => {
    mockSuccess(makeStats({ avgPipelineMinutes: null }));
    render(<InsightsView />);
    const cards = screen.getAllByTestId("stat-card");
    const pipelineCard = cards.find(
      (el) => el.getAttribute("data-label") === "Avg Pipeline Time",
    );
    expect(pipelineCard?.textContent).toContain("—");
  });

  it("falls back to stats.tasksCompletedThisWeek when weeklyThroughput empty", () => {
    mockSuccess(
      makeStats({ tasksCompletedThisWeek: 9, taskCount: 4, eme: null }),
      makeTrends({
        weeklyThroughput: [],
        weeklyCycleTime: [],
        weeklySuccessRate: [],
      }),
    );
    render(<InsightsView />);
    const cards = screen.getAllByTestId("stat-card");
    const tasksCard = cards.find((el) =>
      (el.getAttribute("data-label") ?? "").startsWith("Tasks"),
    );
    expect(tasksCard?.textContent).toContain("9");
  });
});

describe("InsightsView — week start toggle", () => {
  it("defaults to Sunday and switches to Monday on click", () => {
    mockSuccess();
    const { container } = render(<InsightsView />);
    const monBtn = screen.getByTitle(/week starts on monday/i);
    expect(monBtn).toBeInTheDocument();
    fireEvent.click(monBtn);
    expect(localStorage.getItem("ralphx:insights:weekStartDay")).toBe("1");
    // Component still renders post-toggle.
    expect(container.querySelector('[data-testid="insights-view"]')).toBeInTheDocument();
  });

  it("reads persisted Monday from localStorage on mount", () => {
    localStorage.setItem("ralphx:insights:weekStartDay", "1");
    mockSuccess();
    render(<InsightsView />);
    // Mon button should be active — its style includes the accent color.
    const monBtn = screen.getByTitle(/week starts on monday/i);
    expect(monBtn.getAttribute("style") ?? "").toContain("accent-primary");
  });
});

describe("InsightsView — export buttons", () => {
  it("triggers JSON download when JSON button is clicked", () => {
    mockSuccess();
    const createObjectURL = vi.fn(() => "blob:json");
    const revokeObjectURL = vi.fn();
    Object.defineProperty(URL, "createObjectURL", {
      configurable: true,
      value: createObjectURL,
    });
    Object.defineProperty(URL, "revokeObjectURL", {
      configurable: true,
      value: revokeObjectURL,
    });
    const clickSpy = vi
      .spyOn(HTMLAnchorElement.prototype, "click")
      .mockImplementation(() => {});

    render(<InsightsView />);
    const jsonBtn = screen.getByTitle(/download json/i);
    fireEvent.click(jsonBtn);
    expect(createObjectURL).toHaveBeenCalled();
    expect(clickSpy).toHaveBeenCalled();
    expect(revokeObjectURL).toHaveBeenCalled();

    clickSpy.mockRestore();
  });

  it("triggers CSV download when CSV button is clicked", () => {
    mockSuccess();
    const createObjectURL = vi.fn(() => "blob:csv");
    const revokeObjectURL = vi.fn();
    Object.defineProperty(URL, "createObjectURL", {
      configurable: true,
      value: createObjectURL,
    });
    Object.defineProperty(URL, "revokeObjectURL", {
      configurable: true,
      value: revokeObjectURL,
    });
    const clickSpy = vi
      .spyOn(HTMLAnchorElement.prototype, "click")
      .mockImplementation(() => {});

    render(<InsightsView />);
    const csvBtn = screen.getByTitle(/download csv/i);
    fireEvent.click(csvBtn);
    expect(createObjectURL).toHaveBeenCalled();
    expect(clickSpy).toHaveBeenCalled();
    expect(revokeObjectURL).toHaveBeenCalled();

    clickSpy.mockRestore();
  });
});
