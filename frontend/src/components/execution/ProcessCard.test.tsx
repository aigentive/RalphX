/**
 * ProcessCard component tests
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, act } from "@testing-library/react";
import { ProcessCard } from "./ProcessCard";
import type { RunningProcess } from "@/api/running-processes";

// Mock process data helper
function createMockProcess(overrides?: Partial<RunningProcess>): RunningProcess {
  return {
    taskId: "task-123",
    title: "Test Task",
    internalStatus: "executing",
    stepProgress: {
      taskId: "task-123",
      total: 7,
      completed: 2,
      inProgress: 1,
      pending: 4,
      skipped: 0,
      failed: 0,
      currentStep: {
        id: "step-3",
        taskId: "task-123",
        title: "Step 3",
        description: null,
        status: "in_progress",
        sortOrder: 2,
        dependsOn: null,
        createdBy: "user",
        completionNote: null,
        createdAt: "2026-02-11T12:00:00Z",
        updatedAt: "2026-02-11T12:00:00Z",
        startedAt: "2026-02-11T12:00:00Z",
        completedAt: null,
      },
      nextStep: null,
      percentComplete: 28.57,
    },
    elapsedSeconds: 134,
    triggerOrigin: "scheduler",
    taskBranch: "ralphx/app/task-123",
    ...overrides,
  };
}

describe("ProcessCard", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe("basic rendering", () => {
    it("renders with correct test id", () => {
      const process = createMockProcess();
      render(
        <ProcessCard
          process={process}
          onPause={vi.fn()}
          onStop={vi.fn()}
        />
      );
      expect(screen.getByTestId("process-card-task-123")).toBeInTheDocument();
    });

    it("displays task title", () => {
      const process = createMockProcess({ title: "Implement auth system" });
      render(
        <ProcessCard
          process={process}
          onPause={vi.fn()}
          onStop={vi.fn()}
        />
      );
      expect(screen.getByText("Implement auth system")).toBeInTheDocument();
    });

    it("displays status badge for executing status", () => {
      const process = createMockProcess({ internalStatus: "executing" });
      render(
        <ProcessCard
          process={process}
          onPause={vi.fn()}
          onStop={vi.fn()}
        />
      );
      expect(screen.getByText("Executing")).toBeInTheDocument();
    });

    it("displays status badge for re_executing status", () => {
      const process = createMockProcess({ internalStatus: "re_executing" });
      render(
        <ProcessCard
          process={process}
          onPause={vi.fn()}
          onStop={vi.fn()}
        />
      );
      expect(screen.getByText("Re-executing")).toBeInTheDocument();
    });

    it("displays status badge for reviewing status", () => {
      const process = createMockProcess({ internalStatus: "reviewing" });
      render(
        <ProcessCard
          process={process}
          onPause={vi.fn()}
          onStop={vi.fn()}
        />
      );
      expect(screen.getByText("Reviewing")).toBeInTheDocument();
    });
  });

  describe("step progress display", () => {
    it("displays completed/total steps regardless of currentStep", () => {
      const process = createMockProcess({
        stepProgress: {
          taskId: "task-123",
          total: 7,
          completed: 2,
          inProgress: 1,
          pending: 4,
          skipped: 0,
          failed: 0,
          currentStep: {
            id: "step-3",
            taskId: "task-123",
            title: "Step 3",
            description: null,
            status: "in_progress",
            sortOrder: 2,
            dependsOn: null,
            createdBy: "user",
            completionNote: null,
            createdAt: "2026-02-11T12:00:00Z",
            updatedAt: "2026-02-11T12:00:00Z",
            startedAt: "2026-02-11T12:00:00Z",
            completedAt: null,
          },
          nextStep: null,
          percentComplete: 28.57,
        },
      });
      render(
        <ProcessCard
          process={process}
          onPause={vi.fn()}
          onStop={vi.fn()}
        />
      );
      // shows completed/total regardless of which step is active
      expect(screen.getByText(/2\/7 steps/)).toBeInTheDocument();
    });

    it("displays completed/total steps when no currentStep", () => {
      const process = createMockProcess({
        stepProgress: {
          taskId: "task-123",
          total: 7,
          completed: 5,
          inProgress: 0,
          pending: 2,
          skipped: 0,
          failed: 0,
          currentStep: null,
          nextStep: null,
          percentComplete: 71.43,
        },
      });
      render(
        <ProcessCard
          process={process}
          onPause={vi.fn()}
          onStop={vi.fn()}
        />
      );
      expect(screen.getByText(/5\/7 steps/)).toBeInTheDocument();
    });

    it("excludes skipped steps from the displayed total", () => {
      const process = createMockProcess({
        stepProgress: {
          taskId: "task-123",
          total: 5,
          completed: 1,
          inProgress: 1,
          pending: 1,
          skipped: 2,
          failed: 0,
          currentStep: null,
          nextStep: null,
          percentComplete: 33.33,
        },
      });
      render(
        <ProcessCard
          process={process}
          onPause={vi.fn()}
          onStop={vi.fn()}
        />
      );
      expect(screen.getByText(/1\/3 steps/)).toBeInTheDocument();
    });

    it("handles process without step progress", () => {
      const process = createMockProcess({ stepProgress: null });
      render(
        <ProcessCard
          process={process}
          onPause={vi.fn()}
          onStop={vi.fn()}
        />
      );
      // Should not display step progress
      expect(screen.queryByText(/Step/)).not.toBeInTheDocument();
    });
  });

  describe("elapsed time ticker", () => {
    it("displays initial elapsed time correctly", () => {
      const process = createMockProcess({ elapsedSeconds: 134 });
      render(
        <ProcessCard
          process={process}
          onPause={vi.fn()}
          onStop={vi.fn()}
        />
      );
      // 134 seconds = 2m 14s
      expect(screen.getByText(/2m 14s/)).toBeInTheDocument();
    });

    it("updates elapsed time every second", async () => {
      const process = createMockProcess({ elapsedSeconds: 60 });
      render(
        <ProcessCard
          process={process}
          onPause={vi.fn()}
          onStop={vi.fn()}
        />
      );

      // Initial: 1m 0s
      expect(screen.getByText(/1m 0s/)).toBeInTheDocument();

      // Advance 1 second and wait for state update
      await act(async () => {
        await vi.advanceTimersByTimeAsync(1000);
      });
      expect(screen.getByText(/1m 1s/)).toBeInTheDocument();

      // Advance another second and wait for state update
      await act(async () => {
        await vi.advanceTimersByTimeAsync(1000);
      });
      expect(screen.getByText(/1m 2s/)).toBeInTheDocument();
    });

    it("formats elapsed time under 1 minute as seconds only", () => {
      const process = createMockProcess({ elapsedSeconds: 45 });
      render(
        <ProcessCard
          process={process}
          onPause={vi.fn()}
          onStop={vi.fn()}
        />
      );
      expect(screen.getByText(/45s/)).toBeInTheDocument();
    });

    it("handles null elapsed time", () => {
      const process = createMockProcess({ elapsedSeconds: null });
      render(
        <ProcessCard
          process={process}
          onPause={vi.fn()}
          onStop={vi.fn()}
        />
      );
      expect(screen.getByText(/—/)).toBeInTheDocument();
    });
  });

  describe("trigger origin badge", () => {
    it("displays scheduler origin badge", () => {
      const process = createMockProcess({ triggerOrigin: "scheduler" });
      render(
        <ProcessCard
          process={process}
          onPause={vi.fn()}
          onStop={vi.fn()}
        />
      );
      expect(screen.getByText("Scheduled")).toBeInTheDocument();
    });

    it("displays revision origin badge", () => {
      const process = createMockProcess({ triggerOrigin: "revision" });
      render(
        <ProcessCard
          process={process}
          onPause={vi.fn()}
          onStop={vi.fn()}
        />
      );
      expect(screen.getByText("Revision")).toBeInTheDocument();
    });

    it("displays recovery origin badge", () => {
      const process = createMockProcess({ triggerOrigin: "recovery" });
      render(
        <ProcessCard
          process={process}
          onPause={vi.fn()}
          onStop={vi.fn()}
        />
      );
      expect(screen.getByText("Recovered")).toBeInTheDocument();
    });

    it("displays retry origin badge", () => {
      const process = createMockProcess({ triggerOrigin: "retry" });
      render(
        <ProcessCard
          process={process}
          onPause={vi.fn()}
          onStop={vi.fn()}
        />
      );
      expect(screen.getByText("Retried")).toBeInTheDocument();
    });

    it("displays QA origin badge", () => {
      const process = createMockProcess({ triggerOrigin: "qa" });
      render(
        <ProcessCard
          process={process}
          onPause={vi.fn()}
          onStop={vi.fn()}
        />
      );
      expect(screen.getByText("QA Cycle")).toBeInTheDocument();
    });

    it("does not display origin badge when null", () => {
      const process = createMockProcess({ triggerOrigin: null });
      render(
        <ProcessCard
          process={process}
          onPause={vi.fn()}
          onStop={vi.fn()}
        />
      );
      expect(screen.queryByText("Scheduled")).not.toBeInTheDocument();
    });
  });

  describe("branch name display", () => {
    it("displays branch name when provided", () => {
      const process = createMockProcess({ taskBranch: "ralphx/app/task-abc123" });
      render(
        <ProcessCard
          process={process}
          onPause={vi.fn()}
          onStop={vi.fn()}
        />
      );
      expect(screen.getByText("ralphx/app/task-abc123")).toBeInTheDocument();
    });

    it("does not display branch name when null", () => {
      const process = createMockProcess({ taskBranch: null });
      render(
        <ProcessCard
          process={process}
          onPause={vi.fn()}
          onStop={vi.fn()}
        />
      );
      expect(screen.queryByText(/ralphx/)).not.toBeInTheDocument();
    });
  });

  describe("pause and stop buttons", () => {
    it("renders pause button with correct test id", () => {
      const process = createMockProcess();
      render(
        <ProcessCard
          process={process}
          onPause={vi.fn()}
          onStop={vi.fn()}
        />
      );
      expect(screen.getByTestId("pause-button-task-123")).toBeInTheDocument();
    });

    it("renders stop button with correct test id", () => {
      const process = createMockProcess();
      render(
        <ProcessCard
          process={process}
          onPause={vi.fn()}
          onStop={vi.fn()}
        />
      );
      expect(screen.getByTestId("stop-button-task-123")).toBeInTheDocument();
    });

    it("calls onPause with task ID when pause button clicked", () => {
      const onPause = vi.fn();
      const process = createMockProcess({ taskId: "task-abc" });
      render(
        <ProcessCard
          process={process}
          onPause={onPause}
          onStop={vi.fn()}
        />
      );

      fireEvent.click(screen.getByTestId("pause-button-task-abc"));
      expect(onPause).toHaveBeenCalledWith("task-abc");
      expect(onPause).toHaveBeenCalledOnce();
    });

    it("calls onStop with task ID when stop button clicked", () => {
      const onStop = vi.fn();
      const process = createMockProcess({ taskId: "task-xyz" });
      render(
        <ProcessCard
          process={process}
          onPause={vi.fn()}
          onStop={onStop}
        />
      );

      fireEvent.click(screen.getByTestId("stop-button-task-xyz"));
      expect(onStop).toHaveBeenCalledWith("task-xyz");
      expect(onStop).toHaveBeenCalledOnce();
    });

    it("disables buttons when isLoading is true", () => {
      const process = createMockProcess();
      render(
        <ProcessCard
          process={process}
          onPause={vi.fn()}
          onStop={vi.fn()}
          isLoading
        />
      );

      const pauseButton = screen.getByTestId("pause-button-task-123");
      const stopButton = screen.getByTestId("stop-button-task-123");

      expect(pauseButton).toBeDisabled();
      expect(stopButton).toBeDisabled();
    });

    it("enables buttons when isLoading is false", () => {
      const process = createMockProcess();
      render(
        <ProcessCard
          process={process}
          onPause={vi.fn()}
          onStop={vi.fn()}
          isLoading={false}
        />
      );

      const pauseButton = screen.getByTestId("pause-button-task-123");
      const stopButton = screen.getByTestId("stop-button-task-123");

      expect(pauseButton).not.toBeDisabled();
      expect(stopButton).not.toBeDisabled();
    });
  });

  describe("click-to-navigate", () => {
    it("calls onNavigate with process.taskId when card row is clicked", () => {
      const onNavigate = vi.fn();
      const process = createMockProcess({ taskId: "task-nav-123", title: "Navigate to me" });
      render(
        <ProcessCard
          process={process}
          onPause={vi.fn()}
          onStop={vi.fn()}
          onNavigate={onNavigate}
        />
      );

      fireEvent.click(screen.getByTestId("process-card-task-nav-123"));

      expect(onNavigate).toHaveBeenCalledWith("task-nav-123");
      expect(onNavigate).toHaveBeenCalledOnce();
    });

    it("title is rendered as a span element", () => {
      const process = createMockProcess({ title: "My Task" });
      render(
        <ProcessCard
          process={process}
          onPause={vi.fn()}
          onStop={vi.fn()}
        />
      );

      const titleEl = screen.getByText("My Task");
      expect(titleEl.tagName).toBe("SPAN");
    });

    it("clicking pause button does NOT call onNavigate (stopPropagation)", () => {
      const onNavigate = vi.fn();
      const process = createMockProcess({ taskId: "task-sp-pause" });
      render(
        <ProcessCard
          process={process}
          onPause={vi.fn()}
          onStop={vi.fn()}
          onNavigate={onNavigate}
        />
      );

      fireEvent.click(screen.getByTestId("pause-button-task-sp-pause"));
      expect(onNavigate).not.toHaveBeenCalled();
    });

    it("clicking stop button does NOT call onNavigate (stopPropagation)", () => {
      const onNavigate = vi.fn();
      const process = createMockProcess({ taskId: "task-sp-stop" });
      render(
        <ProcessCard
          process={process}
          onPause={vi.fn()}
          onStop={vi.fn()}
          onNavigate={onNavigate}
        />
      );

      fireEvent.click(screen.getByTestId("stop-button-task-sp-stop"));
      expect(onNavigate).not.toHaveBeenCalled();
    });
  });
});
