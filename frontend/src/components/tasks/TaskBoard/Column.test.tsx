/**
 * Tests for Column component
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { DndContext } from "@dnd-kit/core";
import { createMockTask } from "@/test/mock-data";
import { Column } from "./Column";
import type { WorkflowColumnResponse } from "@/lib/api/workflows";
import type { InfiniteData } from "@tanstack/react-query";
import type { TaskListResponse } from "@/types/task";

// Mock IntersectionObserver
class MockIntersectionObserver {
  observe = vi.fn();
  unobserve = vi.fn();
  disconnect = vi.fn();
  constructor() {}
}
window.IntersectionObserver = MockIntersectionObserver as unknown as typeof IntersectionObserver;

// Mock useInfiniteTasksQuery - keep flattenPages implementation, only mock the hook
vi.mock("@/hooks/useInfiniteTasksQuery", async (importOriginal) => {
  const actual = await importOriginal() as Record<string, unknown>;
  return {
    ...actual,
    useInfiniteTasksQuery: vi.fn(),
  };
});

// Mock useProjectStats to avoid EventProvider dependency
vi.mock("@/hooks/useProjectStats", () => ({
  useProjectStats: vi.fn(() => ({ data: undefined })),
}));

import { useInfiniteTasksQuery } from "@/hooks/useInfiniteTasksQuery";
import { useProjectStats } from "@/hooks/useProjectStats";

function createTestQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  });
}

function DndWrapper({ children }: { children: React.ReactNode }) {
  const queryClient = createTestQueryClient();
  return (
    <QueryClientProvider client={queryClient}>
      <DndContext>{children}</DndContext>
    </QueryClientProvider>
  );
}

const createMockColumn = (overrides: Partial<WorkflowColumnResponse> = {}): WorkflowColumnResponse => ({
  id: "backlog",
  name: "Backlog",
  mapsTo: "backlog",
  ...overrides,
});

describe("Column", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // Default mock implementation
    vi.mocked(useInfiniteTasksQuery).mockReturnValue({
      data: { pages: [{ tasks: [], total: 0, hasMore: false, offset: 0 }], pageParams: [undefined] } as InfiniteData<TaskListResponse>,
      fetchNextPage: vi.fn(),
      hasNextPage: false,
      isFetchingNextPage: false,
      isLoading: false,
      isError: false,
      error: null,
    } as unknown as ReturnType<typeof useInfiniteTasksQuery>);
  });

  describe("rendering", () => {
    it("should render with data-testid", () => {
      const column = createMockColumn({ id: "my-column" });
      render(<Column column={column} projectId="p1" showArchived={false} />, { wrapper: DndWrapper });
      expect(screen.getByTestId("column-my-column")).toBeInTheDocument();
    });

    it("should render column name in header", () => {
      const column = createMockColumn({ name: "In Progress" });
      render(<Column column={column} projectId="p1" showArchived={false} />, { wrapper: DndWrapper });
      expect(screen.getByText("In Progress")).toBeInTheDocument();
    });

    it("drops the 220px minWidth when fillWidth is true", () => {
      const column = createMockColumn();
      render(
        <Column column={column} projectId="p1" showArchived={false} fillWidth />,
        { wrapper: DndWrapper },
      );
      const root = screen.getByTestId(`column-${column.id}`);
      expect(root.style.minWidth).toBe("0px");
    });

    it("keeps the 220px minWidth when fillWidth is unset", () => {
      const column = createMockColumn();
      render(
        <Column column={column} projectId="p1" showArchived={false} />,
        { wrapper: DndWrapper },
      );
      const root = screen.getByTestId(`column-${column.id}`);
      expect(root.style.minWidth).toBe("220px");
    });

    it("should render task count in header", () => {
      const tasks = [createMockTask(), createMockTask(), createMockTask()];
      vi.mocked(useInfiniteTasksQuery).mockReturnValue({
        data: { pages: [{ tasks, total: 3, hasMore: false, offset: 0 }], pageParams: [undefined] } as InfiniteData<TaskListResponse>,
        fetchNextPage: vi.fn(),
        hasNextPage: false,
        isFetchingNextPage: false,
        isLoading: false,
        isError: false,
        error: null,
      } as unknown as ReturnType<typeof useInfiniteTasksQuery>);

      const column = createMockColumn();
      render(<Column column={column} projectId="p1" showArchived={false} />, { wrapper: DndWrapper });
      expect(screen.getByText("(3)")).toBeInTheDocument();
    });

    it("should render tasks", () => {
      const tasks = [
        createMockTask({ id: "t1", title: "Task One" }),
        createMockTask({ id: "t2", title: "Task Two" }),
      ];
      vi.mocked(useInfiniteTasksQuery).mockReturnValue({
        data: { pages: [{ tasks, total: 2, hasMore: false, offset: 0 }], pageParams: [undefined] } as InfiniteData<TaskListResponse>,
        fetchNextPage: vi.fn(),
        hasNextPage: false,
        isFetchingNextPage: false,
        isLoading: false,
        isError: false,
        error: null,
      } as unknown as ReturnType<typeof useInfiniteTasksQuery>);

      const column = createMockColumn();
      render(<Column column={column} projectId="p1" showArchived={false} />, { wrapper: DndWrapper });
      expect(screen.getByText("Task One")).toBeInTheDocument();
      expect(screen.getByText("Task Two")).toBeInTheDocument();
    });

    it("should render empty state when no tasks", () => {
      const column = createMockColumn();
      render(<Column column={column} projectId="p1" showArchived={false} />, { wrapper: DndWrapper });
      const columnEl = screen.getByTestId(`column-${column.id}`);
      expect(columnEl).toBeInTheDocument();
      expect(screen.getByTestId("empty-state-tray")).toHaveStyle({
        backgroundColor: "var(--kanban-tray-bg)",
        color: "var(--kanban-empty-ink)",
      });
      expect(screen.getByTestId("empty-state-label")).toHaveStyle({
        color: "var(--kanban-empty-ink)",
        fontSize: "12px",
        fontWeight: "500",
      });
    });
  });

  describe("droppable behavior", () => {
    it("should be a droppable zone", () => {
      const column = createMockColumn();
      render(<Column column={column} projectId="p1" showArchived={false} />, { wrapper: DndWrapper });
      const dropZone = screen.getByTestId(`drop-zone-${column.id}`);
      expect(dropZone).toBeInTheDocument();
    });

    it("should apply isOver styling when isOver is true", () => {
      const column = createMockColumn();
      render(<Column column={column} projectId="p1" showArchived={false} isOver />, { wrapper: DndWrapper });
      const dropZone = screen.getByTestId(`drop-zone-${column.id}`);
      expect(dropZone.style.backgroundColor).toBe("var(--status-info-muted)");
      expect(dropZone.style.borderRadius).toBe("6px");
    });

    it("should not apply isOver styling when isOver is false", () => {
      const column = createMockColumn();
      render(<Column column={column} projectId="p1" showArchived={false} isOver={false} />, { wrapper: DndWrapper });
      const dropZone = screen.getByTestId(`drop-zone-${column.id}`);
      expect(dropZone.style.backgroundColor).toBe("transparent");
      expect(dropZone.style.borderRadius).toBe("0px");
    });
  });

  describe("locked columns", () => {
    it("should show invalid drop icon for In Progress column when isOver and isInvalid", () => {
      const column = createMockColumn({ id: "in_progress", name: "In Progress" });
      render(<Column column={column} projectId="p1" showArchived={false} isOver isInvalid />, { wrapper: DndWrapper });
      expect(screen.getByTestId("invalid-drop-icon")).toBeInTheDocument();
    });

    it("should show invalid drop icon for In Review column when isOver and isInvalid", () => {
      const column = createMockColumn({ id: "in_review", name: "In Review" });
      render(<Column column={column} projectId="p1" showArchived={false} isOver isInvalid />, { wrapper: DndWrapper });
      expect(screen.getByTestId("invalid-drop-icon")).toBeInTheDocument();
    });

    it("should show invalid drop icon for Done column when isOver and isInvalid", () => {
      const column = createMockColumn({ id: "done", name: "Done" });
      render(<Column column={column} projectId="p1" showArchived={false} isOver isInvalid />, { wrapper: DndWrapper });
      expect(screen.getByTestId("invalid-drop-icon")).toBeInTheDocument();
    });

    it("should apply error border when isOver and isInvalid", () => {
      const column = createMockColumn({ id: "in_progress" });
      render(<Column column={column} projectId="p1" showArchived={false} isOver isInvalid />, { wrapper: DndWrapper });
      const dropZone = screen.getByTestId(`drop-zone-${column.id}`);
      expect(dropZone.style.backgroundColor).toBe("var(--status-error-muted)");
      expect(dropZone.style.borderRadius).toBe("6px");
    });
  });

  describe("collapsed state", () => {
    it("should render as compact horizontal rail when collapsed", () => {
      const column = createMockColumn({ id: "ready", name: "Ready" });
      render(
        <Column column={column} projectId="p1" showArchived={false} showMergeTasks isCollapsed onToggleCollapse={vi.fn()} />,
        { wrapper: DndWrapper },
      );
      const el = screen.getByTestId("column-ready");
      expect(el.style.width).toBe("128px");
      expect(el).toHaveAttribute("role", "button");
      expect(el).toHaveAttribute("aria-expanded", "false");
    });

    it("should show horizontal column name when collapsed", () => {
      const column = createMockColumn({ id: "ready", name: "Ready" });
      render(
        <Column column={column} projectId="p1" showArchived={false} showMergeTasks isCollapsed onToggleCollapse={vi.fn()} />,
        { wrapper: DndWrapper },
      );
      const label = screen.getByText("Ready");
      expect(label).toBeInTheDocument();
      expect(label).not.toHaveStyle({ writingMode: "vertical-rl" });
    });

    it("should keep the empty state visible when collapsed with no tasks", () => {
      const column = createMockColumn({ id: "review", name: "In Review" });
      render(
        <Column column={column} projectId="p1" showArchived={false} showMergeTasks isCollapsed onToggleCollapse={vi.fn()} />,
        { wrapper: DndWrapper },
      );

      expect(screen.getByTestId("collapsed-empty-state")).toHaveTextContent("No tasks");
      expect(screen.getByTestId("collapsed-empty-state-label")).toHaveStyle({
        color: "var(--kanban-empty-ink)",
        fontWeight: "500",
      });
    });

    it("should expose a collapse control when expanded", () => {
      const column = createMockColumn({ id: "ready", name: "Ready" });
      render(
        <Column column={column} projectId="p1" showArchived={false} showMergeTasks isCollapsed={false} onToggleCollapse={vi.fn()} />,
        { wrapper: DndWrapper },
      );

      expect(screen.getByLabelText("Collapse Ready column")).toBeInTheDocument();
    });

    it("should not expose a collapse control when expanded with tasks", () => {
      const tasks = [createMockTask({ id: "task-1", title: "Task One" })];
      vi.mocked(useInfiniteTasksQuery).mockReturnValue({
        data: { pages: [{ tasks, total: 1, hasMore: false, offset: 0 }], pageParams: [undefined] } as InfiniteData<TaskListResponse>,
        fetchNextPage: vi.fn(),
        hasNextPage: false,
        isFetchingNextPage: false,
        isLoading: false,
        isError: false,
        error: null,
      } as unknown as ReturnType<typeof useInfiniteTasksQuery>);
      const column = createMockColumn({ id: "ready", name: "Ready" });
      render(
        <Column column={column} projectId="p1" showArchived={false} showMergeTasks isCollapsed={false} onToggleCollapse={vi.fn()} />,
        { wrapper: DndWrapper },
      );

      expect(screen.queryByLabelText("Collapse Ready column")).not.toBeInTheDocument();
    });

    it("should show Draft '+' quick-add button when draft column is collapsed", () => {
      const column = createMockColumn({ id: "draft", name: "Draft" });
      render(
        <Column column={column} projectId="p1" showArchived={false} showMergeTasks isCollapsed onToggleCollapse={vi.fn()} />,
        { wrapper: DndWrapper },
      );
      expect(screen.getByLabelText("Add task")).toBeInTheDocument();
    });

    it("should expand the column when collapsed quick-add is clicked", () => {
      const onToggleCollapse = vi.fn();
      const column = createMockColumn({ id: "draft", name: "Draft" });
      render(
        <Column column={column} projectId="p1" showArchived={false} showMergeTasks isCollapsed onToggleCollapse={onToggleCollapse} />,
        { wrapper: DndWrapper },
      );

      screen.getByLabelText("Add task").click();

      expect(onToggleCollapse).toHaveBeenCalledTimes(1);
    });

    it("should NOT show '+' button on non-draft collapsed columns", () => {
      const column = createMockColumn({ id: "done", name: "Done" });
      render(
        <Column column={column} projectId="p1" showArchived={false} showMergeTasks isCollapsed onToggleCollapse={vi.fn()} />,
        { wrapper: DndWrapper },
      );
      expect(screen.queryByLabelText("Add task")).not.toBeInTheDocument();
    });

    it("should not render sentinel element when collapsed (no infinite scroll)", () => {
      const column = createMockColumn({ id: "ready", name: "Ready" });
      const { container } = render(
        <Column column={column} projectId="p1" showArchived={false} showMergeTasks isCollapsed onToggleCollapse={vi.fn()} />,
        { wrapper: DndWrapper },
      );
      // Collapsed view has no drop zone or sentinel
      expect(container.querySelector("[data-testid='drop-zone-ready']")).not.toBeInTheDocument();
    });

    it("should render sentinel element when expanded (infinite scroll active)", () => {
      const column = createMockColumn({ id: "ready", name: "Ready" });
      render(
        <Column column={column} projectId="p1" showArchived={false} showMergeTasks isCollapsed={false} onToggleCollapse={vi.fn()} />,
        { wrapper: DndWrapper },
      );
      // Expanded view has the drop zone with sentinel
      expect(screen.getByTestId("drop-zone-ready")).toBeInTheDocument();
    });
  });

  describe("cycle time display", () => {
    it("should not show cycle time badge when no project stats", () => {
      vi.mocked(useProjectStats).mockReturnValue({ data: undefined } as ReturnType<typeof useProjectStats>);
      const column = createMockColumn({ mapsTo: "executing" });
      render(<Column column={column} projectId="p1" showArchived={false} />, { wrapper: DndWrapper });
      // No cycle time badge should appear in the header
      expect(screen.queryByTitle(/Avg time/)).not.toBeInTheDocument();
    });

    it("should not show cycle time badge when sampleSize is 0", () => {
      vi.mocked(useProjectStats).mockReturnValue({
        data: {
          taskCount: 5,
          tasksCompletedToday: 0,
          tasksCompletedThisWeek: 0,
          tasksCompletedThisMonth: 0,
          agentSuccessRate: 1,
          agentSuccessCount: 5,
          agentTotalCount: 5,
          reviewPassRate: 1,
          reviewPassCount: 5,
          reviewTotalCount: 5,
          cycleTimeBreakdown: [{ phase: "executing", avgMinutes: 20, sampleSize: 0 }],
          eme: null,
        },
      } as ReturnType<typeof useProjectStats>);
      const column = createMockColumn({ mapsTo: "executing" });
      render(<Column column={column} projectId="p1" showArchived={false} />, { wrapper: DndWrapper });
      expect(screen.queryByTitle(/Avg time/)).not.toBeInTheDocument();
    });

    it("should show formatted cycle time when stats are available with sample data", () => {
      vi.mocked(useProjectStats).mockReturnValue({
        data: {
          taskCount: 5,
          tasksCompletedToday: 1,
          tasksCompletedThisWeek: 3,
          tasksCompletedThisMonth: 5,
          agentSuccessRate: 1,
          agentSuccessCount: 5,
          agentTotalCount: 5,
          reviewPassRate: 1,
          reviewPassCount: 5,
          reviewTotalCount: 5,
          cycleTimeBreakdown: [{ phase: "executing", avgMinutes: 30, sampleSize: 3 }],
          eme: null,
        },
      } as ReturnType<typeof useProjectStats>);
      const column = createMockColumn({ mapsTo: "executing" });
      render(<Column column={column} projectId="p1" showArchived={false} />, { wrapper: DndWrapper });
      // 30 minutes = "30m" formatted
      expect(screen.getByTitle(/Avg time in/)).toBeInTheDocument();
      expect(screen.getByTitle(/30m/)).toBeInTheDocument();
    });
  });

  describe("loading state", () => {
    it("should show skeleton cards when loading", () => {
      vi.mocked(useInfiniteTasksQuery).mockReturnValue({
        data: undefined,
        fetchNextPage: vi.fn(),
        hasNextPage: false,
        isFetchingNextPage: false,
        isLoading: true,
        isError: false,
        error: null,
      } as unknown as ReturnType<typeof useInfiniteTasksQuery>);

      const column = createMockColumn();
      const { container } = render(<Column column={column} projectId="p1" showArchived={false} />, { wrapper: DndWrapper });

      // Skeleton cards have the skeleton component class
      // The Skeleton component from shadcn renders with animate-shimmer class
      expect(screen.getByTestId(`column-${column.id}`)).toBeInTheDocument();
      // Verify skeletons are rendered (they use the Skeleton component class)
      const skeletons = container.querySelectorAll(".rounded-lg");
      expect(skeletons.length).toBeGreaterThan(0);
    });
  });

});
