/**
 * WaitingTaskDetail component tests
 *
 * Covers: container render, status banner, work summary card with submitted-time
 * formatting variants, completion-state branches, loading state, issue progress
 * conditional, and steps section conditional.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { WaitingTaskDetail } from "./WaitingTaskDetail";
import type { Task } from "@/types/task";

// ── Mocks ────────────────────────────────────────────────────────────────────

vi.mock("@/hooks/useTaskSteps", () => ({
  useTaskSteps: vi.fn(),
  useStepProgress: vi.fn(),
}));

vi.mock("@/api/review-issues", () => ({
  reviewIssuesApi: {
    getProgress: vi.fn(),
  },
}));

// Mock TaskMetricsCard so we don't depend on its internal hooks
vi.mock("./shared/TaskMetricsCard", () => ({
  TaskMetricsCard: ({ taskId }: { taskId: string }) => (
    <div data-testid="mock-task-metrics-card" data-task-id={taskId} />
  ),
}));

// Mock StepList so we don't drag in its dependencies
const mockStepList = vi.fn(({ taskId, editable }: { taskId: string; editable: boolean }) => (
  <div
    data-testid="mock-step-list"
    data-task-id={taskId}
    data-editable={String(editable)}
  />
));

vi.mock("../StepList", () => ({
  StepList: (props: { taskId: string; editable: boolean }) => mockStepList(props),
}));

// Mock IssueProgressBar to avoid pulling in IssueList
vi.mock("@/components/reviews/IssueList", () => ({
  IssueProgressBar: ({
    progress,
    showSeverityBreakdown,
  }: {
    progress: { total: number };
    showSeverityBreakdown?: boolean;
  }) => (
    <div
      data-testid="mock-issue-progress-bar"
      data-total={String(progress.total)}
      data-show-severity={String(showSeverityBreakdown ?? false)}
    />
  ),
}));

import { useTaskSteps, useStepProgress } from "@/hooks/useTaskSteps";
import { reviewIssuesApi } from "@/api/review-issues";

const mockUseTaskSteps = vi.mocked(useTaskSteps);
const mockUseStepProgress = vi.mocked(useStepProgress);
const mockGetProgress = vi.mocked(reviewIssuesApi.getProgress);

// ── Helpers ──────────────────────────────────────────────────────────────────

function createTestTask(overrides?: Partial<Task>): Task {
  return {
    id: "task-123",
    projectId: "project-456",
    category: "feature",
    title: "Test Task",
    description: "Test description",
    priority: 2,
    internalStatus: "pending_review",
    needsReviewPoint: false,
    createdAt: "2026-01-28T12:00:00+00:00",
    updatedAt: "2026-01-28T12:00:00+00:00",
    startedAt: "2026-01-28T12:00:00+00:00",
    completedAt: null,
    archivedAt: null,
    blockedReason: null,
    taskBranch: null,
    worktreePath: null,
    mergeCommitSha: null,
    metadata: null,
    ...overrides,
  } as Task;
}

function makeProgress(completed: number, total: number, skipped = 0) {
  return {
    taskId: "task-123",
    total,
    completed,
    inProgress: 0,
    pending: total - completed,
    skipped,
    failed: 0,
    currentStep: null,
    nextStep: null,
    percentComplete: total > 0 ? (completed / total) * 100 : 0,
  };
}

function makeIssueProgress(total: number) {
  return {
    taskId: "task-123",
    total,
    open: 0,
    inProgress: 0,
    addressed: 0,
    verified: total,
    wontfix: 0,
    percentResolved: total > 0 ? 100 : 0,
    bySeverity: {
      critical: { total: 0, open: 0, inProgress: 0, addressed: 0, verified: 0, wontfix: 0 },
      high: { total: 0, open: 0, inProgress: 0, addressed: 0, verified: 0, wontfix: 0 },
      medium: { total: 0, open: 0, inProgress: 0, addressed: 0, verified: 0, wontfix: 0 },
      low: { total: 0, open: 0, inProgress: 0, addressed: 0, verified: 0, wontfix: 0 },
    },
  };
}

function TestWrapper({ children }: { children: React.ReactNode }) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
}

function renderWithProviders(ui: React.ReactElement) {
  return render(ui, { wrapper: TestWrapper });
}

async function flush() {
  // Allow react-query promises to resolve
  await Promise.resolve();
  await Promise.resolve();
}

// ── Tests ────────────────────────────────────────────────────────────────────

describe("WaitingTaskDetail", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockUseTaskSteps.mockReturnValue({
      data: [],
      isLoading: false,
      isError: false,
    } as unknown as ReturnType<typeof useTaskSteps>);
    mockUseStepProgress.mockReturnValue({
      data: undefined,
      isLoading: false,
      isError: false,
    } as unknown as ReturnType<typeof useStepProgress>);
    mockGetProgress.mockResolvedValue(makeIssueProgress(0));
  });

  it("renders container with waiting-task-detail testid", () => {
    renderWithProviders(<WaitingTaskDetail task={createTestTask()} />);
    expect(screen.getByTestId("waiting-task-detail")).toBeInTheDocument();
  });

  it("renders status banner with awaiting AI review title and pending pill", () => {
    renderWithProviders(<WaitingTaskDetail task={createTestTask()} />);
    expect(screen.getByText("Awaiting AI Review")).toBeInTheDocument();
    expect(
      screen.getByText("Work complete — waiting for automated review")
    ).toBeInTheDocument();
    expect(screen.getByText("Pending")).toBeInTheDocument();
  });

  it("renders Work Summary section heading and wrapper", () => {
    renderWithProviders(<WaitingTaskDetail task={createTestTask()} />);
    expect(screen.getByTestId("work-completed-wrapper")).toBeInTheDocument();
    expect(screen.getByText("Work Summary")).toBeInTheDocument();
    expect(screen.getByText("Submitted")).toBeInTheDocument();
    expect(screen.getByText("Steps")).toBeInTheDocument();
  });

  it("renders metrics section with TaskMetricsCard receiving task id", () => {
    renderWithProviders(<WaitingTaskDetail task={createTestTask()} />);
    expect(screen.getByTestId("task-metrics-section")).toBeInTheDocument();
    expect(screen.getByText("Metrics")).toBeInTheDocument();
    expect(screen.getByTestId("mock-task-metrics-card")).toHaveAttribute(
      "data-task-id",
      "task-123"
    );
  });

  describe("WorkSummaryCard branches", () => {
    it("shows loading spinner placeholder when steps are loading", () => {
      mockUseTaskSteps.mockReturnValue({
        data: undefined,
        isLoading: true,
        isError: false,
      } as unknown as ReturnType<typeof useTaskSteps>);

      renderWithProviders(<WaitingTaskDetail task={createTestTask()} />);

      // Loading replaces the card body but not the wrapper
      expect(screen.getByTestId("work-completed-wrapper")).toBeInTheDocument();
      expect(screen.queryByText("Submitted")).not.toBeInTheDocument();
    });

    it("renders 'No steps defined' when total is 0", () => {
      mockUseStepProgress.mockReturnValue({
        data: makeProgress(0, 0),
        isLoading: false,
        isError: false,
      } as unknown as ReturnType<typeof useStepProgress>);

      renderWithProviders(<WaitingTaskDetail task={createTestTask()} />);
      expect(screen.getByText("No steps defined")).toBeInTheDocument();
    });

    it("renders partial progress 'X of Y completed' when some steps remain", () => {
      mockUseStepProgress.mockReturnValue({
        data: makeProgress(2, 5),
        isLoading: false,
        isError: false,
      } as unknown as ReturnType<typeof useStepProgress>);

      renderWithProviders(<WaitingTaskDetail task={createTestTask()} />);
      expect(screen.getByText("2 of 5 completed")).toBeInTheDocument();
    });

    it("renders 'All completed' when all steps are completed", () => {
      mockUseStepProgress.mockReturnValue({
        data: makeProgress(4, 4),
        isLoading: false,
        isError: false,
      } as unknown as ReturnType<typeof useStepProgress>);

      renderWithProviders(<WaitingTaskDetail task={createTestTask()} />);
      expect(screen.getByText("All completed")).toBeInTheDocument();
    });

    it("excludes skipped steps from completable totals", () => {
      // total=5, skipped=2 -> completable total=3, completed=3 -> all completed
      mockUseStepProgress.mockReturnValue({
        data: makeProgress(3, 5, 2),
        isLoading: false,
        isError: false,
      } as unknown as ReturnType<typeof useStepProgress>);

      renderWithProviders(<WaitingTaskDetail task={createTestTask()} />);
      expect(screen.getByText("All completed")).toBeInTheDocument();
    });
  });

  describe("submitted time formatting", () => {
    it("renders 'Just now' when updatedAt is undefined", () => {
      renderWithProviders(
        <WaitingTaskDetail task={createTestTask({ updatedAt: undefined as unknown as string })} />
      );
      expect(screen.getByText("Just now")).toBeInTheDocument();
    });

    it("renders 'Just now' when updatedAt is within the last minute", () => {
      const recent = new Date(Date.now() - 5_000).toISOString();
      renderWithProviders(
        <WaitingTaskDetail task={createTestTask({ updatedAt: recent })} />
      );
      expect(screen.getByText("Just now")).toBeInTheDocument();
    });

    it("renders minutes-ago when updatedAt is within the last hour", () => {
      const tenMinAgo = new Date(Date.now() - 10 * 60_000).toISOString();
      renderWithProviders(
        <WaitingTaskDetail task={createTestTask({ updatedAt: tenMinAgo })} />
      );
      expect(screen.getByText("10m ago")).toBeInTheDocument();
    });

    it("renders hours-ago when updatedAt is within the last day", () => {
      const threeHoursAgo = new Date(Date.now() - 3 * 60 * 60_000).toISOString();
      renderWithProviders(
        <WaitingTaskDetail task={createTestTask({ updatedAt: threeHoursAgo })} />
      );
      expect(screen.getByText("3h ago")).toBeInTheDocument();
    });

    it("renders days-ago when updatedAt is older than 24h", () => {
      const twoDaysAgo = new Date(Date.now() - 2 * 24 * 60 * 60_000).toISOString();
      renderWithProviders(
        <WaitingTaskDetail task={createTestTask({ updatedAt: twoDaysAgo })} />
      );
      expect(screen.getByText("2d ago")).toBeInTheDocument();
    });
  });

  describe("issue progress section", () => {
    it("does not render issue progress section when no issues exist", async () => {
      mockGetProgress.mockResolvedValue(makeIssueProgress(0));
      renderWithProviders(<WaitingTaskDetail task={createTestTask()} />);
      await flush();
      expect(
        screen.queryByTestId("issue-progress-section")
      ).not.toBeInTheDocument();
      expect(screen.queryByText("Issue Resolution")).not.toBeInTheDocument();
    });

    it("renders issue progress section when issues exist", async () => {
      mockGetProgress.mockResolvedValue(makeIssueProgress(4));
      renderWithProviders(<WaitingTaskDetail task={createTestTask()} />);

      // Wait for query to settle
      await new Promise((r) => setTimeout(r, 0));
      await new Promise((r) => setTimeout(r, 0));

      expect(await screen.findByTestId("issue-progress-section")).toBeInTheDocument();
      expect(screen.getByText("Issue Resolution")).toBeInTheDocument();
      const bar = screen.getByTestId("mock-issue-progress-bar");
      expect(bar).toHaveAttribute("data-total", "4");
      expect(bar).toHaveAttribute("data-show-severity", "true");
    });
  });

  describe("steps section", () => {
    it("renders waiting-steps-loading when steps are loading", () => {
      mockUseTaskSteps.mockReturnValue({
        data: undefined,
        isLoading: true,
        isError: false,
      } as unknown as ReturnType<typeof useTaskSteps>);

      renderWithProviders(<WaitingTaskDetail task={createTestTask()} />);
      expect(screen.getByTestId("waiting-steps-loading")).toBeInTheDocument();
      expect(
        screen.queryByTestId("waiting-steps-section")
      ).not.toBeInTheDocument();
    });

    it("does not render steps section when there are no steps", () => {
      mockUseTaskSteps.mockReturnValue({
        data: [],
        isLoading: false,
        isError: false,
      } as unknown as ReturnType<typeof useTaskSteps>);

      renderWithProviders(<WaitingTaskDetail task={createTestTask()} />);
      expect(
        screen.queryByTestId("waiting-steps-section")
      ).not.toBeInTheDocument();
      expect(screen.queryByTestId("waiting-steps-loading")).not.toBeInTheDocument();
    });

    it("renders steps section with non-editable StepList when task has steps", () => {
      mockUseTaskSteps.mockReturnValue({
        data: [
          {
            id: "step-1",
            taskId: "task-123",
            title: "Step 1",
            status: "completed",
            order: 0,
            createdAt: "2026-01-28T12:00:00+00:00",
            updatedAt: "2026-01-28T12:00:00+00:00",
          },
        ],
        isLoading: false,
        isError: false,
      } as unknown as ReturnType<typeof useTaskSteps>);

      renderWithProviders(<WaitingTaskDetail task={createTestTask()} />);
      const section = screen.getByTestId("waiting-steps-section");
      expect(section).toBeInTheDocument();
      // "Steps" label appears in both WorkSummaryCard and section heading;
      // assert via the section subtree only.
      expect(section.querySelector("h3")?.textContent).toBe("Steps");
      const stepList = screen.getByTestId("mock-step-list");
      expect(stepList).toHaveAttribute("data-task-id", "task-123");
      expect(stepList).toHaveAttribute("data-editable", "false");
    });
  });
});
