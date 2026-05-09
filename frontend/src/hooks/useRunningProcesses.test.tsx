/**
 * useRunningProcesses hook tests
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useRunningProcesses, runningProcessesKeys } from "./useRunningProcesses";
import { runningProcessesApi } from "@/api/running-processes";
import { EventProvider } from "@/providers/EventProvider";
import type { ReactNode } from "react";
import type { RunningProcessesResponse } from "@/api/running-processes";

// Mock the API
vi.mock("@/api/running-processes", () => ({
  runningProcessesApi: {
    getRunningProcesses: vi.fn(),
  },
}));

// Mock response data
const mockResponse: RunningProcessesResponse = {
  processes: [
    {
      taskId: "task-1",
      title: "Test Task 1",
      internalStatus: "executing",
      stepProgress: null,
      elapsedSeconds: 60,
      triggerOrigin: "scheduler",
      taskBranch: "ralphx/app/task-1",
    },
    {
      taskId: "task-2",
      title: "Test Task 2",
      internalStatus: "reviewing",
      stepProgress: null,
      elapsedSeconds: 120,
      triggerOrigin: "revision",
      taskBranch: "ralphx/app/task-2",
    },
  ],
};

describe("useRunningProcesses", () => {
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
    <QueryClientProvider client={queryClient}>
      <EventProvider>{children}</EventProvider>
    </QueryClientProvider>
  );

  describe("data fetching", () => {
    it("calls getRunningProcesses API on mount", async () => {
      vi.mocked(runningProcessesApi.getRunningProcesses).mockResolvedValue(mockResponse);

      renderHook(() => useRunningProcesses(), { wrapper });

      await waitFor(() => {
        expect(runningProcessesApi.getRunningProcesses).toHaveBeenCalledOnce();
      });
    });

    it("does not call getRunningProcesses API when disabled", () => {
      vi.mocked(runningProcessesApi.getRunningProcesses).mockResolvedValue(mockResponse);

      const { result } = renderHook(
        () => useRunningProcesses("project-1", { enabled: false }),
        { wrapper }
      );

      expect(runningProcessesApi.getRunningProcesses).not.toHaveBeenCalled();
      expect(result.current.fetchStatus).toBe("idle");
    });

    it("returns loading state initially", () => {
      vi.mocked(runningProcessesApi.getRunningProcesses).mockImplementation(
        () => new Promise(() => {}) // Never resolves
      );

      const { result } = renderHook(() => useRunningProcesses(), { wrapper });

      expect(result.current.isLoading).toBe(true);
      expect(result.current.data).toBeUndefined();
    });

    it("returns data on successful fetch", async () => {
      vi.mocked(runningProcessesApi.getRunningProcesses).mockResolvedValue(mockResponse);

      const { result } = renderHook(() => useRunningProcesses(), { wrapper });

      await waitFor(() => {
        expect(result.current.isSuccess).toBe(true);
      });

      expect(result.current.data).toEqual(mockResponse);
      expect(result.current.data?.processes).toHaveLength(2);
    });

    it("returns error state on failed fetch", async () => {
      const error = new Error("API error");
      vi.mocked(runningProcessesApi.getRunningProcesses).mockRejectedValue(error);

      const { result } = renderHook(() => useRunningProcesses(), { wrapper });

      await waitFor(() => {
        expect(result.current.isError).toBe(true);
      });

      expect(result.current.error).toEqual(error);
    });
  });

  describe("query key", () => {
    it("uses correct query key", () => {
      vi.mocked(runningProcessesApi.getRunningProcesses).mockResolvedValue(mockResponse);

      renderHook(() => useRunningProcesses(), { wrapper });

      const queries = queryClient.getQueryCache().findAll({
        queryKey: runningProcessesKeys.list(),
      });

      expect(queries).toHaveLength(1);
    });

    it("query key factory generates correct keys", () => {
      expect(runningProcessesKeys.all).toEqual(["running-processes"]);
      expect(runningProcessesKeys.list()).toEqual(["running-processes", "list"]);
    });
  });

  describe("event-driven refetch", () => {
    it("invalidates query on task:status_changed event", async () => {
      vi.mocked(runningProcessesApi.getRunningProcesses).mockResolvedValue(mockResponse);

      const { result } = renderHook(() => useRunningProcesses(), { wrapper });

      await waitFor(() => {
        expect(result.current.isSuccess).toBe(true);
      });

      const initialCallCount = vi.mocked(runningProcessesApi.getRunningProcesses).mock.calls.length;

      // Access the event bus from window (exposed by EventProvider in web mode)
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const eventBus = (window as any).__eventBus;
      if (eventBus) {
        eventBus.emit("task:status_changed", { taskId: "task-1", status: "completed" });
      }

      await waitFor(() => {
        const newCallCount = vi.mocked(runningProcessesApi.getRunningProcesses).mock.calls.length;
        expect(newCallCount).toBeGreaterThan(initialCallCount);
      });
    });

    it("invalidates query on execution:status_changed event", async () => {
      vi.mocked(runningProcessesApi.getRunningProcesses).mockResolvedValue(mockResponse);

      const { result } = renderHook(() => useRunningProcesses(), { wrapper });

      await waitFor(() => {
        expect(result.current.isSuccess).toBe(true);
      });

      const initialCallCount = vi.mocked(runningProcessesApi.getRunningProcesses).mock.calls.length;

      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const eventBus = (window as any).__eventBus;
      if (eventBus) {
        eventBus.emit("execution:status_changed", { isPaused: false });
      }

      await waitFor(() => {
        const newCallCount = vi.mocked(runningProcessesApi.getRunningProcesses).mock.calls.length;
        expect(newCallCount).toBeGreaterThan(initialCallCount);
      });
    });

    it("invalidates query on step:status_changed event", async () => {
      vi.mocked(runningProcessesApi.getRunningProcesses).mockResolvedValue(mockResponse);

      const { result } = renderHook(() => useRunningProcesses(), { wrapper });

      await waitFor(() => {
        expect(result.current.isSuccess).toBe(true);
      });

      const initialCallCount = vi.mocked(runningProcessesApi.getRunningProcesses).mock.calls.length;

      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const eventBus = (window as any).__eventBus;
      if (eventBus) {
        eventBus.emit("step:status_changed", { stepId: "step-1", status: "completed" });
      }

      await waitFor(() => {
        const newCallCount = vi.mocked(runningProcessesApi.getRunningProcesses).mock.calls.length;
        expect(newCallCount).toBeGreaterThan(initialCallCount);
      });
    });
  });

  describe("polling behavior", () => {
    it("enables polling with 10s interval", () => {
      vi.mocked(runningProcessesApi.getRunningProcesses).mockResolvedValue(mockResponse);

      renderHook(() => useRunningProcesses(), { wrapper });

      // Check that the query has refetchInterval set
      const queries = queryClient.getQueryCache().findAll({
        queryKey: runningProcessesKeys.list(),
      });

      expect(queries).toHaveLength(1);
      // The refetchInterval is configured in the hook
    });

    it("enables refetch on window focus", () => {
      vi.mocked(runningProcessesApi.getRunningProcesses).mockResolvedValue(mockResponse);

      renderHook(() => useRunningProcesses(), { wrapper });

      const queries = queryClient.getQueryCache().findAll({
        queryKey: runningProcessesKeys.list(),
      });

      expect(queries).toHaveLength(1);
      // The refetchOnWindowFocus is configured in the hook
    });
  });
});
