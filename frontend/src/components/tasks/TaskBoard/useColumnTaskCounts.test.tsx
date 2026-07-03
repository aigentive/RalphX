/**
 * Tests for useColumnTaskCounts hook
 */

import { describe, it, expect, beforeEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { InfiniteData } from "@tanstack/react-query";
import { infiniteTaskKeys } from "@/hooks/useInfiniteTasksQuery";
import { createMockTask } from "@/test/mock-data";
import { defaultWorkflow, type WorkflowColumn } from "@/types/workflow";
import type { InternalStatus } from "@/types/status";
import type { TaskListResponse } from "@/types/task";
import { useColumnTaskCounts } from "./useColumnTaskCounts";

const PROJECT_ID = "project-1";
const SESSION_ID = "session-1";

// Simple columns (no groups)
const simpleColumns: WorkflowColumn[] = [
  { id: "draft", name: "Draft", mapsTo: "backlog" },
  { id: "ready", name: "Ready", mapsTo: "ready" },
];

// Column with groups
const groupedColumns: WorkflowColumn[] = [
  { id: "draft", name: "Draft", mapsTo: "backlog" },
  {
    id: "ready",
    name: "Ready",
    mapsTo: "ready",
    groups: [
      { id: "fresh", label: "Fresh", statuses: ["ready"] },
      { id: "blocked", label: "Blocked", statuses: ["blocked"] },
    ],
  },
];

let queryClient: QueryClient;

function createWrapper() {
  return ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
}

function seedCache(
  statuses: string[],
  taskCount: number,
  includeArchived = false,
  ideationSessionId: string | null = SESSION_ID,
) {
  const key = infiniteTaskKeys.list({
    projectId: PROJECT_ID,
    statuses: statuses as InternalStatus[],
    includeArchived,
    ideationSessionId,
  });
  const tasks = Array.from({ length: taskCount }, (_, i) =>
    createMockTask({ id: `task-${statuses[0]}-${i}`, internalStatus: statuses[0] as InternalStatus }),
  );
  const data: InfiniteData<TaskListResponse> = {
    pages: [{ tasks, total: taskCount, hasMore: false, offset: 0 }],
    pageParams: [0],
  };
  queryClient.setQueryData(key, data);
}

describe("useColumnTaskCounts", () => {
  beforeEach(() => {
    queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
  });

  it("returns 0 counts when cache is empty", () => {
    const { result } = renderHook(
      () => useColumnTaskCounts(simpleColumns, PROJECT_ID, false, SESSION_ID),
      { wrapper: createWrapper() },
    );

    expect(result.current.get("draft")).toBe(0);
    expect(result.current.get("ready")).toBe(0);
  });

  it("reads task counts from query cache", () => {
    seedCache(["backlog"], 3);
    seedCache(["ready"], 5);

    const { result } = renderHook(
      () => useColumnTaskCounts(simpleColumns, PROJECT_ID, false, SESSION_ID),
      { wrapper: createWrapper() },
    );

    expect(result.current.get("draft")).toBe(3);
    expect(result.current.get("ready")).toBe(5);
  });

  it("handles grouped columns by combining statuses", () => {
    // "ready" column groups: ["ready"] + ["blocked"]
    seedCache(["ready", "blocked"], 4);

    const { result } = renderHook(
      () => useColumnTaskCounts(groupedColumns, PROJECT_ID, false, SESSION_ID),
      { wrapper: createWrapper() },
    );

    expect(result.current.get("ready")).toBe(4);
  });

  it("counts paused tasks under the default In Progress lane", () => {
    seedCache(["executing", "re_executing", "paused"], 1);

    const { result } = renderHook(
      () => useColumnTaskCounts(defaultWorkflow.columns, PROJECT_ID, false, SESSION_ID),
      { wrapper: createWrapper() },
    );

    expect(result.current.get("in_progress")).toBe(1);
  });

  it("reacts to cache updates", () => {
    seedCache(["backlog"], 2);

    const { result } = renderHook(
      () => useColumnTaskCounts(simpleColumns, PROJECT_ID, false, SESSION_ID),
      { wrapper: createWrapper() },
    );

    expect(result.current.get("draft")).toBe(2);

    // Update cache with more tasks
    act(() => {
      seedCache(["backlog"], 7);
    });

    expect(result.current.get("draft")).toBe(7);
  });

  it("returns stable reference when counts unchanged", () => {
    seedCache(["backlog"], 2);
    seedCache(["ready"], 3);

    const { result } = renderHook(
      () => useColumnTaskCounts(simpleColumns, PROJECT_ID, false, SESSION_ID),
      { wrapper: createWrapper() },
    );

    const first = result.current;

    // Trigger a cache event that doesn't change counts (re-set same data)
    act(() => {
      seedCache(["backlog"], 2);
    });

    // Reference should be stable
    expect(result.current).toBe(first);
  });

  it("returns new reference when counts change", () => {
    seedCache(["backlog"], 2);

    const { result } = renderHook(
      () => useColumnTaskCounts(simpleColumns, PROJECT_ID, false, SESSION_ID),
      { wrapper: createWrapper() },
    );

    const first = result.current;

    act(() => {
      seedCache(["backlog"], 5);
    });

    expect(result.current).not.toBe(first);
    expect(result.current.get("draft")).toBe(5);
  });

  it("excludes merge tasks when showMergeTasks is false", () => {
    const key = infiniteTaskKeys.list({
      projectId: PROJECT_ID,
      statuses: ["backlog"],
      includeArchived: false,
      ideationSessionId: SESSION_ID,
    });

    const tasks = [
      createMockTask({ id: "t1", internalStatus: "backlog", category: "feature" }),
      createMockTask({ id: "t2", internalStatus: "backlog", category: "plan_merge" }),
      createMockTask({ id: "t3", internalStatus: "backlog", category: "feature" }),
    ];
    const data: InfiniteData<TaskListResponse> = {
      pages: [{ tasks, total: 3, hasMore: false, offset: 0 }],
      pageParams: [0],
    };
    queryClient.setQueryData(key, data);

    const { result } = renderHook(
      () => useColumnTaskCounts(simpleColumns, PROJECT_ID, false, SESSION_ID, false),
      { wrapper: createWrapper() },
    );

    // Only non-merge tasks counted
    expect(result.current.get("draft")).toBe(2);
  });

  it("includes merge tasks when showMergeTasks is true", () => {
    const key = infiniteTaskKeys.list({
      projectId: PROJECT_ID,
      statuses: ["backlog"],
      includeArchived: false,
      ideationSessionId: SESSION_ID,
    });

    const tasks = [
      createMockTask({ id: "t1", internalStatus: "backlog", category: "feature" }),
      createMockTask({ id: "t2", internalStatus: "backlog", category: "plan_merge" }),
    ];
    const data: InfiniteData<TaskListResponse> = {
      pages: [{ tasks, total: 2, hasMore: false, offset: 0 }],
      pageParams: [0],
    };
    queryClient.setQueryData(key, data);

    const { result } = renderHook(
      () => useColumnTaskCounts(simpleColumns, PROJECT_ID, false, SESSION_ID, true),
      { wrapper: createWrapper() },
    );

    expect(result.current.get("draft")).toBe(2);
  });

  it("sums across multiple pages", () => {
    const key = infiniteTaskKeys.list({
      projectId: PROJECT_ID,
      statuses: ["backlog"],
      includeArchived: false,
      ideationSessionId: SESSION_ID,
    });

    const page1Tasks = Array.from({ length: 3 }, (_, i) =>
      createMockTask({ id: `p1-${i}`, internalStatus: "backlog" }),
    );
    const page2Tasks = Array.from({ length: 4 }, (_, i) =>
      createMockTask({ id: `p2-${i}`, internalStatus: "backlog" }),
    );

    const data: InfiniteData<TaskListResponse> = {
      pages: [
        { tasks: page1Tasks, total: 7, hasMore: true, offset: 0 },
        { tasks: page2Tasks, total: 7, hasMore: false, offset: 3 },
      ],
      pageParams: [0, 3],
    };
    queryClient.setQueryData(key, data);

    const { result } = renderHook(
      () => useColumnTaskCounts(simpleColumns, PROJECT_ID, false, SESSION_ID),
      { wrapper: createWrapper() },
    );

    expect(result.current.get("draft")).toBe(7);
  });
});
