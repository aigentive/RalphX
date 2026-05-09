import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, act, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import {
  useExecutionStatus,
  usePauseExecution,
  useStopExecution,
  executionKeys,
} from "./useExecutionControl";
import { useUiStore } from "@/stores/uiStore";
import type { ExecutionStatusResponse, ExecutionCommandResponse } from "@/lib/tauri";

// Mock Tauri API
vi.mock("@/lib/tauri", () => ({
  api: {
    execution: {
      getStatus: vi.fn(),
      pause: vi.fn(),
      resume: vi.fn(),
      stop: vi.fn(),
    },
  },
  ExecutionStatusResponseSchema: {},
  ExecutionCommandResponseSchema: {},
}));

// Import the mocked module
import { api } from "@/lib/tauri";

// Helper to create wrapper with QueryClient
function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
        gcTime: 0,
      },
    },
  });

  return ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
}

const mockStatus: ExecutionStatusResponse = {
  isPaused: false,
  haltMode: "running",
  runningCount: 0,
  maxConcurrent: 10,
  queuedCount: 0,
  canStartTask: true,
};

const mockCommandResponse: ExecutionCommandResponse = {
  success: true,
  status: mockStatus,
};

describe("executionKeys", () => {
  it("generates correct base key", () => {
    expect(executionKeys.all).toEqual(["execution"]);
  });

  it("generates correct status key", () => {
    expect(executionKeys.status()).toEqual(["execution", "status", "all"]);
  });
});

describe("useExecutionStatus", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useUiStore.setState({
      executionStatus: {
        isPaused: false,
        haltMode: "running",
        runningCount: 0,
        maxConcurrent: 10,
        queuedCount: 0,
        canStartTask: true,
      },
    });
  });

  afterEach(() => {
    vi.resetAllMocks();
  });

  it("fetches execution status on mount", async () => {
    vi.mocked(api.execution.getStatus).mockResolvedValue(mockStatus);

    const { result } = renderHook(() => useExecutionStatus(), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(result.current.isSuccess).toBe(true);
    });

    expect(api.execution.getStatus).toHaveBeenCalledTimes(1);
    expect(result.current.data).toEqual(mockStatus);
  });

  it("does not fetch execution status when disabled", async () => {
    vi.mocked(api.execution.getStatus).mockResolvedValue(mockStatus);

    const { result } = renderHook(
      () => useExecutionStatus("project-1", { enabled: false }),
      { wrapper: createWrapper() }
    );

    expect(api.execution.getStatus).not.toHaveBeenCalled();
    expect(result.current.isPaused).toBe(false);
  });

  it("updates uiStore on successful fetch", async () => {
    const pausedStatus: ExecutionStatusResponse = {
      isPaused: true,
      haltMode: "paused",
      runningCount: 1,
      maxConcurrent: 10,
      queuedCount: 3,
      canStartTask: false,
    };
    vi.mocked(api.execution.getStatus).mockResolvedValue(pausedStatus);

    renderHook(() => useExecutionStatus(), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(useUiStore.getState().executionStatus).toEqual(pausedStatus);
    });
  });

  it("returns isPaused from data", async () => {
    vi.mocked(api.execution.getStatus).mockResolvedValue({
      ...mockStatus,
      isPaused: true,
    });

    const { result } = renderHook(() => useExecutionStatus(), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(result.current.isPaused).toBe(true);
    });
  });

  it("returns runningCount from data", async () => {
    vi.mocked(api.execution.getStatus).mockResolvedValue({
      ...mockStatus,
      runningCount: 2,
    });

    const { result } = renderHook(() => useExecutionStatus(), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(result.current.runningCount).toBe(2);
    });
  });

  it("returns queuedCount from data", async () => {
    vi.mocked(api.execution.getStatus).mockResolvedValue({
      ...mockStatus,
      queuedCount: 5,
    });

    const { result } = renderHook(() => useExecutionStatus(), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(result.current.queuedCount).toBe(5);
    });
  });

  it("returns maxConcurrent from data", async () => {
    vi.mocked(api.execution.getStatus).mockResolvedValue({
      ...mockStatus,
      maxConcurrent: 4,
    });

    const { result } = renderHook(() => useExecutionStatus(), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(result.current.maxConcurrent).toBe(4);
    });
  });

  it("returns canStartTask from data", async () => {
    vi.mocked(api.execution.getStatus).mockResolvedValue({
      ...mockStatus,
      canStartTask: false,
    });

    const { result } = renderHook(() => useExecutionStatus(), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(result.current.canStartTask).toBe(false);
    });
  });

  it("handles error state", async () => {
    vi.mocked(api.execution.getStatus).mockRejectedValue(new Error("Network error"));

    const { result } = renderHook(() => useExecutionStatus(), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(result.current.isError).toBe(true);
    });

    expect(result.current.error?.message).toBe("Network error");
  });
});

