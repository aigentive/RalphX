/**
 * Tests for TaskCard component
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { DndContext } from "@dnd-kit/core";
import { createMockTask } from "@/test/mock-data";
import { TaskCard } from "./TaskCard";
import type { PlanBranch } from "@/api/plan-branch.types";
import type { QAPrepStatus } from "@/types/qa-config";
import type { QAOverallStatus } from "@/types/qa";

const mockPlanBranchState = vi.hoisted((): { current: PlanBranch | null } => ({
  current: null,
}));

vi.mock("@/hooks/usePlanBranchForTask", () => ({
  usePlanBranchForTask: vi.fn(() => ({ data: mockPlanBranchState.current })),
}));

function createTestPlanBranch(overrides?: Partial<PlanBranch>): PlanBranch {
  return {
    id: "plan-branch-123",
    planArtifactId: "artifact-123",
    sessionId: "session-123",
    projectId: "project-456",
    branchName: "ralphx/ralphx/plan-a3612efd",
    sourceBranch: "main",
    status: "active",
    mergeTaskId: "task-123",
    createdAt: "2026-01-28T12:00:00+00:00",
    mergedAt: null,
    prNumber: 68,
    prUrl: "https://github.com/aigentive/ralphx.app/pull/68",
    prDraft: false,
    prPushStatus: "pushed",
    prStatus: "Open",
    prPollingActive: true,
    prEligible: true,
    baseBranchOverride: null,
    ...overrides,
  };
}

// Create a new QueryClient for each test
const createTestQueryClient = () =>
  new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  });

// Wrapper component for dnd-kit and QueryClient context
function DndWrapper({ children }: { children: React.ReactNode }) {
  const queryClient = createTestQueryClient();
  return (
    <QueryClientProvider client={queryClient}>
      <DndContext>{children}</DndContext>
    </QueryClientProvider>
  );
}

describe("TaskCard", () => {
  beforeEach(() => {
    mockPlanBranchState.current = null;
  });

  describe("rendering", () => {
    it("should render with data-testid", () => {
      const task = createMockTask({ id: "task-123" });
      render(<TaskCard task={task} />, { wrapper: DndWrapper });
      expect(screen.getByTestId("task-card-task-123")).toBeInTheDocument();
    });

    it("should render task title", () => {
      const task = createMockTask({ title: "My Test Task" });
      render(<TaskCard task={task} />, { wrapper: DndWrapper });
      expect(screen.getByText("My Test Task")).toBeInTheDocument();
    });

    it("should render category badge", () => {
      const task = createMockTask({ category: "feature" });
      render(<TaskCard task={task} />, { wrapper: DndWrapper });
      expect(screen.getByText("feature")).toBeInTheDocument();
    });

    it("renders plan merge category with a friendly label", () => {
      const task = createMockTask({ category: "plan_merge" });
      render(<TaskCard task={task} />, { wrapper: DndWrapper });

      expect(screen.getByText("Plan merge")).toBeInTheDocument();
      expect(screen.queryByText("plan_merge")).not.toBeInTheDocument();
    });

    it("shows merged PR state instead of review prompt for merged plan branches", () => {
      mockPlanBranchState.current = createTestPlanBranch({
        status: "merged",
        prStatus: "Open",
      });
      const task = createMockTask({
        category: "plan_merge",
        internalStatus: "merged",
      });

      render(<TaskCard task={task} />, { wrapper: DndWrapper });

      expect(screen.getByText("Merged PR")).toBeInTheDocument();
      expect(screen.queryByText("Review PR")).not.toBeInTheDocument();
    });

    it("shows closed PR state when the PR was closed without merge", () => {
      mockPlanBranchState.current = createTestPlanBranch({ prStatus: "Closed" });
      const task = createMockTask({
        category: "plan_merge",
        internalStatus: "waiting_on_pr",
      });

      render(<TaskCard task={task} />, { wrapper: DndWrapper });

      expect(screen.getByText("Closed PR")).toBeInTheDocument();
      expect(screen.queryByText("Review PR")).not.toBeInTheDocument();
    });

    it("should render flat v29a card chrome without a left status stripe", () => {
      const task = createMockTask({ priority: 2 });
      render(<TaskCard task={task} />, { wrapper: DndWrapper });
      const card = screen.getByTestId(`task-card-${task.id}`);
      expect(card.style.borderLeft).toBe("");
      expect(card.getAttribute("style")).toContain("background-color: var(--kanban-card-bg, #232329)");
      expect(card.getAttribute("style")).toContain("border-color: var(--kanban-card-border, #34343C)");
      expect(card.style.borderStyle).toBe("solid");
      expect(card.style.borderWidth).toBe("1px");
    });

    it("should tint completed cards with the v29a full-card success surface", () => {
      const task = createMockTask({ internalStatus: "approved" });
      render(<TaskCard task={task} />, { wrapper: DndWrapper });
      const card = screen.getByTestId(`task-card-${task.id}`);
      expect(card.getAttribute("style")).toContain("background-color: var(--kanban-card-success-bg");
      expect(card.getAttribute("style")).toContain("border-color: var(--kanban-card-success-border");
      expect(card.style.borderStyle).toBe("solid");
      expect(card.style.borderWidth).toBe("1px");
    });

    it("should tint merge workflow cards with the v29a warning surface", () => {
      const task = createMockTask({
        category: "plan_merge",
        internalStatus: "pending_merge",
      });
      render(<TaskCard task={task} />, { wrapper: DndWrapper });
      const card = screen.getByTestId(`task-card-${task.id}`);
      expect(card.getAttribute("style")).toContain("background-color: var(--kanban-card-warning-bg");
      expect(card.getAttribute("style")).toContain("border-color: var(--kanban-card-warning-border");
      expect(card.style.borderStyle).toBe("solid");
      expect(card.style.borderWidth).toBe("1px");
    });

    it("should preserve status tint when a completed or escalated card is selected", () => {
      const completedTask = createMockTask({ id: "completed-task", internalStatus: "approved" });
      const escalatedTask = createMockTask({ id: "escalated-task", internalStatus: "escalated" });

      render(
        <>
          <TaskCard task={completedTask} isSelected />
          <TaskCard task={escalatedTask} isSelected />
        </>,
        { wrapper: DndWrapper }
      );

      const completedCard = screen.getByTestId("task-card-completed-task");
      expect(completedCard.getAttribute("style")).toContain("background-color: var(--kanban-card-success-bg");
      expect(completedCard.getAttribute("style")).toContain("border-color: var(--kanban-card-success-border");
      expect(completedCard.getAttribute("style")).toContain("box-shadow: inset 0 0 0 1px var(--kanban-card-selected-border");
      expect(completedCard.getAttribute("style")).not.toContain("background-color: var(--kanban-card-selected-bg");

      const escalatedCard = screen.getByTestId("task-card-escalated-task");
      expect(escalatedCard.getAttribute("style")).toContain("background-color: var(--kanban-card-warning-bg");
      expect(escalatedCard.getAttribute("style")).toContain("border-color: var(--kanban-card-warning-border");
      expect(escalatedCard.getAttribute("style")).not.toContain("background-color: var(--kanban-card-selected-bg");
    });

    it("should not dim non-draggable terminal and escalated cards", () => {
      const completedTask = createMockTask({ id: "completed-task", internalStatus: "approved" });
      const escalatedTask = createMockTask({ id: "escalated-task", internalStatus: "escalated" });

      render(
        <>
          <TaskCard task={completedTask} />
          <TaskCard task={escalatedTask} />
        </>,
        { wrapper: DndWrapper }
      );

      expect(screen.getByTestId("task-card-completed-task")).toHaveClass("cursor-default");
      expect(screen.getByTestId("task-card-completed-task")).not.toHaveClass("opacity-70");
      expect(screen.getByTestId("task-card-escalated-task")).toHaveClass("cursor-default");
      expect(screen.getByTestId("task-card-escalated-task")).not.toHaveClass("opacity-70");
    });

    it("should truncate long titles", () => {
      const longTitle = "This is a very long task title that should be truncated";
      const task = createMockTask({ title: longTitle });
      render(<TaskCard task={task} />, { wrapper: DndWrapper });
      const titleElement = screen.getByTestId("task-title");
      expect(titleElement).toHaveClass("truncate");
    });
  });

  describe("status badges", () => {
    it("should render review status badge when reviewStatus is provided", () => {
      const task = createMockTask();
      render(<TaskCard task={task} reviewStatus="ai_approved" />, {
        wrapper: DndWrapper,
      });
      expect(screen.getByText("AI Approved")).toBeInTheDocument();
    });

    it("should not render review badge when reviewStatus is not provided", () => {
      const task = createMockTask();
      render(<TaskCard task={task} />, { wrapper: DndWrapper });
      expect(screen.queryByText("AI Approved")).not.toBeInTheDocument();
    });

    it("should render checkpoint indicator when hasCheckpoint is true", () => {
      const task = createMockTask();
      render(<TaskCard task={task} hasCheckpoint />, { wrapper: DndWrapper });
      expect(screen.getByTestId("checkpoint-indicator")).toBeInTheDocument();
    });
  });

  describe("QA badge", () => {
    it("should render QA badge when needsQA is true", () => {
      const task = createMockTask();
      render(<TaskCard task={task} needsQA />, { wrapper: DndWrapper });
      expect(screen.getByTestId("task-qa-badge")).toBeInTheDocument();
    });

    it("should not render QA badge when needsQA is false", () => {
      const task = createMockTask();
      render(<TaskCard task={task} needsQA={false} />, { wrapper: DndWrapper });
      expect(screen.queryByTestId("task-qa-badge")).not.toBeInTheDocument();
    });

    it("should not render QA badge when needsQA is not provided", () => {
      const task = createMockTask();
      render(<TaskCard task={task} />, { wrapper: DndWrapper });
      expect(screen.queryByTestId("task-qa-badge")).not.toBeInTheDocument();
    });

    it("should show pending status when no prep or test status", () => {
      const task = createMockTask();
      render(<TaskCard task={task} needsQA />, { wrapper: DndWrapper });
      expect(screen.getByText("QA Pending")).toBeInTheDocument();
    });

    it("should show preparing status when prep is running", () => {
      const task = createMockTask();
      const prepStatus: QAPrepStatus = "running";
      render(<TaskCard task={task} needsQA prepStatus={prepStatus} />, {
        wrapper: DndWrapper,
      });
      expect(screen.getByText("Preparing")).toBeInTheDocument();
    });

    it("should show ready status when prep is completed", () => {
      const task = createMockTask();
      const prepStatus: QAPrepStatus = "completed";
      render(<TaskCard task={task} needsQA prepStatus={prepStatus} />, {
        wrapper: DndWrapper,
      });
      expect(screen.getByText("QA Ready")).toBeInTheDocument();
    });

    it("should show testing status when test is running", () => {
      const task = createMockTask();
      const testStatus: QAOverallStatus = "running";
      render(<TaskCard task={task} needsQA testStatus={testStatus} />, {
        wrapper: DndWrapper,
      });
      expect(screen.getByText("Testing")).toBeInTheDocument();
    });

    it("should show passed status when test is passed", () => {
      const task = createMockTask();
      const testStatus: QAOverallStatus = "passed";
      render(<TaskCard task={task} needsQA testStatus={testStatus} />, {
        wrapper: DndWrapper,
      });
      expect(screen.getByText("Passed")).toBeInTheDocument();
    });

    it("should show failed status when test is failed", () => {
      const task = createMockTask();
      const testStatus: QAOverallStatus = "failed";
      render(<TaskCard task={task} needsQA testStatus={testStatus} />, {
        wrapper: DndWrapper,
      });
      expect(screen.getByText("Failed")).toBeInTheDocument();
    });

    it("should prioritize test status over prep status", () => {
      const task = createMockTask();
      const prepStatus: QAPrepStatus = "running";
      const testStatus: QAOverallStatus = "passed";
      render(
        <TaskCard task={task} needsQA prepStatus={prepStatus} testStatus={testStatus} />,
        { wrapper: DndWrapper }
      );
      // Test status should take precedence
      expect(screen.getByText("Passed")).toBeInTheDocument();
      expect(screen.queryByText("Preparing")).not.toBeInTheDocument();
    });
  });

  describe("click handler", () => {
    it("should not crash when card is clicked", () => {
      const task = createMockTask();
      render(<TaskCard task={task} />, { wrapper: DndWrapper });

      expect(() =>
        fireEvent.click(screen.getByTestId(`task-card-${task.id}`))
      ).not.toThrow();
    });
  });

  describe("dragging state", () => {
    it("should apply opacity 1 when isDragging is true (card is visible in overlay)", () => {
      const task = createMockTask();
      render(<TaskCard task={task} isDragging />, { wrapper: DndWrapper });

      const card = screen.getByTestId(`task-card-${task.id}`);
      // isDragging prop doesn't directly control opacity - that's handled by isBeingDragged from useDraggable
      // When isDragging prop is true (used in DragOverlay), card should be visible with full opacity
      expect(card.style.opacity).toBe("1");
    });

    it("should have opacity 1 when not dragging", () => {
      const task = createMockTask();
      render(<TaskCard task={task} isDragging={false} />, {
        wrapper: DndWrapper,
      });

      const card = screen.getByTestId(`task-card-${task.id}`);
      expect(card.style.opacity).toBe("1");
    });
  });

  describe("drag handle", () => {
    it("should render drag handle", () => {
      const task = createMockTask();
      render(<TaskCard task={task} />, { wrapper: DndWrapper });
      expect(screen.getByTestId("drag-handle")).toBeInTheDocument();
    });

    it("should have proper cursor style on drag handle", () => {
      const task = createMockTask();
      render(<TaskCard task={task} />, { wrapper: DndWrapper });
      const handle = screen.getByTestId("drag-handle");
      expect(handle).toHaveClass("cursor-grab");
    });
  });
});
