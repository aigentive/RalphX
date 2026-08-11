/**
 * BasicTaskDetail component tests
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BasicTaskDetail } from "./BasicTaskDetail";
import type { Task } from "@/types/task";

vi.mock("@/hooks/useTaskSteps", () => ({
  useTaskSteps: vi.fn(),
}));

const mockConfirmation = {
  confirm: vi.fn(async () => true),
  confirmationDialogProps: {},
  ConfirmationDialog: () => null,
};

vi.mock("@/hooks/useConfirmation", () => ({
  useConfirmation: vi.fn(() => mockConfirmation),
}));

vi.mock("@/lib/tauri", () => ({
  api: {
    execution: {
      getStatus: vi.fn(async () => ({
        isPaused: false,
        haltMode: "running",
        runningCount: 0,
        maxConcurrent: 10,
        globalMaxConcurrent: 20,
        queuedCount: 0,
        canStartTask: true,
        ideationActive: 0,
        ideationIdle: 0,
        ideationWaiting: 0,
        ideationMaxProject: 0,
        ideationMaxGlobal: 0,
      })),
      resume: vi.fn(async () => ({
        success: true,
        status: {
          isPaused: false,
          haltMode: "running",
          runningCount: 0,
          maxConcurrent: 10,
          globalMaxConcurrent: 20,
          queuedCount: 0,
          canStartTask: true,
          ideationActive: 0,
          ideationIdle: 0,
          ideationWaiting: 0,
          ideationMaxProject: 0,
          ideationMaxGlobal: 0,
        },
      })),
    },
    tasks: {
      move: vi.fn(async () => ({})),
      restart: vi.fn(async () => ({
        type: "Success",
        task: {},
        category: "direct",
        resumedToStatus: "ready",
        disposition: "restarted_to_ready",
      })),
      unblock: vi.fn(async () => ({})),
    },
  },
}));

vi.mock("@/lib/task-actions/task-actions", () => ({
  stopExecutionRetry: vi.fn(async () => true),
}));

import { stopExecutionRetry } from "@/lib/task-actions/task-actions";
const mockStopExecutionRetry = vi.mocked(stopExecutionRetry);

const mockStepList = vi.fn(({ taskId, editable, hideCompletionNotes }) => (
  <div
    data-testid="mock-step-list"
    data-task-id={taskId}
    data-editable={String(editable)}
    data-hide-completion-notes={String(hideCompletionNotes)}
  />
));

vi.mock("../StepList", () => ({
  StepList: (props: unknown) => mockStepList(props),
}));

import { useTaskSteps } from "@/hooks/useTaskSteps";
import { api } from "@/lib/tauri";

const mockUseTaskSteps = vi.mocked(useTaskSteps);
const mockApiExecutionGetStatus = vi.mocked(api.execution.getStatus);
const mockApiExecutionResume = vi.mocked(api.execution.resume);
const mockApiTasksMove = vi.mocked(api.tasks.move);
const mockApiTasksRestart = vi.mocked(api.tasks.restart);
const mockApiTasksUnblock = vi.mocked(api.tasks.unblock);

const runningExecutionStatus = {
  isPaused: false,
  haltMode: "running" as const,
  runningCount: 0,
  maxConcurrent: 10,
  globalMaxConcurrent: 20,
  queuedCount: 0,
  canStartTask: true,
  ideationActive: 0,
  ideationIdle: 0,
  ideationWaiting: 0,
  ideationMaxProject: 0,
  ideationMaxGlobal: 0,
};

const stoppedExecutionStatus = {
  ...runningExecutionStatus,
  isPaused: true,
  haltMode: "stopped" as const,
  canStartTask: false,
};

function createTestTask(overrides?: Partial<Task>): Task {
  return {
    id: "task-123",
    projectId: "project-456",
    category: "feature",
    title: "Test Task Title",
    description: "Test task description",
    priority: 2,
    internalStatus: "ready",
    needsReviewPoint: false,
    sourceProposalId: null,
    planArtifactId: null,
    createdAt: "2026-01-28T12:00:00+00:00",
    updatedAt: "2026-01-28T12:00:00+00:00",
    startedAt: null,
    completedAt: null,
    archivedAt: null,
    ...overrides,
  };
}

function TestWrapper({ children }: { children: React.ReactNode }) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  });
  return (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
}

describe("BasicTaskDetail", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockApiExecutionGetStatus.mockResolvedValue(runningExecutionStatus);
    mockApiExecutionResume.mockResolvedValue({
      success: true,
      status: runningExecutionStatus,
    });
    mockApiTasksRestart.mockResolvedValue({
      type: "Success",
      task: {},
      category: "direct",
      resumedToStatus: "ready",
      disposition: "restarted_to_ready",
    });
    mockUseTaskSteps.mockReturnValue({
      data: [],
      isLoading: false,
      isError: false,
    } as ReturnType<typeof useTaskSteps>);
  });

  it("renders container and description content", () => {
    const task = createTestTask({ description: "Task description here" });
    render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

    expect(screen.getByTestId("basic-task-detail")).toBeInTheDocument();
    expect(screen.getByText("Description")).toBeInTheDocument();
    expect(screen.getByText("Task description here")).toBeInTheDocument();
  });

  it("renders placeholder when description is null", () => {
    const task = createTestTask({ description: null });
    render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

    expect(screen.getByText(/no description provided/i)).toBeInTheDocument();
  });

  it("shows loading state while fetching steps", () => {
    const task = createTestTask();
    mockUseTaskSteps.mockReturnValue({
      data: undefined,
      isLoading: true,
      isError: false,
    } as ReturnType<typeof useTaskSteps>);

    render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });
    expect(screen.getByTestId("basic-task-steps-loading")).toBeInTheDocument();
  });

  it("renders StepList section when task has steps", () => {
    const task = createTestTask();
    mockUseTaskSteps.mockReturnValue({
      data: [
        {
          id: "step-1",
          taskId: task.id,
          title: "Step 1",
          status: "pending",
          order: 0,
          createdAt: "2026-01-28T12:00:00+00:00",
          updatedAt: "2026-01-28T12:00:00+00:00",
        },
      ],
      isLoading: false,
      isError: false,
    } as ReturnType<typeof useTaskSteps>);

    render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

    expect(screen.getByTestId("basic-task-steps-section")).toBeInTheDocument();
    expect(screen.getByTestId("mock-step-list")).toBeInTheDocument();
    expect(screen.getByTestId("mock-step-list")).toHaveAttribute(
      "data-hide-completion-notes",
      "false"
    );
  });

  it("passes historical mode to StepList", () => {
    const task = createTestTask();
    mockUseTaskSteps.mockReturnValue({
      data: [
        {
          id: "step-1",
          taskId: task.id,
          title: "Step 1",
          status: "pending",
          order: 0,
          createdAt: "2026-01-28T12:00:00+00:00",
          updatedAt: "2026-01-28T12:00:00+00:00",
        },
      ],
      isLoading: false,
      isError: false,
    } as ReturnType<typeof useTaskSteps>);

    render(<BasicTaskDetail task={task} isHistorical />, { wrapper: TestWrapper });
    expect(screen.getByTestId("mock-step-list")).toHaveAttribute(
      "data-hide-completion-notes",
      "true"
    );
  });

  it("shows empty steps state when no steps exist", () => {
    const task = createTestTask();
    mockUseTaskSteps.mockReturnValue({
      data: [],
      isLoading: false,
      isError: false,
    } as ReturnType<typeof useTaskSteps>);

    render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });
    expect(screen.getByText("No steps defined yet")).toBeInTheDocument();
  });

  describe("failure reason display", () => {
    it("displays failure reason when task is in failed state with metadata", () => {
      const failureMetadata = JSON.stringify({
        failure_error: "Connection timeout",
        failure_details: "Task failed to connect to the server",
        is_timeout: true,
      });
      const task = createTestTask({
        internalStatus: "failed",
        metadata: failureMetadata,
      });

      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.getByTestId("failure-reason-section")).toBeInTheDocument();
      expect(screen.getByText("Failure Details")).toBeInTheDocument();
      expect(screen.getByText("Connection timeout")).toBeInTheDocument();
      expect(screen.getByText("timeout")).toBeInTheDocument();
      expect(screen.getByText("Task failed to connect to the server")).toBeInTheDocument();
    });

    it("hides failure reason section when task is not failed", () => {
      const task = createTestTask({ internalStatus: "ready" });
      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.queryByTestId("failure-reason-section")).not.toBeInTheDocument();
    });

    it("displays failure error without timeout badge when is_timeout is false", () => {
      const failureMetadata = JSON.stringify({
        failure_error: "Invalid input",
        failure_details: null,
        is_timeout: false,
      });
      const task = createTestTask({
        internalStatus: "failed",
        metadata: failureMetadata,
      });

      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.getByText("Invalid input")).toBeInTheDocument();
      expect(screen.queryByText("timeout")).not.toBeInTheDocument();
    });

    it("displays error without details when failure_details is null", () => {
      const failureMetadata = JSON.stringify({
        failure_error: "Process error",
        failure_details: null,
        is_timeout: false,
      });
      const task = createTestTask({
        internalStatus: "failed",
        metadata: failureMetadata,
      });

      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.getByText("Process error")).toBeInTheDocument();
      expect(screen.queryByText(/Task failed/)).not.toBeInTheDocument();
    });

    it("handles malformed JSON metadata gracefully (shows fallback)", () => {
      const task = createTestTask({
        internalStatus: "failed",
        metadata: "invalid json",
      });

      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.getByTestId("failure-reason-section")).toBeInTheDocument();
      expect(screen.getByText("Task execution failed. Error details were not recorded during the state transition.")).toBeInTheDocument();
    });

    it("displays generic fallback banner for failed task with null metadata", () => {
      const task = createTestTask({
        internalStatus: "failed",
        metadata: null,
      });

      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.getByTestId("failure-reason-section")).toBeInTheDocument();
      expect(screen.getByText("Task execution failed. Error details were not recorded during the state transition.")).toBeInTheDocument();
    });

    it("uses last agent error when failure_error is empty", () => {
      const failureMetadata = JSON.stringify({
        failure_error: "",
        is_timeout: false,
        last_agent_error:
          "Agent failed: sed: .artifacts/specs/p6-pr-list-affordances/tracker.md: No such file or directory",
        last_agent_error_context: "execution",
      });
      const task = createTestTask({
        internalStatus: "failed",
        metadata: failureMetadata,
      });

      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.getByTestId("failure-reason-section")).toBeInTheDocument();
      expect(screen.getByTestId("failure-error-message")).toHaveTextContent(
        "Agent failed: sed: .artifacts/specs/p6-pr-list-affordances/tracker.md: No such file or directory"
      );
      expect(
        screen.queryByText("Task execution failed. Error details were not recorded during the state transition.")
      ).not.toBeInTheDocument();
    });

    it("does not show stale agent error banner after task merged successfully", () => {
      const metadata = JSON.stringify({
        last_agent_error:
          "Agent failed: sed: frontend/src/components/pr/PrDetailPanel.tsx: No such file or directory",
        last_agent_error_context: "execution",
        last_agent_error_at: "2026-06-27T08:19:07+00:00",
      });
      const task = createTestTask({
        internalStatus: "merged",
        metadata,
        completedAt: "2026-06-27T08:20:00+00:00",
      });

      render(<BasicTaskDetail task={task} isHistorical />, {
        wrapper: TestWrapper,
      });

      expect(screen.queryByTestId("agent-error-section")).not.toBeInTheDocument();
      expect(screen.queryByText(/PrDetailPanel\.tsx/)).not.toBeInTheDocument();
    });

    it("displays blockedReason when failed with null metadata but blockedReason exists", () => {
      const task = createTestTask({
        internalStatus: "failed",
        metadata: null,
        blockedReason: "Dependency task failed to complete",
      });

      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.getByTestId("failure-reason-section")).toBeInTheDocument();
      expect(screen.getByText("Dependency task failed to complete")).toBeInTheDocument();
    });

    it("displays rich failure info when metadata is valid (existing behavior)", () => {
      const failureMetadata = JSON.stringify({
        failure_error: "Build script exited with code 1",
        failure_details: "npm run build failed",
        is_timeout: false,
      });
      const task = createTestTask({
        internalStatus: "failed",
        metadata: failureMetadata,
      });

      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.getByTestId("failure-reason-section")).toBeInTheDocument();
      expect(screen.getByText("Build script exited with code 1")).toBeInTheDocument();
      expect(screen.getByText("npm run build failed")).toBeInTheDocument();
    });

    it("displays generic fallback for failed task with malformed JSON metadata", () => {
      const task = createTestTask({
        internalStatus: "failed",
        metadata: "{ invalid json }",
      });

      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.getByTestId("failure-reason-section")).toBeInTheDocument();
      expect(screen.getByText("Task execution failed. Error details were not recorded during the state transition.")).toBeInTheDocument();
    });

    it("displays generic fallback for qa_failed task with null metadata", () => {
      const task = createTestTask({
        internalStatus: "qa_failed",
        metadata: null,
      });

      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.getByTestId("failure-reason-section")).toBeInTheDocument();
      expect(screen.getByText("Task execution failed. Error details were not recorded during the state transition.")).toBeInTheDocument();
    });
  });


  describe("restart action for terminal states", () => {
    beforeEach(() => {
      vi.clearAllMocks();
      mockUseTaskSteps.mockReturnValue({
        data: [],
        isLoading: false,
        isError: false,
      } as ReturnType<typeof useTaskSteps>);
      mockConfirmation.confirm = vi.fn(async () => true);
    });

    it("renders restart button for failed state", () => {
      const task = createTestTask({ internalStatus: "failed" });
      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.getByTestId("restart-button")).toBeInTheDocument();
      expect(screen.getByText("Restart")).toBeInTheDocument();
    });

    it("renders restart button for stopped state", () => {
      const task = createTestTask({ internalStatus: "stopped" });
      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.getByTestId("restart-button")).toBeInTheDocument();
      expect(screen.getByText("Restart")).toBeInTheDocument();
    });

    it("renders restart button for cancelled state", () => {
      const task = createTestTask({ internalStatus: "cancelled" });
      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.getByTestId("restart-button")).toBeInTheDocument();
      expect(screen.getByText("Restart")).toBeInTheDocument();
    });

    it("renders resume button for paused state", () => {
      const task = createTestTask({ internalStatus: "paused" });
      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.getByTestId("restart-button")).toBeInTheDocument();
      expect(screen.getByText("Resume")).toBeInTheDocument();
    });

    it("does not render restart button for backlog state", () => {
      const task = createTestTask({ internalStatus: "backlog" });
      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.queryByTestId("restart-button")).not.toBeInTheDocument();
    });

    it("does not render restart button for ready state", () => {
      const task = createTestTask({ internalStatus: "ready" });
      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.queryByTestId("restart-button")).not.toBeInTheDocument();
    });

    it("does not render restart button for blocked state", () => {
      const task = createTestTask({ internalStatus: "blocked" });
      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.queryByTestId("restart-button")).not.toBeInTheDocument();
    });

    it("does not render restart button when isHistorical is true", () => {
      const task = createTestTask({ internalStatus: "failed" });
      render(<BasicTaskDetail task={task} isHistorical />, {
        wrapper: TestWrapper,
      });

      expect(screen.queryByTestId("restart-button")).not.toBeInTheDocument();
    });

    it("routes failed restart through the backend recover-or-restart authority", async () => {
      const user = userEvent.setup();
      const task = createTestTask({ internalStatus: "failed" });
      mockConfirmation.confirm = vi.fn(async () => true);

      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      const button = screen.getByTestId("restart-button");
      await user.click(button);

      await waitFor(() => {
        expect(mockApiTasksRestart).toHaveBeenCalledWith(
          task.id,
          false,
          undefined
        );
      });
      expect(mockApiTasksMove).not.toHaveBeenCalled();
    });

    it("moves cancelled tasks to ready without a legacy agent variant", async () => {
      const user = userEvent.setup();
      const task = createTestTask({ internalStatus: "cancelled" });

      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      await user.click(screen.getByTestId("restart-button"));

      await waitFor(() => {
        expect(mockApiTasksMove).toHaveBeenCalledWith(task.id, "ready", undefined);
      });
      expect(mockApiTasksRestart).not.toHaveBeenCalled();
    });

    it.each([
      ["force-resume-button", "merging"],
      ["go-to-ready-button", "ready"],
    ])(
      "uses the current move contract for the %s validation choice",
      async (buttonTestId, expectedStatus) => {
        const user = userEvent.setup();
        const metadata = JSON.stringify({
          stop_metadata: JSON.stringify({
            stopped_from_status: "merging",
            stopped_at: "2026-07-21T00:00:00Z",
            stop_reason: "User requested",
          }),
        });
        const task = createTestTask({ internalStatus: "stopped", metadata });

        render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

        await user.type(screen.getByTestId("restart-note-textarea"), "Review new changes");
        await user.click(screen.getByTestId("restart-button"));
        expect(screen.getByTestId("resume-validation-dialog")).toBeInTheDocument();

        await user.click(screen.getByTestId(buttonTestId));

        await waitFor(() => {
          expect(mockApiTasksMove).toHaveBeenCalledWith(
            task.id,
            expectedStatus,
            "Review new changes"
          );
        });
        expect(mockApiTasksRestart).not.toHaveBeenCalled();
      }
    );

    it("resumes execution after restart when the project is globally stopped", async () => {
      const user = userEvent.setup();
      const task = createTestTask({ internalStatus: "failed" });
      mockConfirmation.confirm = vi.fn(async () => true);
      mockApiExecutionGetStatus.mockResolvedValue(stoppedExecutionStatus);

      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      await user.click(screen.getByTestId("restart-button"));

      await waitFor(() => {
        expect(mockApiTasksRestart).toHaveBeenCalledWith(
          task.id,
          false,
          undefined
        );
        expect(mockApiExecutionResume).toHaveBeenCalledWith(task.projectId);
      });
    });
  });

  describe("restart note textarea", () => {
    beforeEach(() => {
      vi.clearAllMocks();
      mockUseTaskSteps.mockReturnValue({
        data: [],
        isLoading: false,
        isError: false,
      } as ReturnType<typeof useTaskSteps>);
      mockConfirmation.confirm = vi.fn(async () => true);
    });

    it("renders note textarea for failed state", () => {
      const task = createTestTask({ internalStatus: "failed" });
      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.getByTestId("restart-note-textarea")).toBeInTheDocument();
    });

    it("renders note textarea for stopped state", () => {
      const task = createTestTask({ internalStatus: "stopped" });
      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.getByTestId("restart-note-textarea")).toBeInTheDocument();
    });

    it("renders note textarea for cancelled state", () => {
      const task = createTestTask({ internalStatus: "cancelled" });
      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.getByTestId("restart-note-textarea")).toBeInTheDocument();
    });

    it("renders note textarea for paused state", () => {
      const task = createTestTask({ internalStatus: "paused" });
      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.getByTestId("restart-note-textarea")).toBeInTheDocument();
    });

    it("does not render note textarea for ready state", () => {
      const task = createTestTask({ internalStatus: "ready" });
      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.queryByTestId("restart-note-textarea")).not.toBeInTheDocument();
    });

    it("passes note to api.tasks.restart when restarting failed task", async () => {
      const user = userEvent.setup();
      const task = createTestTask({ internalStatus: "failed" });

      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      const textarea = screen.getByTestId("restart-note-textarea");
      await user.type(textarea, "Fix the broken import");

      const button = screen.getByTestId("restart-button");
      await user.click(button);

      await waitFor(() => {
        expect(mockApiTasksRestart).toHaveBeenCalledWith(
          task.id,
          false,
          "Fix the broken import"
        );
      });
      expect(mockApiTasksMove).not.toHaveBeenCalled();
      expect(mockConfirmation.confirm).toHaveBeenCalledWith(
        expect.objectContaining({
          description: expect.stringContaining(
            "recover the existing completed work and continue to review"
          ),
        })
      );
      expect(await screen.findByTestId("restart-outcome")).toHaveTextContent(
        "A fresh execution attempt was started."
      );
    });

    it("shows a backend recovery block without moving or clearing the failed task", async () => {
      mockApiTasksRestart.mockResolvedValueOnce({
        type: "Blocked",
        warnings: [
          {
            code: "dirty_worktree",
            message: "The preserved worktree has uncommitted changes",
          },
        ],
      });
      const user = userEvent.setup();
      const task = createTestTask({ internalStatus: "failed" });
      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      await user.click(screen.getByTestId("restart-button"));

      expect(
        await screen.findByText("The preserved worktree has uncommitted changes")
      ).toBeInTheDocument();
      expect(mockApiTasksMove).not.toHaveBeenCalled();
    });

    it("passes undefined note when textarea is empty", async () => {
      const user = userEvent.setup();
      const task = createTestTask({ internalStatus: "failed" });

      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      const button = screen.getByTestId("restart-button");
      await user.click(button);

      await waitFor(() => {
        expect(mockApiTasksRestart).toHaveBeenCalledWith(
          task.id,
          false,
          undefined
        );
      });
    });

    it("note textarea accepts user input for stopped task", async () => {
      const user = userEvent.setup();
      // stop_metadata is a nested JSON string inside the outer metadata JSON object
      const stopMetadataInner = JSON.stringify({
        stopped_from_status: "executing",
        stopped_at: new Date().toISOString(),
        stop_reason: "User requested",
      });
      const stopMetadata = JSON.stringify({
        stop_metadata: stopMetadataInner,
      });
      const task = createTestTask({
        internalStatus: "stopped",
        metadata: stopMetadata,
      });

      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      const textarea = screen.getByTestId("restart-note-textarea");
      expect(textarea).toBeInTheDocument();
      await user.type(textarea, "Try a different approach");
      expect(textarea).toHaveValue("Try a different approach");
    });

    it("renders stop history section with stopped-from label, reason and time-ago", () => {
      const stopMetadataInner = JSON.stringify({
        stopped_from_status: "executing",
        stopped_at: new Date(Date.now() - 5 * 60 * 1000).toISOString(),
        stop_reason: "Manual halt during pipeline",
      });
      const metadata = JSON.stringify({ stop_metadata: stopMetadataInner });
      const task = createTestTask({ internalStatus: "stopped", metadata });

      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.getByText("Stopped from")).toBeInTheDocument();
      expect(screen.getByText(/Manual halt during pipeline/i)).toBeInTheDocument();
      expect(screen.getByText("Reason")).toBeInTheDocument();
    });
  });

  describe("attempt count display for failed tasks", () => {
    beforeEach(() => {
      vi.clearAllMocks();
      mockUseTaskSteps.mockReturnValue({
        data: [],
        isLoading: false,
        isError: false,
      } as ReturnType<typeof useTaskSteps>);
    });

    it("shows attempt count when auto_retry_count_executing is present in metadata", () => {
      const failureMetadata = JSON.stringify({
        failure_error: "Agent gave up",
        is_timeout: false,
        auto_retry_count_executing: 3,
      });
      const task = createTestTask({
        internalStatus: "failed",
        metadata: failureMetadata,
      });

      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.getByTestId("attempt-count")).toBeInTheDocument();
      expect(screen.getByText(/Failed after 3 attempts/)).toBeInTheDocument();
    });

    it("shows singular 'attempt' for count of 1", () => {
      const failureMetadata = JSON.stringify({
        failure_error: "Agent gave up",
        is_timeout: false,
        auto_retry_count_executing: 1,
      });
      const task = createTestTask({
        internalStatus: "failed",
        metadata: failureMetadata,
      });

      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.getByText(/Failed after 1 attempt/)).toBeInTheDocument();
    });

    it("does not show attempt count when auto_retry_count_executing is 0", () => {
      const failureMetadata = JSON.stringify({
        failure_error: "Agent gave up",
        is_timeout: false,
        auto_retry_count_executing: 0,
      });
      const task = createTestTask({
        internalStatus: "failed",
        metadata: failureMetadata,
      });

      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.queryByTestId("attempt-count")).not.toBeInTheDocument();
    });

    it("shows timeout badge alongside attempt count for timeout failures", () => {
      const failureMetadata = JSON.stringify({
        failure_error: "Agent timed out",
        is_timeout: true,
        auto_retry_count_executing: 5,
      });
      const task = createTestTask({
        internalStatus: "failed",
        metadata: failureMetadata,
      });

      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.getByTestId("attempt-count")).toBeInTheDocument();
      expect(screen.getByTestId("timeout-badge")).toBeInTheDocument();
      expect(screen.getByText("timeout")).toBeInTheDocument();
    });

    it("shows timeout badge even when attempt count is 0", () => {
      const failureMetadata = JSON.stringify({
        failure_error: "Agent timed out",
        is_timeout: true,
        auto_retry_count_executing: 0,
      });
      const task = createTestTask({
        internalStatus: "failed",
        metadata: failureMetadata,
      });

      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.getByTestId("timeout-badge")).toBeInTheDocument();
      expect(screen.queryByTestId("attempt-count")).not.toBeInTheDocument();
    });

    it("shows failure error message via data-testid", () => {
      const failureMetadata = JSON.stringify({
        failure_error: "Build script failed",
        is_timeout: false,
        auto_retry_count_executing: 2,
      });
      const task = createTestTask({
        internalStatus: "failed",
        metadata: failureMetadata,
      });

      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.getByTestId("failure-error-message")).toBeInTheDocument();
      expect(screen.getByTestId("failure-error-message")).toHaveTextContent("Build script failed");
    });

    it("shows failure details via data-testid when present", () => {
      const failureMetadata = JSON.stringify({
        failure_error: "Build failed",
        failure_details: "npm run build exited with code 1",
        is_timeout: false,
        auto_retry_count_executing: 1,
      });
      const task = createTestTask({
        internalStatus: "failed",
        metadata: failureMetadata,
      });

      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.getByTestId("failure-details")).toHaveTextContent(
        "npm run build exited with code 1"
      );
    });
  });

  describe("execution_recovery sub-states for failed tasks", () => {
    beforeEach(() => {
      vi.clearAllMocks();
      mockUseTaskSteps.mockReturnValue({
        data: [],
        isLoading: false,
        isError: false,
      } as ReturnType<typeof useTaskSteps>);
      mockConfirmation.confirm = vi.fn(async () => true);
    });

    describe("sub-state 1: auto-retrying (last_state=retrying, stop_retrying=false)", () => {
      it("shows auto-retrying section when last_state is retrying", () => {
        const metadata = JSON.stringify({
          execution_recovery: {
            last_state: "retrying",
            stop_retrying: false,
            events: [],
          },
        });
        const task = createTestTask({ internalStatus: "failed", metadata });

        render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

        expect(screen.getByTestId("auto-retrying-section")).toBeInTheDocument();
        expect(screen.getByTestId("auto-retry-badge")).toBeInTheDocument();
        expect(screen.getByTestId("stop-retrying-button")).toBeInTheDocument();
      });

      it("shows 'Auto-retrying' badge text with no count when events list is empty", () => {
        const metadata = JSON.stringify({
          execution_recovery: {
            last_state: "retrying",
            stop_retrying: false,
            events: [],
          },
        });
        const task = createTestTask({ internalStatus: "failed", metadata });

        render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

        expect(screen.getByTestId("auto-retry-badge")).toHaveTextContent("Auto-retrying");
        expect(screen.getByTestId("auto-retry-badge")).not.toHaveTextContent("attempt");
      });

      it("shows attempt count from auto_retry_triggered events", () => {
        const metadata = JSON.stringify({
          execution_recovery: {
            last_state: "retrying",
            stop_retrying: false,
            events: [
              { kind: "auto_retry_triggered", timestamp: "2026-01-01T00:00:00Z" },
              { kind: "auto_retry_triggered", timestamp: "2026-01-01T00:01:00Z" },
            ],
          },
        });
        const task = createTestTask({ internalStatus: "failed", metadata });

        render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

        expect(screen.getByTestId("auto-retry-badge")).toHaveTextContent("attempt 2");
      });

      it("does not count non-retry events", () => {
        const metadata = JSON.stringify({
          execution_recovery: {
            last_state: "retrying",
            stop_retrying: false,
            events: [
              { kind: "auto_retry_triggered", timestamp: "2026-01-01T00:00:00Z" },
              { kind: "stop_requested", timestamp: "2026-01-01T00:01:00Z" },
              { kind: "AutoRetryTriggered", timestamp: "2026-01-01T00:02:00Z" }, // wrong case — not counted
            ],
          },
        });
        const task = createTestTask({ internalStatus: "failed", metadata });

        render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

        // Only 1 valid snake_case event should be counted
        expect(screen.getByTestId("auto-retry-badge")).toHaveTextContent("attempt 1");
      });

      it("calls stopExecutionRetry on Stop Retrying button click", async () => {
        const user = userEvent.setup();
        const metadata = JSON.stringify({
          execution_recovery: {
            last_state: "retrying",
            stop_retrying: false,
            events: [],
          },
        });
        const task = createTestTask({ internalStatus: "failed", metadata });

        render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

        await user.click(screen.getByTestId("stop-retrying-button"));

        await waitFor(() => {
          expect(mockStopExecutionRetry).toHaveBeenCalledWith(task.id);
        });
      });
    });

    describe("sub-state 2: permanently failed (last_state=failed or stop_retrying=true)", () => {
      it("does not show auto-retrying section when last_state is failed", () => {
        const metadata = JSON.stringify({
          execution_recovery: {
            last_state: "failed",
            stop_retrying: false,
            events: [],
          },
        });
        const task = createTestTask({ internalStatus: "failed", metadata });

        render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

        expect(screen.queryByTestId("auto-retrying-section")).not.toBeInTheDocument();
      });

      it("does not show auto-retrying section when stop_retrying is true", () => {
        const metadata = JSON.stringify({
          execution_recovery: {
            last_state: "retrying",
            stop_retrying: true,
            events: [],
          },
        });
        const task = createTestTask({ internalStatus: "failed", metadata });

        render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

        expect(screen.queryByTestId("auto-retrying-section")).not.toBeInTheDocument();
      });

      it("shows standard failed UI (failure details section) for permanently failed state", () => {
        const metadata = JSON.stringify({
          failure_error: "Max retries exceeded",
          is_timeout: false,
          execution_recovery: {
            last_state: "failed",
            stop_retrying: false,
            events: [],
          },
        });
        const task = createTestTask({ internalStatus: "failed", metadata });

        render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

        expect(screen.queryByTestId("auto-retrying-section")).not.toBeInTheDocument();
        expect(screen.getByTestId("failure-reason-section")).toBeInTheDocument();
        expect(screen.getByText("Max retries exceeded")).toBeInTheDocument();
      });
    });

    describe("sub-state 3: legacy (no execution_recovery metadata)", () => {
      it("does not show auto-retrying section when no execution_recovery in metadata", () => {
        const metadata = JSON.stringify({
          failure_error: "Something went wrong",
          is_timeout: false,
        });
        const task = createTestTask({ internalStatus: "failed", metadata });

        render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

        expect(screen.queryByTestId("auto-retrying-section")).not.toBeInTheDocument();
      });

      it("does not show auto-retrying section when metadata is null", () => {
        const task = createTestTask({ internalStatus: "failed", metadata: null });

        render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

        expect(screen.queryByTestId("auto-retrying-section")).not.toBeInTheDocument();
      });

      it("shows standard failed UI for legacy tasks with no execution_recovery", () => {
        const task = createTestTask({ internalStatus: "failed", metadata: null });

        render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

        expect(screen.queryByTestId("auto-retrying-section")).not.toBeInTheDocument();
        expect(screen.getByTestId("failure-reason-section")).toBeInTheDocument();
      });
    });
  });

  describe("dependency-failed StatusBanner for blocked tasks", () => {
    beforeEach(() => {
      vi.clearAllMocks();
      mockUseTaskSteps.mockReturnValue({
        data: [],
        isLoading: false,
        isError: false,
      } as ReturnType<typeof useTaskSteps>);
      mockConfirmation.confirm = vi.fn(async () => true);
    });

    it("shows dependency-failed banner when blockedReason indicates failed dependency", () => {
      const task = createTestTask({
        internalStatus: "blocked",
        blockedReason: "Dependency Setup Infrastructure failed.",
      });

      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.getByTestId("dependency-failed-banner")).toBeInTheDocument();
      expect(screen.getByText("Dependency Failed")).toBeInTheDocument();
      expect(
        screen.getByText(/"Setup Infrastructure" failed/)
      ).toBeInTheDocument();
    });

    it("does not show dependency-failed banner when blockedReason is a normal waiting reason", () => {
      const task = createTestTask({
        internalStatus: "blocked",
        blockedReason: "Waiting for: Setup Infrastructure",
      });

      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.queryByTestId("dependency-failed-banner")).not.toBeInTheDocument();
    });

    it("does not show dependency-failed banner when task is not blocked", () => {
      const task = createTestTask({
        internalStatus: "ready",
        blockedReason: null,
      });

      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.queryByTestId("dependency-failed-banner")).not.toBeInTheDocument();
    });

    it("does not show dependency-failed banner when blockedReason is null", () => {
      const task = createTestTask({
        internalStatus: "blocked",
        blockedReason: null,
      });

      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.queryByTestId("dependency-failed-banner")).not.toBeInTheDocument();
    });

    it("shows unblock-warning-card with unblock button for failed dependency", () => {
      const task = createTestTask({
        internalStatus: "blocked",
        blockedReason: "Dependency Build Frontend failed.",
      });

      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.getByTestId("unblock-warning-card")).toBeInTheDocument();
      expect(screen.getByTestId("unblock-button")).toBeInTheDocument();
      expect(screen.getByText("Unblock Anyway")).toBeInTheDocument();
    });

    it("hides unblock-warning-card in historical mode", () => {
      const task = createTestTask({
        internalStatus: "blocked",
        blockedReason: "Dependency Build Frontend failed.",
      });

      render(<BasicTaskDetail task={task} isHistorical />, { wrapper: TestWrapper });

      expect(screen.queryByTestId("unblock-warning-card")).not.toBeInTheDocument();
    });

    it("shows confirmation dialog with warning text on unblock click", async () => {
      const user = userEvent.setup();
      const task = createTestTask({
        internalStatus: "blocked",
        blockedReason: "Dependency Build Frontend failed.",
      });

      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      await user.click(screen.getByTestId("unblock-button"));

      await waitFor(() => {
        expect(mockConfirmation.confirm).toHaveBeenCalledWith(
          expect.objectContaining({
            title: "Unblock despite failed dependency?",
            description: expect.stringContaining("Build Frontend"),
          })
        );
      });
    });

    it("calls api.tasks.unblock when confirmed", async () => {
      const user = userEvent.setup();
      mockConfirmation.confirm = vi.fn(async () => true);
      const task = createTestTask({
        internalStatus: "blocked",
        blockedReason: "Dependency Build Frontend failed.",
      });

      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      await user.click(screen.getByTestId("unblock-button"));

      await waitFor(() => {
        expect(mockApiTasksUnblock).toHaveBeenCalledWith(task.id);
      });
    });

    it("does not call api.tasks.unblock when confirmation is cancelled", async () => {
      const user = userEvent.setup();
      mockConfirmation.confirm = vi.fn(async () => false);
      const task = createTestTask({
        internalStatus: "blocked",
        blockedReason: "Dependency Build Frontend failed.",
      });

      render(<BasicTaskDetail task={task} />, { wrapper: TestWrapper });

      await user.click(screen.getByTestId("unblock-button"));

      await waitFor(() => {
        expect(mockConfirmation.confirm).toHaveBeenCalled();
        expect(mockApiTasksUnblock).not.toHaveBeenCalled();
      });
    });
  });
});
