import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { TaskValidationSummary } from "@/hooks/useTaskValidationSummary";
import type { Task } from "@/types/task";
import { AGENT_CONTROL_DISABLED_HINT } from "@/lib/remote/agent-gate";
import { TooltipProvider } from "@/components/ui/tooltip";
import { CompletedTaskDetail } from "./CompletedTaskDetail";
import { setDetailViewEnvironment } from "./agent-gate.test-utils";

const {
  historyState,
  transitionsState,
  gitDiffState,
  validationState,
  moveTask,
  resumeExecution,
} = vi.hoisted(() => ({
  historyState: {
    data: [
      {
        id: "review-1",
        outcome: "approved",
        notes: "Reviewer signed off",
        feedback: "All checks pass",
        reviewer: "ai_reviewer",
        created_at: "2026-07-07T12:00:00Z",
      },
    ] as unknown[],
    isLoading: false,
  },
  transitionsState: {
    data: [] as unknown[],
  },
  gitDiffState: {
    commits: [
      {
        shortSha: "abc1234",
        message: "feat: complete task",
        sha: "abc1234",
      },
    ],
  },
  validationState: {
    display: undefined as TaskValidationSummary | undefined,
  },
  moveTask: vi.fn(),
  resumeExecution: vi.fn(),
}));

vi.mock("@/hooks/useReviews", () => ({
  useTaskStateHistory: () => historyState,
  reviewKeys: { all: ["review"] },
}));

vi.mock("@/hooks/useTaskStateTransitions", () => ({
  useTaskStateTransitions: () => transitionsState,
}));

vi.mock("@/hooks/useGitDiff", () => ({
  useGitDiff: () => gitDiffState,
}));

vi.mock("@/components/reviews/ReviewDetailModal", () => ({
  ReviewDetailModal: ({ onClose }: { onClose: () => void }) => (
    <div data-testid="review-detail-modal">
      <button type="button" onClick={onClose}>
        Close review modal
      </button>
    </div>
  ),
}));

vi.mock("../TaskRerunDialog", () => ({
  TaskRerunDialog: ({
    isOpen,
    onConfirm,
    error,
  }: {
    isOpen: boolean;
    onConfirm: (result: { option: "keep_changes"; note: string }) => void;
    error?: string | null;
  }) =>
    isOpen ? (
      <div>
        <button
          type="button"
          data-testid="confirm-rerun"
          onClick={() => onConfirm({ option: "keep_changes", note: "Review new changes" })}
        >
          Confirm rerun
        </button>
        {error && <p>{error}</p>}
      </div>
    ) : null,
}));

vi.mock("@/lib/tauri", () => ({
  api: {
    tasks: {
      move: moveTask,
    },
  },
}));

vi.mock("@/lib/task-actions/resume-execution-if-stopped", () => ({
  resumeExecutionIfStopped: resumeExecution,
}));

vi.mock("@/hooks/useTaskValidationSummary", () => ({
  useTaskValidationSummary: () => ({
    data: validationState.display,
    isLoading: false,
    isError: false,
  }),
}));

vi.mock("@/hooks/useTaskValidationEvents", () => ({
  useTaskValidationLiveState: () => null,
  useDisplayTaskValidationSummary: () => validationState.display,
}));

function validationSummary(): TaskValidationSummary {
  return {
    task_id: "task-1",
    project_id: "project-1",
    policy_enabled: true,
    latest_run: {
      id: "run-1",
      purpose: "final",
      context_type: "execution",
      requested_by_agent: "ralphx-execution-worker",
      status: "passed",
      mode: "force",
      policy_enabled: true,
      head_sha: "abcdef1234567890",
      head_short_sha: "abcdef12",
      base_ref: "main",
      started_at: "2026-07-07T11:55:00Z",
      completed_at: "2026-07-07T11:56:00Z",
    },
    commands: [],
    legacy_validation_cache: null,
    disabled_reason: null,
  };
}

function task(): Task {
  return {
    id: "task-1",
    projectId: "project-1",
    title: "Completed task",
    description: "Readable completed task summary",
    internalStatus: "approved",
    category: "feature",
    startedAt: "2026-07-07T11:00:00Z",
    completedAt: "2026-07-07T12:00:00Z",
  } as unknown as Task;
}

function TestWrapper({ children }: { children: React.ReactNode }) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return (
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>{children}</TooltipProvider>
    </QueryClientProvider>
  );
}

