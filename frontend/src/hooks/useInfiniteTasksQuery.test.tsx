/**
 * Tests for useInfiniteTasksQuery hook
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { useInfiniteTasksQuery, flattenPages } from "./useInfiniteTasksQuery";
import { api } from "@/lib/tauri";
import { usePlanStore } from "@/stores/planStore";
import type { TaskListResponse } from "@/types/task";

// Mock the tauri API
vi.mock("@/lib/tauri", () => ({
  api: {
    tasks: {
      list: vi.fn(),
    },
  },
}));

describe("useInfiniteTasksQuery", () => {
  let queryClient: QueryClient;

  beforeEach(() => {
    queryClient = new QueryClient({
      defaultOptions: {
        queries: {
          retry: false,
        },
      },
    });
    vi.clearAllMocks();
  });

  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );

  const createMockTask = (id: string, projectId: string) => ({
    id,
    projectId,
    category: "feature",
    title: `Task ${id}`,
    description: null,
    priority: 0,
    internalStatus: "backlog" as const,
    needsReviewPoint: false,
    sourceProposalId: null,
    planArtifactId: null,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
    startedAt: null,
    completedAt: null,
    archivedAt: null,
  });

  it("should fetch first page of tasks", async () => {
    const mockResponse: TaskListResponse = {
      tasks: [
        createMockTask("task-1", "project-123"),
        createMockTask("task-2", "project-123"),
      ],
      total: 2,
      hasMore: false,
      offset: 0,
    };

    vi.mocked(api.tasks.list).mockResolvedValue(mockResponse);

    const { result } = renderHook(
      () =>
        useInfiniteTasksQuery({
          projectId: "project-123",
          statuses: ["backlog"],
        }),
      { wrapper }
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(api.tasks.list).toHaveBeenCalledWith({
      projectId: "project-123",
      statuses: ["backlog"],
      offset: 0,
      limit: 20,
      includeArchived: false,
    });

    expect(result.current.data?.pages).toHaveLength(1);
    expect(result.current.data?.pages[0].tasks).toHaveLength(2);
    expect(result.current.hasNextPage).toBe(false);
  });

  it("should fetch next page when hasMore is true", async () => {
    const firstPageResponse: TaskListResponse = {
      tasks: Array.from({ length: 20 }, (_, i) =>
        createMockTask(`task-${i}`, "project-123")
      ),
      total: 40,
      hasMore: true,
      offset: 0,
    };

    const secondPageResponse: TaskListResponse = {
      tasks: Array.from({ length: 20 }, (_, i) =>
        createMockTask(`task-${i + 20}`, "project-123")
      ),
      total: 40,
      hasMore: false,
      offset: 20,
    };

    vi.mocked(api.tasks.list)
      .mockResolvedValueOnce(firstPageResponse)
      .mockResolvedValueOnce(secondPageResponse);

    const { result } = renderHook(
      () =>
        useInfiniteTasksQuery({
          projectId: "project-123",
        }),
      { wrapper }
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(result.current.hasNextPage).toBe(true);

    // Fetch next page
    result.current.fetchNextPage();

    // Wait for the second page to be loaded
    await waitFor(() => expect(result.current.data?.pages).toHaveLength(2));

    expect(api.tasks.list).toHaveBeenCalledTimes(2);
    expect(api.tasks.list).toHaveBeenNthCalledWith(2, {
      projectId: "project-123",
      offset: 20,
      limit: 20,
      includeArchived: false,
    });

    expect(result.current.isFetchingNextPage).toBe(false);
    expect(result.current.hasNextPage).toBe(false);
  });

  it("should respect includeArchived parameter", async () => {
    const mockResponse: TaskListResponse = {
      tasks: [createMockTask("task-1", "project-123")],
      total: 1,
      hasMore: false,
      offset: 0,
    };

    vi.mocked(api.tasks.list).mockResolvedValue(mockResponse);

    renderHook(
      () =>
        useInfiniteTasksQuery({
          projectId: "project-123",
          includeArchived: true,
        }),
      { wrapper }
    );

    await waitFor(() =>
      expect(api.tasks.list).toHaveBeenCalledWith({
        projectId: "project-123",
        offset: 0,
        limit: 20,
        includeArchived: true,
      })
    );
  });

  it("should handle empty results", async () => {
    const mockResponse: TaskListResponse = {
      tasks: [],
      total: 0,
      hasMore: false,
      offset: 0,
    };

    vi.mocked(api.tasks.list).mockResolvedValue(mockResponse);

    const { result } = renderHook(
      () =>
        useInfiniteTasksQuery({
          projectId: "project-123",
        }),
      { wrapper }
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(result.current.data?.pages[0].tasks).toHaveLength(0);
    expect(result.current.hasNextPage).toBe(false);
  });

  it("should handle API errors", async () => {
    vi.mocked(api.tasks.list).mockRejectedValue(new Error("API Error"));

    const { result } = renderHook(
      () =>
        useInfiniteTasksQuery({
          projectId: "project-123",
        }),
      { wrapper }
    );

    await waitFor(() => expect(result.current.isError).toBe(true));

    expect(result.current.error?.message).toBe("API Error");
  });
});

describe("useInfiniteTasksQuery plan guard", () => {
  let queryClient: QueryClient;

  beforeEach(() => {
    queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    vi.clearAllMocks();
    // Reset plan store to pristine state before each test
    usePlanStore.setState({
      activePlanByProject: {},
      activeExecutionPlanIdByProject: {},
      activePlanLoadedByProject: {},
      planCandidates: [],
      isLoading: false,
      error: null,
    });
  });

  afterEach(() => {
    // Clean up store after each test to avoid polluting other suites
    usePlanStore.setState({
      activePlanByProject: {},
      activeExecutionPlanIdByProject: {},
      activePlanLoadedByProject: {},
      planCandidates: [],
      isLoading: false,
      error: null,
    });
  });

  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );

  const emptyResponse: TaskListResponse = {
    tasks: [],
    total: 0,
    hasMore: false,
    offset: 0,
  };

  it("should be disabled when activePlanId is set but executionPlanId is null (loading gap)", async () => {
    usePlanStore.setState({
      activePlanByProject: { "project-123": "session-abc" },
    });

    const { result } = renderHook(
      () =>
        useInfiniteTasksQuery({
          projectId: "project-123",
          executionPlanId: null,
        }),
      { wrapper }
    );

    // Allow time for any pending queries to fire
    await new Promise((resolve) => setTimeout(resolve, 50));

    expect(api.tasks.list).not.toHaveBeenCalled();
    expect(result.current.fetchStatus).toBe("idle");
  });

  it("should be enabled when activePlanId and executionPlanId are both set", async () => {
    vi.mocked(api.tasks.list).mockResolvedValue(emptyResponse);

    usePlanStore.setState({
      activePlanByProject: { "project-123": "session-abc" },
    });

    const { result } = renderHook(
      () =>
        useInfiniteTasksQuery({
          projectId: "project-123",
          executionPlanId: "exec-plan-xyz",
        }),
      { wrapper }
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(api.tasks.list).toHaveBeenCalledWith(
      expect.objectContaining({
        projectId: "project-123",
        executionPlanId: "exec-plan-xyz",
      })
    );
  });

  it("should be enabled when no active plan exists (activePlanId is null)", async () => {
    vi.mocked(api.tasks.list).mockResolvedValue(emptyResponse);
    // No active plan set in store

    const { result } = renderHook(
      () =>
        useInfiniteTasksQuery({
          projectId: "project-123",
        }),
      { wrapper }
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(api.tasks.list).toHaveBeenCalled();
  });

  it("should enable query after executionPlanId resolves from null", async () => {
    vi.mocked(api.tasks.list).mockResolvedValue(emptyResponse);

    usePlanStore.setState({
      activePlanByProject: { "project-123": "session-abc" },
    });

    const { result, rerender } = renderHook(
      ({ execId }: { execId: string | null }) =>
        useInfiniteTasksQuery({
          projectId: "project-123",
          executionPlanId: execId,
        }),
      { wrapper, initialProps: { execId: null } }
    );

    // Initially disabled (gap state)
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(api.tasks.list).not.toHaveBeenCalled();

    // executionPlanId resolves
    rerender({ execId: "exec-plan-xyz" });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(api.tasks.list).toHaveBeenCalledWith(
      expect.objectContaining({ executionPlanId: "exec-plan-xyz" })
    );
  });

  it("should be enabled when ideationSessionId is provided even without executionPlanId", async () => {
    vi.mocked(api.tasks.list).mockResolvedValue(emptyResponse);

    usePlanStore.setState({
      activePlanByProject: { "project-123": "session-abc" },
    });

    const { result } = renderHook(
      () =>
        useInfiniteTasksQuery({
          projectId: "project-123",
          ideationSessionId: "ideation-session-xyz",
          executionPlanId: null,
        }),
      { wrapper }
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(api.tasks.list).toHaveBeenCalledWith(
      expect.objectContaining({
        projectId: "project-123",
        ideationSessionId: "ideation-session-xyz",
      })
    );
  });
});

describe("flattenPages", () => {
  const createMockTask = (id: string) => ({
    id,
    projectId: "project-123",
    category: "feature",
    title: `Task ${id}`,
    description: null,
    priority: 0,
    internalStatus: "backlog" as const,
    needsReviewPoint: false,
    sourceProposalId: null,
    planArtifactId: null,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
    startedAt: null,
    completedAt: null,
    archivedAt: null,
  });

  it("should flatten multiple pages into single array", () => {
    const data = {
      pages: [
        {
          tasks: [createMockTask("task-1"), createMockTask("task-2")],
          total: 4,
          hasMore: true,
          offset: 0,
        },
        {
          tasks: [createMockTask("task-3"), createMockTask("task-4")],
          total: 4,
          hasMore: false,
          offset: 2,
        },
      ],
      pageParams: [0, 2],
    };

    const result = flattenPages(data);

    expect(result).toHaveLength(4);
    expect(result[0].id).toBe("task-1");
    expect(result[3].id).toBe("task-4");
  });

  it("should return empty array for undefined data", () => {
    const result = flattenPages(undefined);

    expect(result).toEqual([]);
  });

  it("should return empty array for data without pages", () => {
    const result = flattenPages({ pages: [] });

    expect(result).toEqual([]);
  });

  it("should handle single page", () => {
    const data = {
      pages: [
        {
          tasks: [createMockTask("task-1")],
          total: 1,
          hasMore: false,
          offset: 0,
        },
      ],
      pageParams: [0],
    };

    const result = flattenPages(data);

    expect(result).toHaveLength(1);
    expect(result[0].id).toBe("task-1");
  });
});