describe("usePauseExecution", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useUiStore.setState({
      executionStatus: {
        isPaused: false,
        runningCount: 0,
        maxConcurrent: 10,
        queuedCount: 0,
        canStartTask: true,
      },
    });
  });

  afterEach(() => {
    vi.resetAllMocks();
  });

  it("calls pause API when toggling from running to paused", async () => {
    vi.mocked(api.execution.pause).mockResolvedValue({
      success: true,
      status: { ...mockStatus, isPaused: true, haltMode: "paused" },
    });

    const { result } = renderHook(() => usePauseExecution(), {
      wrapper: createWrapper(),
    });

    await act(async () => {
      result.current.toggle();
    });

    expect(api.execution.pause).toHaveBeenCalledTimes(1);
  });

  it("calls resume API when toggling from paused to running", async () => {
    useUiStore.setState({
      executionStatus: { ...mockStatus, isPaused: true, haltMode: "paused" },
    });

    vi.mocked(api.execution.resume).mockResolvedValue({
      success: true,
      status: mockStatus,
    });

    const { result } = renderHook(() => usePauseExecution(), {
      wrapper: createWrapper(),
    });

    await act(async () => {
      result.current.toggle();
    });

    expect(api.execution.resume).toHaveBeenCalledTimes(1);
  });

  it("updates uiStore after pause", async () => {
    const pausedStatus = { ...mockStatus, isPaused: true };
    pausedStatus.haltMode = "paused";
    vi.mocked(api.execution.pause).mockResolvedValue({
      success: true,
      status: pausedStatus,
    });

    const { result } = renderHook(() => usePauseExecution(), {
      wrapper: createWrapper(),
    });

    await act(async () => {
      result.current.toggle();
    });

    await waitFor(() => {
      expect(useUiStore.getState().executionStatus.isPaused).toBe(true);
    });
  });

  it("updates uiStore after resume", async () => {
    useUiStore.setState({
      executionStatus: { ...mockStatus, isPaused: true, haltMode: "paused" },
    });

    vi.mocked(api.execution.resume).mockResolvedValue({
      success: true,
      status: mockStatus,
    });

    const { result } = renderHook(() => usePauseExecution(), {
      wrapper: createWrapper(),
    });

    await act(async () => {
      result.current.toggle();
    });

    await waitFor(() => {
      expect(useUiStore.getState().executionStatus.isPaused).toBe(false);
    });
  });

  it("provides isPending state during mutation", async () => {
    let resolvePromise: (value: ExecutionCommandResponse) => void;
    const promise = new Promise<ExecutionCommandResponse>((resolve) => {
      resolvePromise = resolve;
    });
    vi.mocked(api.execution.pause).mockReturnValue(promise);

    const { result } = renderHook(() => usePauseExecution(), {
      wrapper: createWrapper(),
    });

    expect(result.current.isPending).toBe(false);

    act(() => {
      result.current.toggle();
    });

    await waitFor(() => {
      expect(result.current.isPending).toBe(true);
    });

    await act(async () => {
      resolvePromise!({
        success: true,
        status: { ...mockStatus, isPaused: true, haltMode: "paused" },
      });
    });

    await waitFor(() => {
      expect(result.current.isPending).toBe(false);
    });
  });

  it("handles pause error", async () => {
    vi.mocked(api.execution.pause).mockRejectedValue(new Error("Pause failed"));

    const { result } = renderHook(() => usePauseExecution(), {
      wrapper: createWrapper(),
    });

    await act(async () => {
      try {
        result.current.toggle();
      } catch {
        // Expected
      }
    });

    await waitFor(() => {
      expect(result.current.isError).toBe(true);
    });
  });

  it("exposes pause and resume methods directly", async () => {
    vi.mocked(api.execution.pause).mockResolvedValue(mockCommandResponse);
    vi.mocked(api.execution.resume).mockResolvedValue(mockCommandResponse);

    const { result } = renderHook(() => usePauseExecution(), {
      wrapper: createWrapper(),
    });

    expect(typeof result.current.pause).toBe("function");
    expect(typeof result.current.resume).toBe("function");
  });

  it("starts execution when halt mode is stopped", async () => {
    useUiStore.setState({
      executionStatus: { ...mockStatus, isPaused: true, haltMode: "stopped" },
    });

    vi.mocked(api.execution.resume).mockResolvedValue({
      success: true,
      status: mockStatus,
    });

    const { result } = renderHook(() => usePauseExecution(), {
      wrapper: createWrapper(),
    });

    await act(async () => {
      result.current.toggle();
    });

    expect(api.execution.resume).toHaveBeenCalledTimes(1);
    await waitFor(() => {
      expect(result.current.canPauseToggle).toBe(true);
    });
  });
});

