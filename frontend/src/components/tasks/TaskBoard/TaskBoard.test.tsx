/**
 * Tests for TaskBoard component
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, fireEvent, act, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { api } from "@/lib/tauri";
import { createMockTask } from "@/test/mock-data";
import { useUiStore } from "@/stores/uiStore";
import { TaskBoard } from "./TaskBoard";
import type { TaskListResponse } from "@/types/task";
import type { InfiniteData } from "@tanstack/react-query";
import type { WorkflowColumnResponse } from "@/lib/api/workflows";

// Mock IntersectionObserver
class MockIntersectionObserver {
  observe = vi.fn();
  unobserve = vi.fn();
  disconnect = vi.fn();
  constructor() {}
}
window.IntersectionObserver = MockIntersectionObserver as unknown as typeof IntersectionObserver;

// Mock Tauri API
vi.mock("@/lib/tauri", () => ({
  api: {
    tasks: {
      list: vi.fn(),
      move: vi.fn(),
      getArchivedCount: vi.fn(),
      search: vi.fn(),
    },
  },
}));

// Mock planStore
vi.mock("@/stores/planStore", () => ({
  usePlanStore: (() => {
    const state = {
      activePlanByProject: { "p1": "session-1" },
      activePlanLoadedByProject: { "p1": true },
      activeExecutionPlanIdByProject: {},
      planCandidates: [],
      isLoading: false,
      error: null,
      loadActivePlan: vi.fn(),
      setActivePlan: vi.fn(),
      clearActivePlan: vi.fn(),
      loadCandidates: vi.fn(),
    };
    const usePlanStore = vi.fn((selector) =>
      selector ? selector(state) : state
    );
    (usePlanStore as unknown as { getState: () => typeof state }).getState = () =>
      state;
    return usePlanStore;
  })(),
  selectActiveExecutionPlanId:
    (projectId: string) =>
    (state: { activeExecutionPlanIdByProject: Record<string, string | null> }): string | null =>
      state.activeExecutionPlanIdByProject[projectId] ?? null,
}));

// Mock workflows API
vi.mock("@/lib/api/workflows", () => ({
  getActiveWorkflowColumns: vi.fn(),
}));

// Mock useInfiniteTasksQuery - keep flattenPages implementation, only mock the hook
vi.mock("@/hooks/useInfiniteTasksQuery", async (importOriginal) => {
  const actual = await importOriginal() as Record<string, unknown>;
  return {
    ...actual,
    useInfiniteTasksQuery: vi.fn(),
  };
});

// Mock Tauri events
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
  emit: vi.fn(),
}));

vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => ({
    subscribe: vi.fn(() => vi.fn()),
  }),
}));

// Mock useColumnCollapse to keep all columns expanded
vi.mock("./useColumnCollapse", () => ({
  useColumnCollapse: () => ({
    isCollapsed: () => false,
    toggleCollapse: vi.fn(),
    expandColumn: vi.fn(),
  }),
}));

import { getActiveWorkflowColumns } from "@/lib/api/workflows";
import { useInfiniteTasksQuery } from "@/hooks/useInfiniteTasksQuery";

// Helper to create mock columns
function createMockColumns(): WorkflowColumnResponse[] {
  return [
    { id: "draft", name: "Draft", mapsTo: "backlog" },
    { id: "ready", name: "Ready", mapsTo: "ready" },
    { id: "in_progress", name: "In Progress", mapsTo: "executing" },
    { id: "in_review", name: "In Review", mapsTo: "pending_review" },
    { id: "done", name: "Done", mapsTo: "approved" },
  ];
}

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
}

describe("TaskBoard", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
    useUiStore.setState({
      boardSearchQuery: null,
      kanbanCardDisplayMode: "default",
    });
    // Default mock for archived count
    vi.mocked(api.tasks.getArchivedCount).mockResolvedValue(0);
    // Default mock for search
    vi.mocked(api.tasks.search).mockResolvedValue([]);
    // Default mock for infinite query
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

  describe("loading state", () => {
    it("should show skeleton while loading", async () => {
      vi.mocked(getActiveWorkflowColumns).mockImplementation(() => new Promise(() => {}));

      render(<TaskBoard projectId="p1" />, { wrapper: createWrapper() });
      expect(screen.getByTestId("task-board-skeleton")).toBeInTheDocument();
    });

    it("should hide skeleton when data is loaded", async () => {
      vi.mocked(getActiveWorkflowColumns).mockResolvedValue(createMockColumns());

      render(<TaskBoard projectId="p1" />, { wrapper: createWrapper() });

      await waitFor(() => {
        expect(screen.queryByTestId("task-board-skeleton")).not.toBeInTheDocument();
      });
    });
  });

  describe("rendering columns", () => {
    it("should render with data-testid", async () => {
      vi.mocked(getActiveWorkflowColumns).mockResolvedValue(createMockColumns());

      render(<TaskBoard projectId="p1" />, { wrapper: createWrapper() });

      await waitFor(() => {
        expect(screen.getByTestId("task-board")).toBeInTheDocument();
      });
    });

    it("should render 5 columns from default workflow", async () => {
      vi.mocked(getActiveWorkflowColumns).mockResolvedValue(createMockColumns());

      render(<TaskBoard projectId="p1" />, { wrapper: createWrapper() });

      await waitFor(() => {
        expect(screen.getByTestId("column-draft")).toBeInTheDocument();
        expect(screen.getByTestId("column-ready")).toBeInTheDocument();
        expect(screen.getByTestId("column-in_progress")).toBeInTheDocument();
        expect(screen.getByTestId("column-in_review")).toBeInTheDocument();
        expect(screen.getByTestId("column-done")).toBeInTheDocument();
      });
    });

    it("should render tasks in their columns", async () => {
      const tasks = [
        createMockTask({ id: "t1", title: "Task One", internalStatus: "backlog" }),
        createMockTask({ id: "t2", title: "Task Two", internalStatus: "ready" }),
      ];
      vi.mocked(getActiveWorkflowColumns).mockResolvedValue(createMockColumns());

      // Mock the infinite query to return tasks based on status
      vi.mocked(useInfiniteTasksQuery).mockImplementation((params) => {
        const tasksForStatus = tasks.filter(
          (t) => params.statuses?.includes(t.internalStatus) ?? false
        );
        return {
          data: { pages: [{ tasks: tasksForStatus, total: tasksForStatus.length, hasMore: false, offset: 0 }], pageParams: [undefined] } as InfiniteData<TaskListResponse>,
          fetchNextPage: vi.fn(),
          hasNextPage: false,
          isFetchingNextPage: false,
          isLoading: false,
          isError: false,
          error: null,
        } as unknown as ReturnType<typeof useInfiniteTasksQuery>;
      });

      render(<TaskBoard projectId="p1" />, { wrapper: createWrapper() });

      await waitFor(() => {
        expect(screen.getByText("Task One")).toBeInTheDocument();
        expect(screen.getByText("Task Two")).toBeInTheDocument();
      });
    });
  });

  describe("horizontal scrolling", () => {
    it("should have horizontal scroll container", async () => {
      vi.mocked(getActiveWorkflowColumns).mockResolvedValue(createMockColumns());

      render(<TaskBoard projectId="p1" />, { wrapper: createWrapper() });

      await waitFor(() => {
        const board = screen.getByTestId("task-board");
        expect(board).toHaveClass("overflow-x-auto");
      });
    });

    it("keeps the kanban toolbar divider on the v29a line token", async () => {
      vi.mocked(getActiveWorkflowColumns).mockResolvedValue(createMockColumns());

      render(<TaskBoard projectId="p1" />, { wrapper: createWrapper() });

      await waitFor(() => {
        expect(screen.getByTestId("kanban-toolbar")).toHaveStyle({
          borderBottomColor: "var(--kanban-toolbar-border, #2E2E36)",
          borderBottomStyle: "solid",
          borderBottomWidth: "1px",
        });
      });
    });
  });

  describe("error handling", () => {
    it("should show error message when fetch fails", async () => {
      vi.mocked(getActiveWorkflowColumns).mockRejectedValue(new Error("Failed to fetch"));

      render(<TaskBoard projectId="p1" />, { wrapper: createWrapper() });

      await waitFor(() => {
        expect(screen.getByTestId("task-board-error")).toBeInTheDocument();
      });
    });
  });

  describe("toolbar interactions", () => {
    it("renders accessible card density controls and toggles mini mode", async () => {
      vi.mocked(getActiveWorkflowColumns).mockResolvedValue(createMockColumns());

      render(<TaskBoard projectId="p1" />, { wrapper: createWrapper() });

      const densityGroup = await screen.findByRole("group", {
        name: /kanban card layout/i,
      });
      const defaultButton = within(densityGroup).getByRole("button", {
        name: /default cards/i,
      });
      const miniButton = within(densityGroup).getByRole("button", {
        name: /mini cards/i,
      });

      expect(defaultButton).toHaveAttribute("aria-pressed", "true");
      expect(miniButton).toHaveAttribute("aria-pressed", "false");

      fireEvent.click(miniButton);

      expect(defaultButton).toHaveAttribute("aria-pressed", "false");
      expect(miniButton).toHaveAttribute("aria-pressed", "true");
    });

    it("keeps mini card density after leaving and returning across projects", async () => {
      vi.mocked(getActiveWorkflowColumns).mockResolvedValue(createMockColumns());

      const firstRender = render(<TaskBoard projectId="p1" />, { wrapper: createWrapper() });

      const firstDensityGroup = await screen.findByRole("group", {
        name: /kanban card layout/i,
      });
      const firstMiniButton = within(firstDensityGroup).getByRole("button", {
        name: /mini cards/i,
      });

      fireEvent.click(firstMiniButton);
      expect(localStorage.getItem("ralphx-kanban-card-display-mode")).toBe("mini");

      firstRender.unmount();

      render(<TaskBoard projectId="p2" />, { wrapper: createWrapper() });

      const secondDensityGroup = await screen.findByRole("group", {
        name: /kanban card layout/i,
      });
      expect(
        within(secondDensityGroup).getByRole("button", { name: /mini cards/i })
      ).toHaveAttribute("aria-pressed", "true");
      expect(
        within(secondDensityGroup).getByRole("button", { name: /default cards/i })
      ).toHaveAttribute("aria-pressed", "false");
    });

    it("clears boardSearchQuery when search bar close button is clicked", async () => {
      vi.mocked(getActiveWorkflowColumns).mockResolvedValue(createMockColumns());

      // Seed the uiStore with a non-null search query so the close handler is meaningful
      act(() => {
        useUiStore.getState().setBoardSearchQuery("hello");
      });
      expect(useUiStore.getState().boardSearchQuery).toBe("hello");

      render(<TaskBoard projectId="p1" />, { wrapper: createWrapper() });

      const closeBtn = await screen.findByRole("button", { name: /close search/i });
      fireEvent.click(closeBtn);

      await waitFor(() => {
        expect(useUiStore.getState().boardSearchQuery).toBeNull();
      });
    });

    it("invokes onOpenPlanQuickSwitcher when PlanSelectorInline trigger fires", async () => {
      vi.mocked(getActiveWorkflowColumns).mockResolvedValue(createMockColumns());
      const onOpenPlanQuickSwitcher = vi.fn();

      render(
        <TaskBoard projectId="p1" onOpenPlanQuickSwitcher={onOpenPlanQuickSwitcher} />,
        { wrapper: createWrapper() },
      );

      const trigger = await screen.findByTestId("plan-selector-inline-trigger");
      fireEvent.click(trigger);

      expect(onOpenPlanQuickSwitcher).toHaveBeenCalledWith("kanban_inline");
    });
  });

  describe("fillWidth prop", () => {
    it("keeps fixed 300px columns when fillWidth=true so the host scrolls instead of stretching lanes", async () => {
      vi.mocked(getActiveWorkflowColumns).mockResolvedValue(createMockColumns());
      render(<TaskBoard projectId="p1" fillWidth />, { wrapper: createWrapper() });

      const board = await screen.findByTestId("task-board");
      const tpl = (board as HTMLElement).style.gridTemplateColumns;
      expect(tpl).toContain("300px");
      expect(tpl).not.toContain("1fr");
      expect(tpl).not.toContain("minmax(0, 1fr)");
    });

    it("uses fixed 300px expanded columns when fillWidth is unset", async () => {
      vi.mocked(getActiveWorkflowColumns).mockResolvedValue(createMockColumns());
      render(<TaskBoard projectId="p1" />, { wrapper: createWrapper() });

      const board = await screen.findByTestId("task-board");
      const tpl = (board as HTMLElement).style.gridTemplateColumns;
      expect(tpl).toContain("300px");
      expect(tpl).not.toContain("1fr");
    });
  });
});
