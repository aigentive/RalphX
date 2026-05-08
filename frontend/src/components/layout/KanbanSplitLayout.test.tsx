import { describe, expect, it, beforeEach, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { useUiStore } from "@/stores/uiStore";
import { KanbanSplitLayout } from "./KanbanSplitLayout";

const { mockUseTaskChatAvailability } = vi.hoisted(() => ({
  mockUseTaskChatAvailability: vi.fn(() => false),
}));

vi.mock("@/components/Chat/IntegratedChatPanel", () => ({
  IntegratedChatPanel: ({ projectId }: { projectId: string }) => (
    <div data-testid="integrated-chat-panel">Task chat for {projectId}</div>
  ),
}));

vi.mock("@/components/tasks/TaskDetailOverlay", () => ({
  TaskDetailOverlay: ({
    projectId,
    constrainContent,
  }: {
    projectId: string;
    constrainContent?: boolean;
  }) => (
    <div data-testid="task-detail-overlay" data-constrained={String(Boolean(constrainContent))}>
      Task detail for {projectId}
    </div>
  ),
}));

vi.mock("@/components/tasks/TaskCreationOverlay", () => ({
  TaskCreationOverlay: () => <div data-testid="task-creation-overlay" />,
}));

vi.mock("@/hooks/useTaskChatAvailability", () => ({
  useTaskChatAvailability: mockUseTaskChatAvailability,
}));

describe("KanbanSplitLayout", () => {
  beforeEach(() => {
    mockUseTaskChatAvailability.mockReturnValue(false);
    useUiStore.setState({
      selectedTaskId: null,
      taskCreationContext: null,
      taskHistoryState: null,
    });
    localStorage.clear();
  });

  it("does not render a project chat pane on the main Kanban board", () => {
    render(
      <KanbanSplitLayout projectId="project-1">
        <div data-testid="kanban-board">Board</div>
      </KanbanSplitLayout>
    );

    expect(screen.getByTestId("kanban-board")).toBeInTheDocument();
    expect(screen.getByTestId("kanban-split-layout").getAttribute("style")).toContain(
      "background-color: var(--app-content-bg)"
    );
    expect(screen.queryByTestId("kanban-task-chat-panel")).not.toBeInTheDocument();
    expect(screen.queryByTestId("integrated-chat-panel")).not.toBeInTheDocument();
  });

  it("keeps task detail open without rendering an empty chat pane when chat is unavailable", () => {
    useUiStore.setState({ selectedTaskId: "task-1" });

    render(
      <KanbanSplitLayout projectId="project-1">
        <div data-testid="kanban-board">Board</div>
      </KanbanSplitLayout>
    );

    expect(screen.getByTestId("task-detail-overlay")).toBeInTheDocument();
    expect(screen.getByTestId("task-detail-overlay")).toHaveAttribute("data-constrained", "true");
    expect(screen.queryByTestId("kanban-task-chat-panel")).not.toBeInTheDocument();
    expect(screen.queryByTestId("integrated-chat-panel")).not.toBeInTheDocument();
  });

  it("renders selected-task chat when an agent chat is available", () => {
    mockUseTaskChatAvailability.mockReturnValue(true);
    useUiStore.setState({ selectedTaskId: "task-1" });

    render(
      <KanbanSplitLayout projectId="project-1">
        <div data-testid="kanban-board">Board</div>
      </KanbanSplitLayout>
    );

    expect(screen.getByTestId("task-detail-overlay")).toBeInTheDocument();
    expect(screen.getByTestId("task-detail-overlay")).toHaveAttribute("data-constrained", "false");
    expect(screen.getByTestId("kanban-task-chat-panel")).toBeInTheDocument();
    expect(screen.getByTestId("kanban-task-chat-panel").getAttribute("style")).toContain(
      "border-color: var(--app-content-border)"
    );
    expect(screen.getByTestId("integrated-chat-panel")).toHaveTextContent("Task chat for project-1");
  });
});