describe("useStopExecution", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useUiStore.setState({
      executionStatus: {
        isPaused: false,
        runningCount: 1,
        maxConcurrent: 10,
        queuedCount: 2,
        canStartTask: true,
      },
    });
  });

  afterEach(() => {
    vi.resetAllMocks();
  });

  it("calls stop API", async () => {
    const stoppedStatus = {
      ...mockStatus,
      isPaused: true,
      runningCount: 0,
      queuedCount: 2,
    };
    vi.mocked(api.execution.stop).mockResolvedValue({
      success: true,
      status: stoppedStatus,
    });

    const { result } = renderHook(() => useStopExecution(), {
      wrapper: createWrapper(),
    });

    await act(async () => {
      result.current.stop();
    });

    expect(api.execution.stop).toHaveBeenCalledTimes(1);
  });

  it("updates uiStore after stop", async () => {
    const stoppedStatus: ExecutionStatusResponse = {
      isPaused: true,
      runningCount: 0,
      maxConcurrent: 10,
      queuedCount: 2,
      canStartTask: false,
    };
    vi.mocked(api.execution.stop).mockResolvedValue({
      success: true,
      status: stoppedStatus,
    });

    const { result } = renderHook(() => useStopExecution(), {
      wrapper: createWrapper(),
    });

    await act(async () => {
      result.current.stop();
    });

    await waitFor(() => {
      expect(useUiStore.getState().executionStatus.isPaused).toBe(true);
      expect(useUiStore.getState().executionStatus.runningCount).toBe(0);
    });
  });

  it("provides isPending state during mutation", async () => {
    let resolvePromise: (value: ExecutionCommandResponse) => void;
    const promise = new Promise<ExecutionCommandResponse>((resolve) => {
      resolvePromise = resolve;
    });
    vi.mocked(api.execution.stop).mockReturnValue(promise);

    const { result } = renderHook(() => useStopExecution(), {
      wrapper: createWrapper(),
    });

    expect(result.current.isPending).toBe(false);

    act(() => {
      result.current.stop();
    });

    await waitFor(() => {
      expect(result.current.isPending).toBe(true);
    });

    await act(async () => {
      resolvePromise!({
        success: true,
        status: { ...mockStatus, isPaused: true },
      });
    });

    await waitFor(() => {
      expect(result.current.isPending).toBe(false);
    });
  });

  it("handles stop error", async () => {
    vi.mocked(api.execution.stop).mockRejectedValue(new Error("Stop failed"));

    const { result } = renderHook(() => useStopExecution(), {
      wrapper: createWrapper(),
    });

    await act(async () => {
      try {
        result.current.stop();
      } catch {
        // Expected
      }
    });

    await waitFor(() => {
      expect(result.current.isError).toBe(true);
    });
  });

  it("canStop returns true when running tasks exist", () => {
    useUiStore.setState({
      executionStatus: {
        ...mockStatus,
        runningCount: 1,
      },
    });

    const { result } = renderHook(() => useStopExecution(), {
      wrapper: createWrapper(),
    });

    expect(result.current.canStop).toBe(true);
  });

  it("canStop returns false when no running tasks", () => {
    useUiStore.setState({
      executionStatus: {
        ...mockStatus,
        runningCount: 0,
      },
    });

    const { result } = renderHook(() => useStopExecution(), {
      wrapper: createWrapper(),
    });

    expect(result.current.canStop).toBe(false);
  });
});
