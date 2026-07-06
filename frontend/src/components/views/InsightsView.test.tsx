/**
 * InsightsView tests — orchestration of metrics panels, conditional rendering
 * (no project / loading / error / data), week-start toggle, and exports.
 * Heavy child panels are stubbed.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";

import { InsightsView } from "./InsightsView";
import {
  useInsightsChatUsageStats,
  useInsightsPrInsights,
  useInsightsStats,
  useInsightsTrends,
} from "@/hooks/useInsightsMetrics";
import { useProjectStore } from "@/stores/projectStore";
import type {
  ProjectPrInsights,
  ProjectStats,
  ProjectTrends,
  WeeklyDataPoint,
} from "@/types/project-stats";

// ---------------------------------------------------------------------------
// Hook & module mocks
// ---------------------------------------------------------------------------

vi.mock("@/hooks/useInsightsMetrics", () => ({
  useInsightsStats: vi.fn(),
  useInsightsChatUsageStats: vi.fn(),
  useInsightsPrInsights: vi.fn(),
  useInsightsTrends: vi.fn(),
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
  EffortEstimationPanel: (props: { projectId?: string; lowHours: number; readOnly?: boolean }) => (
    <div data-testid="eme-panel" data-project={props.projectId ?? "all"} data-readonly={String(props.readOnly)}>
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
vi.mock("./insights/DeliveryThroughputChart", () => ({
  DeliveryThroughputChart: ({ currentValue }: { currentValue?: string }) => (
    <div data-testid="delivery-throughput-chart">{currentValue}</div>
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
vi.mock("./insights/PrPerformanceInsightsCard", () => ({
  PrPerformanceInsightsCard: () => <div data-testid="pr-insights" />,
}));
vi.mock("./insights/AgentWorkspaceInsightsCard", () => ({
  AgentWorkspaceInsightsCard: () => <div data-testid="agent-workspaces-insights" />,
}));
vi.mock("@/components/tasks/detail-views/shared/DetailCard", () => ({
  DetailCard: ({ children }: { children: React.ReactNode }) => (
    <div data-testid="detail-card">{children}</div>
  ),
}));

const mockedStats = vi.mocked(useInsightsStats);
const mockedTrends = vi.mocked(useInsightsTrends);
const mockedUsage = vi.mocked(useInsightsChatUsageStats);
const mockedPrInsights = vi.mocked(useInsightsPrInsights);
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
      scope: "task_pipeline",
      scopeLabel: "Task pipeline",
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
    weeklyDeliveryThroughput: [
      {
        weekStart: "2026-01-01",
        unifiedDeliveries: 4,
        taskDeliveries: 3,
        workspaceDeliveries: 1,
        mergedPrs: 2,
        sampleSize: 6,
      },
      {
        weekStart: "2026-02-01",
        unifiedDeliveries: 6,
        taskDeliveries: 5,
        workspaceDeliveries: 1,
        mergedPrs: 4,
        sampleSize: 10,
      },
    ],
    weeklyCycleTime: makeWeekly([60, 70, 50]),
    weeklyPipelineCycleTime: makeWeekly([90, 100, 80]),
    weeklySuccessRate: makeWeekly([0.8, 0.85, 0.9]),
    ...overrides,
  };
}

function makePrInsights(overrides: Partial<ProjectPrInsights> = {}): ProjectPrInsights {
  return {
    summary: {
      totalPrs: 3,
      directWorkspacePrs: 2,
      taskPipelinePrs: 1,
      executionOwnedWorkspaceRefs: 0,
      mergedPrs: 2,
      openPrs: 1,
      draftPrs: 0,
      changesRequestedPrs: 0,
      closedPrs: 0,
      needsAgentPrs: 0,
      unpushedWorkspacePrs: 0,
      totalWorkspaces: 2,
      directWorkspaces: 2,
      directWorkspacesWithPrs: 2,
      directWorkspacePrConversionRate: 1,
      terminalMergeRate: 1,
      avgWorkspacePrCycleHours: 12,
      avgPlanPrWaitHours: 6,
      requestedChangesEvents: 0,
      autofixNeededEvents: 0,
      agentFixCompletedEvents: 0,
      supervisionEnabledWorkspaces: 1,
      autoMergeDesiredWorkspaces: 1,
      autoMergeActiveWorkspaces: 1,
    },
    origins: [],
    weeklyThroughput: [],
    workspaceDwellTimes: [],
    latestPrs: [],
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
  } as ReturnType<typeof useInsightsStats>);
  mockedTrends.mockReturnValue({
    data: trends,
    isLoading: false,
    error: null,
  } as ReturnType<typeof useInsightsTrends>);
  mockedUsage.mockReturnValue({
    data: undefined,
    isLoading: false,
    error: null,
  } as ReturnType<typeof useInsightsChatUsageStats>);
  mockedPrInsights.mockReturnValue({
    data: undefined,
    isLoading: false,
    error: null,
  } as ReturnType<typeof useInsightsPrInsights>);
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
  vi.useRealTimers();
  localStorage.clear();
});

describe("InsightsView — empty/loading/error states", () => {
  it("defaults to all projects when no active project is selected", () => {
    setProject(null);
    render(<InsightsView />);
    expect(screen.getByTestId("insights-view")).toBeInTheDocument();
    expect(screen.getByTestId("insights-project-filter")).toHaveTextContent("All projects");
    expect(mockedStats).toHaveBeenCalledWith(null, 0, expect.any(Number));
  });

  it("renders loading state when stats query is loading", () => {
    mockedStats.mockReturnValue({
      data: undefined,
      isLoading: true,
      error: null,
    } as ReturnType<typeof useInsightsStats>);
    mockedTrends.mockReturnValue({
      data: undefined,
      isLoading: false,
      error: null,
    } as ReturnType<typeof useInsightsTrends>);
    mockedUsage.mockReturnValue({
      data: undefined,
      isLoading: false,
      error: null,
    } as ReturnType<typeof useInsightsChatUsageStats>);
    mockedPrInsights.mockReturnValue({
      data: undefined,
      isLoading: false,
      error: null,
    } as ReturnType<typeof useInsightsPrInsights>);
    render(<InsightsView />);
    expect(screen.getByText(/loading insights/i)).toBeInTheDocument();
  });

  it("renders loading state when trends query is loading", () => {
    mockedStats.mockReturnValue({
      data: undefined,
      isLoading: false,
      error: null,
    } as ReturnType<typeof useInsightsStats>);
    mockedTrends.mockReturnValue({
      data: undefined,
      isLoading: true,
      error: null,
    } as ReturnType<typeof useInsightsTrends>);
    mockedUsage.mockReturnValue({
      data: undefined,
      isLoading: false,
      error: null,
    } as ReturnType<typeof useInsightsChatUsageStats>);
    mockedPrInsights.mockReturnValue({
      data: undefined,
      isLoading: false,
      error: null,
    } as ReturnType<typeof useInsightsPrInsights>);
    render(<InsightsView />);
    expect(screen.getByText(/loading insights/i)).toBeInTheDocument();
  });

  it("renders error state when stats query has error", () => {
    mockedStats.mockReturnValue({
      data: undefined,
      isLoading: false,
      error: new Error("boom"),
    } as ReturnType<typeof useInsightsStats>);
    mockedTrends.mockReturnValue({
      data: undefined,
      isLoading: false,
      error: null,
    } as ReturnType<typeof useInsightsTrends>);
    mockedUsage.mockReturnValue({
      data: undefined,
      isLoading: false,
      error: null,
    } as ReturnType<typeof useInsightsChatUsageStats>);
    render(<InsightsView />);
    expect(screen.getByText(/failed to load insights/i)).toBeInTheDocument();
  });

  it("returns null when both queries resolve without data", () => {
    mockedStats.mockReturnValue({
      data: undefined,
      isLoading: false,
      error: null,
    } as ReturnType<typeof useInsightsStats>);
    mockedTrends.mockReturnValue({
      data: undefined,
      isLoading: false,
      error: null,
    } as ReturnType<typeof useInsightsTrends>);
    mockedUsage.mockReturnValue({
      data: undefined,
      isLoading: false,
      error: null,
    } as ReturnType<typeof useInsightsChatUsageStats>);
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
    expect(screen.getByText(/all-project engineering performance/i)).toBeInTheDocument();
    // Stat cards
    expect(screen.getAllByTestId("stat-card").length).toBeGreaterThanOrEqual(4);
    // EME panel renders (showEme=true: taskCount>=5 + eme not null) — appears in
    // both the inline (medium) and right-column (large) slots.
    expect(screen.getAllByTestId("eme-panel").length).toBeGreaterThanOrEqual(1);
    // Trend charts (>=10 tasks unlocks trends)
    expect(screen.getByTestId("delivery-throughput-chart")).toBeInTheDocument();
    expect(screen.getAllByTestId("trend-chart").length).toBeGreaterThanOrEqual(2);
    // Breakdowns
    expect(screen.getByTestId("cycle-breakdown")).toBeInTheDocument();
    expect(screen.getByTestId("column-dwell")).toBeInTheDocument();
    // Copy markdown button
    expect(screen.getByTestId("copy-md")).toBeInTheDocument();
  });

  it("filters Insights locally without changing the active project store", () => {
    setProject("proj-1");
    mockSuccess();
    render(<InsightsView />);

    const filter = screen.getByTestId("insights-project-filter");
    expect(filter).toHaveTextContent("All projects");

    fireEvent.click(filter);
    fireEvent.change(screen.getByTestId("insights-project-filter-search"), {
      target: { value: "P" },
    });
    fireEvent.click(screen.getByTestId("insights-project-option-proj-1"));

    expect(filter).toHaveTextContent("P");
    expect(mockedStats).toHaveBeenLastCalledWith("proj-1", 0, expect.any(Number));
    expect(screen.getByText(/P engineering performance/i)).toBeInTheDocument();
  });

  it("renders trends-locked message when taskCount < 10", () => {
    mockSuccess(
      makeStats({ taskCount: 4, eme: null }),
      makeTrends({ weeklyDeliveryThroughput: [] }),
    );
    render(<InsightsView />);
    expect(screen.getByText(/trend charts unlock after 10/i)).toBeInTheDocument();
    expect(screen.getByText(/4 of 10 task-pipeline completions available/)).toBeInTheDocument();
    // No trend charts
    expect(screen.queryAllByTestId("trend-chart").length).toBe(0);
  });

  it("labels the current delivery bucket as this week using local calendar dates", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-05-22T12:00:00Z"));
    mockSuccess(
      makeStats({ tasksCompletedThisWeek: 0 }),
      makeTrends({
        weeklyDeliveryThroughput: [
          {
            weekStart: "2026-05-17",
            unifiedDeliveries: 0,
            taskDeliveries: 0,
            workspaceDeliveries: 0,
            mergedPrs: 5,
            sampleSize: 5,
          },
        ],
      }),
    );

    render(<InsightsView />);

    const cards = screen.getAllByTestId("stat-card");
    const deliveriesCard = cards.find(
      (el) => el.getAttribute("data-label") === "Deliveries This Week",
    );
    expect(deliveriesCard?.textContent).toContain("0 tasks / 0 workspaces / 5 merged PRs");
    expect(screen.getByTestId("delivery-throughput-chart")).toHaveTextContent("0 this week");
  });

  it("falls back to the latest active delivery week when this week is empty", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-05-22T12:00:00Z"));
    mockSuccess(
      makeStats({ tasksCompletedThisWeek: 0 }),
      makeTrends({
        weeklyDeliveryThroughput: [
          {
            weekStart: "2026-05-10",
            unifiedDeliveries: 5,
            taskDeliveries: 0,
            workspaceDeliveries: 5,
            mergedPrs: 5,
            sampleSize: 10,
          },
          {
            weekStart: "2026-05-17",
            unifiedDeliveries: 0,
            taskDeliveries: 0,
            workspaceDeliveries: 0,
            mergedPrs: 0,
            sampleSize: 0,
          },
        ],
      }),
    );

    render(<InsightsView />);

    const cards = screen.getAllByTestId("stat-card");
    const deliveriesCard = cards.find(
      (el) => el.getAttribute("data-label") === "Deliveries Latest Active Week",
    );
    expect(deliveriesCard?.textContent).toContain("5");
    expect(deliveriesCard?.textContent).toContain(
      "week of May 10 · 0 tasks / 5 workspaces / 5 merged PRs",
    );
    expect(screen.getByTestId("delivery-throughput-chart")).toHaveTextContent(
      "5 week of May 10",
    );
  });

  it("renders EME-locked message when fewer than 5 tasks completed", () => {
    mockSuccess(makeStats({ taskCount: 3, eme: null }));
    render(<InsightsView />);
    // Locked card renders in both inline (medium) and sticky (large) slots.
    expect(
      screen.getAllByText(/effort estimation unlocks after 5/i).length,
    ).toBeGreaterThanOrEqual(1);
    expect(
      screen.getAllByText(/3 of 5 task-pipeline completions available/).length,
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
    } as ReturnType<typeof useInsightsChatUsageStats>);
    render(<InsightsView />);
    expect(screen.getByTestId("usage-insights")).toBeInTheDocument();
  });

  it("renders PrPerformanceInsightsCard when PR insight data is present", () => {
    mockSuccess();
    mockedPrInsights.mockReturnValue({
      data: makePrInsights(),
      isLoading: false,
      error: null,
    } as ReturnType<typeof useInsightsPrInsights>);
    render(<InsightsView />);
    expect(screen.getByTestId("agent-workspaces-insights")).toBeInTheDocument();
    expect(screen.getByTestId("pr-insights")).toBeInTheDocument();
  });

  it("renders workspace PR insights and delivery throughput with zero task-pipeline rows", () => {
    const basePrInsights = makePrInsights();

    mockSuccess(
      makeStats({ taskCount: 0, eme: null }),
      makeTrends({
        weeklyThroughput: [],
        weeklyDeliveryThroughput: [
          {
            weekStart: "2026-05-17",
            unifiedDeliveries: 2,
            taskDeliveries: 0,
            workspaceDeliveries: 2,
            mergedPrs: 1,
            sampleSize: 3,
          },
        ],
        weeklyCycleTime: [],
        weeklyPipelineCycleTime: [],
        weeklySuccessRate: [],
      }),
    );
    mockedPrInsights.mockReturnValue({
      data: makePrInsights({
        summary: {
          ...basePrInsights.summary,
          totalPrs: 2,
          directWorkspacePrs: 2,
          taskPipelinePrs: 0,
        },
      }),
      isLoading: false,
      error: null,
    } as ReturnType<typeof useInsightsPrInsights>);

    render(<InsightsView />);

    expect(screen.getByTestId("agent-workspaces-insights")).toBeInTheDocument();
    expect(screen.getByTestId("pr-insights")).toBeInTheDocument();
    expect(screen.getByTestId("delivery-throughput-chart")).toBeInTheDocument();
    expect(screen.queryByText(/trend charts unlock after 10/i)).not.toBeInTheDocument();
  });

  it("renders Avg Pipeline Time as em dash when avgPipelineMinutes is null", () => {
    mockSuccess(makeStats({ avgPipelineMinutes: null }));
    render(<InsightsView />);
    const cards = screen.getAllByTestId("stat-card");
    const pipelineCard = cards.find(
      (el) => el.getAttribute("data-label") === "Task Pipeline Time",
    );
    expect(pipelineCard?.textContent).toContain("—");
  });

  it("falls back to stats.tasksCompletedThisWeek when weeklyThroughput empty", () => {
    mockSuccess(
      makeStats({ tasksCompletedThisWeek: 9, taskCount: 4, eme: null }),
      makeTrends({
        weeklyThroughput: [],
        weeklyDeliveryThroughput: [],
        weeklyCycleTime: [],
        weeklySuccessRate: [],
      }),
    );
    render(<InsightsView />);
    const cards = screen.getAllByTestId("stat-card");
    const tasksCard = cards.find((el) =>
      (el.getAttribute("data-label") ?? "").startsWith("Deliveries"),
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
