import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { Task } from "@/types/task";

import { EscalatedTaskDetail } from "./EscalatedTaskDetail";
import { MergeConflictTaskDetail } from "./MergeConflictTaskDetail";
import { MergeIncompleteTaskDetail } from "./MergeIncompleteTaskDetail";
import { RevisionTaskDetail } from "./RevisionTaskDetail";
import { WaitingTaskDetail } from "./WaitingTaskDetail";

const testState = vi.hoisted(() => ({
  steps: {
    data: [{ id: "step-1" }, { id: "step-2" }] as unknown[],
    isLoading: false,
  },
  progress: {
    data: {
      taskId: "task-1",
      total: 2,
      completed: 2,
      inProgress: 0,
      pending: 0,
      skipped: 0,
      failed: 0,
      currentStep: null,
      nextStep: null,
      percentComplete: 100,
    },
  },
  history: {
    data: [
      {
        id: "review-1",
        outcome: "changes_requested",
        notes: "Reviewer found a follow-up issue.",
        feedback: "Tighten the task detail parity coverage.",
        reviewer: "ai",
        created_at: "2026-07-07T12:00:00Z",
      },
    ] as unknown[],
    isLoading: false,
  },
  transitions: {
    data: [] as unknown[],
  },
  taskMetrics: {
    data: {
      stepCount: 2,
      completedStepCount: 2,
      reviewCount: 1,
      approvedReviewCount: 0,
      executionMinutes: 8,
      totalAgeHours: 1,
    },
    isLoading: false,
    isError: false,
  },
  conflictDetection: {
    conflicts: ["src/conflicted.ts"],
    isLoading: false,
    isEnabled: true,
  },
  planBranch: {
    data: null,
  },
  mergePipeline: {
    data: {
      needsAttention: [],
    },
  },
}));

vi.mock("../StepList", () => ({
  StepList: ({ taskId }: { taskId: string }) => (
    <div data-testid="mock-step-list">Steps for {taskId}</div>
  ),
}));

vi.mock("@/hooks/useTaskSteps", () => ({
  useTaskSteps: () => testState.steps,
  useStepProgress: () => testState.progress,
}));

vi.mock("@/hooks/useReviews", () => ({
  useTaskStateHistory: () => testState.history,
  reviewKeys: { all: ["reviews"] },
}));

vi.mock("@/hooks/useTaskStateTransitions", () => ({
  useTaskStateTransitions: () => testState.transitions,
}));

vi.mock("@/hooks/useTaskMetrics", () => ({
  useTaskMetrics: () => testState.taskMetrics,
}));

vi.mock("@/api/review-issues", () => ({
  reviewIssuesApi: {
    getProgress: vi.fn(() => Promise.resolve({ total: 0 })),
    getByTaskId: vi.fn(() => Promise.resolve([])),
  },
}));

vi.mock("@/components/reviews/IssueList", () => ({
  IssueProgressBar: () => <div data-testid="mock-issue-progress" />,
  IssueList: () => <div data-testid="mock-issue-list" />,
}));

vi.mock("@/components/reviews/ReviewFeedbackBody", () => ({
  ReviewFeedbackBody: ({ feedback }: { feedback: string | null }) => (
    <div data-testid="mock-review-feedback">{feedback}</div>
  ),
}));

vi.mock("@/hooks/useConflictDetection", () => ({
  useConflictDetection: () => testState.conflictDetection,
}));

vi.mock("@/hooks/useConflictDiff", () => ({
  useConflictDiff: () => ({ data: null, isLoading: false }),
}));

vi.mock("@/hooks/usePlanBranchForTask", () => ({
  usePlanBranchForTask: () => testState.planBranch,
}));

vi.mock("@/hooks/useMergePipeline", () => ({
  useMergePipeline: () => testState.mergePipeline,
}));

vi.mock("@/hooks/useConfirmation", () => ({
  useConfirmation: () => ({
    confirm: vi.fn(() => Promise.resolve(false)),
    confirmationDialogProps: { open: false },
    ConfirmationDialog: () => null,
  }),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(() => Promise.resolve({})),
}));

vi.mock("@/lib/tauri", () => ({
  api: {
    tasks: { move: vi.fn() },
    reviews: {
      approveTask: vi.fn(),
      requestTaskChanges: vi.fn(),
      reReviewTask: vi.fn(),
    },
  },
}));

vi.mock("@/lib/navigation", () => ({
  navigateToIdeationSession: vi.fn(),
}));

vi.mock("@/components/diff/ConflictDiffViewer", () => ({
  ConflictDiffViewer: () => <div data-testid="mock-conflict-diff" />,
}));

function renderWithClient(ui: ReactNode) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>,
  );
}

