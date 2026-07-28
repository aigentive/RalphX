import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, waitFor, act } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { useTaskMutation } from "./useTaskMutation";
import { api } from "@/lib/tauri";
import type { Task } from "@/types/task";

// Mock the tauri API
vi.mock("@/lib/tauri", () => ({
  api: {
    tasks: {
      create: vi.fn(),
      update: vi.fn(),
      delete: vi.fn(),
      move: vi.fn(),
    },
  },
}));

// Helper to create a mock task
const createMockTask = (overrides: Partial<Task> = {}): Task => ({
  id: "task-1",
  projectId: "project-1",
  category: "feature",
  title: "Test Task",
  description: null,
  priority: 0,
  internalStatus: "backlog",
  needsReviewPoint: false,
  createdAt: "2026-01-24T12:00:00Z",
  updatedAt: "2026-01-24T12:00:00Z",
  startedAt: null,
  completedAt: null,
  ...overrides,
});

describe("useTaskMutation", () => {
  let queryClient: QueryClient;

  beforeEach(() => {
    queryClient = new QueryClient({
      defaultOptions: {
        queries: {
          retry: false,
        },
        mutations: {
          retry: false,
        },
      },
    });
    vi.clearAllMocks();
  });

  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );

  describe("createMutation", () => {
    it("should create a new task", async () => {
      const newTask = createMockTask({ id: "new-task-1", title: "New Task" });
      vi.mocked(api.tasks.create).mockResolvedValue(newTask);

      const { result } = renderHook(() => useTaskMutation("project-1"), {
        wrapper,
      });

      await act(async () => {
        result.current.createMutation.mutate({
          projectId: "project-1",
          title: "New Task",
        });
      });

      await waitFor(() => {
        expect(result.current.createMutation.isSuccess).toBe(true);
      });

      expect(api.tasks.create).toHaveBeenCalledWith({
        projectId: "project-1",
        title: "New Task",
      });
      expect(result.current.createMutation.data).toEqual(newTask);
    });

    it("should handle create error", async () => {
      const error = new Error("Failed to create task");
      vi.mocked(api.tasks.create).mockRejectedValue(error);

      const { result } = renderHook(() => useTaskMutation("project-1"), {
        wrapper,
      });

      await act(async () => {
        result.current.createMutation.mutate({
          projectId: "project-1",
          title: "New Task",
        });
      });

      await waitFor(() => {
        expect(result.current.createMutation.isError).toBe(true);
      });

      expect(result.current.createMutation.error).toBe(error);
    });

    it("should invalidate task queries on success", async () => {
      const newTask = createMockTask();
      vi.mocked(api.tasks.create).mockResolvedValue(newTask);

      const invalidateQueriesSpy = vi.spyOn(queryClient, "invalidateQueries");

      const { result } = renderHook(() => useTaskMutation("project-1"), {
        wrapper,
      });

      await act(async () => {
        result.current.createMutation.mutate({
          projectId: "project-1",
          title: "New Task",
        });
      });

      await waitFor(() => {
        expect(result.current.createMutation.isSuccess).toBe(true);
      });

      expect(invalidateQueriesSpy).toHaveBeenCalledWith({
        queryKey: ["tasks", "list", "project-1"],
      });
    });
  });

  describe("updateMutation", () => {
    it("should update an existing task", async () => {
      const updatedTask = createMockTask({ title: "Updated Title" });
      vi.mocked(api.tasks.update).mockResolvedValue(updatedTask);

      const { result } = renderHook(() => useTaskMutation("project-1"), {
        wrapper,
      });

      await act(async () => {
        result.current.updateMutation.mutate({
          taskId: "task-1",
          input: { title: "Updated Title" },
        });
      });

      await waitFor(() => {
        expect(result.current.updateMutation.isSuccess).toBe(true);
      });

      expect(api.tasks.update).toHaveBeenCalledWith("task-1", {
        title: "Updated Title",
      });
      expect(result.current.updateMutation.data).toEqual(updatedTask);
    });

    it("should invalidate queries on update success", async () => {
      const updatedTask = createMockTask();
      vi.mocked(api.tasks.update).mockResolvedValue(updatedTask);

      const invalidateQueriesSpy = vi.spyOn(queryClient, "invalidateQueries");

      const { result } = renderHook(() => useTaskMutation("project-1"), {
        wrapper,
      });

      await act(async () => {
        result.current.updateMutation.mutate({
          taskId: "task-1",
          input: { title: "Updated" },
        });
      });

      await waitFor(() => {
        expect(result.current.updateMutation.isSuccess).toBe(true);
      });

      expect(invalidateQueriesSpy).toHaveBeenCalledWith({
        queryKey: ["tasks", "list", "project-1"],
      });
    });
  });

  describe("moveMutation", () => {
    it("should move a task to a new status", async () => {
      const movedTask = createMockTask({ internalStatus: "ready" });
      vi.mocked(api.tasks.move).mockResolvedValue(movedTask);

      const { result } = renderHook(() => useTaskMutation("project-1"), {
        wrapper,
      });

      await act(async () => {
        result.current.moveMutation.mutate({
          taskId: "task-1",
          toStatus: "ready",
        });
      });

      await waitFor(() => {
        expect(result.current.moveMutation.isSuccess).toBe(true);
      });

      expect(api.tasks.move).toHaveBeenCalledWith("task-1", "ready");
      expect(result.current.moveMutation.data).toEqual(movedTask);
    });

    it("should invalidate queries on move success", async () => {
      const movedTask = createMockTask({ internalStatus: "ready" });
      vi.mocked(api.tasks.move).mockResolvedValue(movedTask);

      const invalidateQueriesSpy = vi.spyOn(queryClient, "invalidateQueries");

      const { result } = renderHook(() => useTaskMutation("project-1"), {
        wrapper,
      });

      await act(async () => {
        result.current.moveMutation.mutate({
          taskId: "task-1",
          toStatus: "ready",
        });
      });

      await waitFor(() => {
        expect(result.current.moveMutation.isSuccess).toBe(true);
      });

      expect(invalidateQueriesSpy).toHaveBeenCalledWith({
        queryKey: ["tasks", "list", "project-1"],
      });
    });

    it("should handle move error", async () => {
      const error = new Error("Invalid status transition");
      vi.mocked(api.tasks.move).mockRejectedValue(error);

      const { result } = renderHook(() => useTaskMutation("project-1"), {
        wrapper,
      });

      await act(async () => {
        result.current.moveMutation.mutate({
          taskId: "task-1",
          toStatus: "invalid",
        });
      });

      await waitFor(() => {
        expect(result.current.moveMutation.isError).toBe(true);
      });

      expect(result.current.moveMutation.error).toBe(error);
    });
  });
});