describe("Agents CompletedTaskDetail", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    historyState.isLoading = false;
    validationState.display = validationSummary();
    moveTask.mockResolvedValue(undefined);
    resumeExecution.mockResolvedValue(undefined);
    setDetailViewEnvironment("local");
  });

  it("renders completed stage content, review evidence, and current actions in the one-column shell", () => {
    render(<CompletedTaskDetail task={task()} />, { wrapper: TestWrapper });

    expect(screen.getByTestId("completed-task-detail")).toBeInTheDocument();
    expect(screen.getByTestId("task-detail-stage-body")).toHaveTextContent("Task Completed");
    const evidence = screen.getByTestId("task-detail-evidence");
    expect(evidence).toHaveTextContent("Task Validation");
    expect(evidence).toHaveTextContent("Review History");
    expect(
      within(evidence).getByTestId("task-validation-section"),
    ).toBeInTheDocument();
    expect(
      within(evidence).getByTestId("task-validation-section").compareDocumentPosition(
        within(evidence).getByText("Review History"),
      ) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(screen.getByTestId("task-detail-actions")).toBeInTheDocument();
    expect(screen.getByTestId("review-code-button")).toBeInTheDocument();

    fireEvent.click(screen.getByTestId("review-code-button"));

    expect(screen.getByTestId("review-detail-modal")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Close review modal" }));

    expect(screen.queryByTestId("review-detail-modal")).not.toBeInTheDocument();
  });

  it("renders a loading state while review history is loading", () => {
    historyState.isLoading = true;

    const { container } = render(<CompletedTaskDetail task={task()} />, {
      wrapper: TestWrapper,
    });

    expect(container.querySelector(".animate-spin")).toBeInTheDocument();
    expect(screen.queryByTestId("completed-task-detail")).not.toBeInTheDocument();
  });

  it("keeps completed stage evidence but hides mutation actions in historical mode", () => {
    render(<CompletedTaskDetail task={task()} isHistorical />, {
      wrapper: TestWrapper,
    });

    expect(screen.getByTestId("task-detail-stage-body")).toHaveTextContent("Task Completed");
    const evidence = screen.getByTestId("task-detail-evidence");
    expect(evidence).toHaveTextContent("Latest Task Validation");
    expect(evidence).toHaveTextContent("Review History");
    expect(screen.queryByTestId("task-detail-actions")).not.toBeInTheDocument();
    expect(screen.queryByTestId("review-code-button")).not.toBeInTheDocument();
  });

  it("reopens through the current move contract without a legacy agent variant", async () => {
    render(<CompletedTaskDetail task={task()} />, { wrapper: TestWrapper });

    fireEvent.click(screen.getByTestId("reopen-task-button"));
    fireEvent.click(screen.getByTestId("confirm-rerun"));

    await waitFor(() => {
      expect(moveTask).toHaveBeenCalledWith("task-1", "ready", "Review new changes");
      expect(resumeExecution).toHaveBeenCalledWith("project-1");
    });
  });

  it.each([
    ["remote-default", true, AGENT_CONTROL_DISABLED_HINT],
    ["remote-agent", false, null],
    ["local", false, null],
  ] as const)("gates rerun in the %s environment", (environment, disabled, hint) => {
    setDetailViewEnvironment(environment);
    render(<CompletedTaskDetail task={task()} />, { wrapper: TestWrapper });

    const button = screen.getByTestId("reopen-task-button");
    if (disabled) {
      expect(button).toBeDisabled();
    } else {
      expect(button).toBeEnabled();
    }
    if (hint) {
      expect(screen.getByTestId("agent-gate-tooltip")).toHaveAttribute("data-agent-gated", "true");
    } else {
      expect(screen.queryByTestId("agent-gate-tooltip")).not.toBeInTheDocument();
    }
  });

  it("reports a resume-preflight failure after the move still succeeds", async () => {
    resumeExecution.mockRejectedValue(new Error("scheduler resume unavailable"));
    render(<CompletedTaskDetail task={task()} />, { wrapper: TestWrapper });

    fireEvent.click(screen.getByTestId("reopen-task-button"));
    fireEvent.click(screen.getByTestId("confirm-rerun"));

    await waitFor(() => expect(moveTask).toHaveBeenCalledWith("task-1", "ready", "Review new changes"));
    expect(await screen.findByText("scheduler resume unavailable")).toBeInTheDocument();
  });

  it("resumes execution after a remote-agent rerun through the reachable twin", async () => {
    setDetailViewEnvironment("remote-agent");
    render(<CompletedTaskDetail task={task()} />, { wrapper: TestWrapper });
    fireEvent.click(screen.getByTestId("reopen-task-button"));
    fireEvent.click(screen.getByTestId("confirm-rerun"));
    await waitFor(() =>
      expect(moveTask).toHaveBeenCalledWith("task-1", "ready", "Review new changes"),
    );
    expect(resumeExecution).toHaveBeenCalledWith("project-1");
    await waitFor(() => expect(screen.queryByTestId("confirm-rerun")).not.toBeInTheDocument());
  });
});