function task(overrides: Partial<Task> = {}): Task {
  return {
    id: "task-1",
    projectId: "project-1",
    title: "Non-screenshot task state",
    description: "Readable non-screenshot state description.",
    internalStatus: "pending_review",
    category: "feature",
    taskBranch: "task/non-screenshot-state",
    startedAt: "2026-07-07T11:00:00Z",
    updatedAt: "2026-07-07T12:00:00Z",
    completedAt: null,
    metadata: null,
    ...overrides,
  } as unknown as Task;
}

function expectOneColumnShell(testId: string, stageText: string) {
  const shell = screen.getByTestId(testId);
  expect(shell).not.toHaveClass("grid");
  expect(shell.className).not.toContain("xl:grid-cols");
  expect(screen.getByTestId("task-detail-summary")).toHaveTextContent(
    "Readable non-screenshot state description.",
  );
  expect(screen.getByTestId("task-detail-stage-body")).toHaveTextContent(stageText);
  expect(screen.queryByTestId("task-validation-section")).not.toBeInTheDocument();
}

describe("Agents non-screenshot task detail states", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("keeps pending review content in the shared one-column shell", () => {
    renderWithClient(
      <WaitingTaskDetail task={task({ internalStatus: "pending_review" })} />,
    );

    expectOneColumnShell("waiting-task-detail", "Awaiting AI Review");
    const stageBody = screen.getByTestId("task-detail-stage-body");
    expect(within(stageBody).getByTestId("work-completed-wrapper")).toBeInTheDocument();
    expect(within(stageBody).getByTestId("task-metrics-section")).toBeInTheDocument();
    expect(within(stageBody).getByTestId("waiting-steps-section")).toBeInTheDocument();
  });

  it("keeps revision feedback and steps in the shared one-column shell", () => {
    renderWithClient(
      <RevisionTaskDetail task={task({ internalStatus: "revision_needed" })} />,
    );

    expectOneColumnShell("revision-task-detail", "Revision Needed");
    const stageBody = screen.getByTestId("task-detail-stage-body");
    expect(within(stageBody).getByTestId("revision-feedback-section")).toBeInTheDocument();
    expect(within(stageBody).getByTestId("revision-steps-section")).toBeInTheDocument();
  });

  it("shows merge conflict details without mutation actions in historical mode", () => {
    renderWithClient(
      <MergeConflictTaskDetail
        task={task({
          internalStatus: "merge_conflict",
          metadata: JSON.stringify({ conflict_files: ["src/conflicted.ts"] }),
        })}
        isHistorical
      />,
    );

    expectOneColumnShell("merge-conflict-task-detail", "Merge Conflict");
    const stageBody = screen.getByTestId("task-detail-stage-body");
    expect(within(stageBody).getByTestId("conflict-files-section")).toHaveTextContent(
      "src/conflicted.ts",
    );
    expect(screen.queryByTestId("action-buttons")).not.toBeInTheDocument();
    expect(screen.queryByTestId("resolution-instructions-section")).not.toBeInTheDocument();
  });

  it("shows merge incomplete recovery context without mutation actions in historical mode", () => {
    renderWithClient(
      <MergeIncompleteTaskDetail
        task={task({
          internalStatus: "merge_incomplete",
          metadata: JSON.stringify({
            error: "Git merge failed after validation.",
            error_code: "merge_failed",
            source_branch: "task/non-screenshot-state",
            target_branch: "main",
            diagnostic_info: "fatal: merge failed",
            merge_recovery: {
              events: [
                {
                  kind: "attempt_failed",
                  source: "system",
                  at: "2026-07-07T12:15:00Z",
                  message: "Retry failed after merge validation.",
                  attempt: 1,
                },
              ],
            },
          }),
        })}
        isHistorical
      />,
    );

    expectOneColumnShell("merge-incomplete-task-detail", "Merge Incomplete");
    const stageBody = screen.getByTestId("task-detail-stage-body");
    expect(within(stageBody).getByTestId("recovery-attempts-section")).toHaveTextContent(
      "Attempt Failed",
    );
    expect(within(stageBody).getByTestId("error-context-section")).toHaveTextContent(
      "Git merge failed after validation.",
    );
    expect(screen.queryByTestId("action-buttons")).not.toBeInTheDocument();
    expect(screen.queryByTestId("recovery-steps-section")).not.toBeInTheDocument();
  });

  it("shows escalated review context without decision actions in historical mode", () => {
    renderWithClient(
      <EscalatedTaskDetail task={task({ internalStatus: "escalated" })} isHistorical />,
    );

    expectOneColumnShell("escalated-task-detail", "AI Escalated");
    const stageBody = screen.getByTestId("task-detail-stage-body");
    expect(within(stageBody).getByTestId("ai-escalation-reason-section")).toHaveTextContent(
      "Reviewer found a follow-up issue.",
    );
    expect(within(stageBody).getByTestId("previous-attempts-section")).toBeInTheDocument();
    expect(screen.queryByTestId("action-buttons")).not.toBeInTheDocument();
  });
});
