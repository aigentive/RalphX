/**
 * QueuedTaskRow component tests
 */

import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { QueuedTaskRow } from "./QueuedTaskRow";
import type { QueuedTask } from "@/hooks/useQueuedTasks";

function createMockQueuedTask(overrides?: Partial<QueuedTask>): QueuedTask {
  return {
    id: "task-456",
    projectId: "project-1",
    category: "feature",
    title: "Queued Task",
    description: null,
    priority: 50,
    internalStatus: "ready",
    needsReviewPoint: false,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    startedAt: null,
    completedAt: null,
    archivedAt: null,
    blockedReason: null,
    planTitle: "Sprint 1",
    ...overrides,
  };
}

describe("QueuedTaskRow", () => {
  describe("rendering", () => {
    it("renders the task title", () => {
      render(<QueuedTaskRow position={1} task={createMockQueuedTask({ title: "Build login page" })} />);
      expect(screen.getByText("Build login page")).toBeInTheDocument();
    });

    it("renders the queue position", () => {
      render(<QueuedTaskRow position={3} task={createMockQueuedTask()} />);
      expect(screen.getByText("3")).toBeInTheDocument();
    });

    it("renders the plan title", () => {
      render(<QueuedTaskRow position={1} task={createMockQueuedTask({ planTitle: "My Plan" })} />);
      expect(screen.getByText("My Plan")).toBeInTheDocument();
    });
  });

  describe("click-to-navigate", () => {
    it("emits an Agent task navigation target when title is clicked", () => {
      const onNavigateToTask = vi.fn();
      const task = createMockQueuedTask({
        id: "task-queue-789",
        title: "Click me",
        ideationSessionId: "session-1",
        executionPlanId: "plan-branch-1",
      });
      render(
        <QueuedTaskRow
          position={2}
          task={task}
          onNavigateToTask={onNavigateToTask}
        />,
      );

      fireEvent.click(screen.getByText("Click me"));

      expect(onNavigateToTask).toHaveBeenCalledWith({
        taskId: "task-queue-789",
        source: "queued",
        projectId: "project-1",
        ideationSessionId: "session-1",
        executionPlanId: "plan-branch-1",
      });
      expect(onNavigateToTask).toHaveBeenCalledOnce();
    });

    it("title is rendered as a button element", () => {
      const task = createMockQueuedTask({ title: "Button Task" });
      render(<QueuedTaskRow position={1} task={task} />);

      const titleEl = screen.getByText("Button Task");
      expect(titleEl.tagName).toBe("BUTTON");
    });
  });
});
