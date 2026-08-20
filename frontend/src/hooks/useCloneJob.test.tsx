/**
 * Tests for useCloneJob.
 *
 * Proof obligation 6: cross-job event rejection, terminal state reached
 * through get_clone_job_status even when every event is missed, and the
 * polling interval stopping on terminal state / unmount. Also covers the
 * plain-language `unknown` failure surface.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { createContext, useContext, type ReactNode } from "react";
import { useCloneJob } from "./useCloneJob";
import { MockEventBus, type EventBus } from "@/lib/event-bus";
import { projectsApi, type StartProjectCloneInput } from "@/api/projects";
import type { CloneJobStatus } from "@/types/clone";

vi.mock("@/api/projects", () => ({
  projectsApi: {
    startProjectClone: vi.fn(),
    getCloneJobStatus: vi.fn(),
    cancelProjectClone: vi.fn(),
  },
}));

let testEventBus: MockEventBus;
const TestEventBusContext = createContext<EventBus | null>(null);

vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => {
    const bus = useContext(TestEventBusContext);
    if (!bus) throw new Error("useEventBus must be used within TestEventBusProvider");
    return bus;
  },
}));

function TestEventBusProvider({ children }: { children: ReactNode }) {
  return (
    <TestEventBusContext.Provider value={testEventBus}>{children}</TestEventBusContext.Provider>
  );
}

const wrapper = ({ children }: { children: ReactNode }) => (
  <TestEventBusProvider>{children}</TestEventBusProvider>
);

const mockStartProjectClone = vi.mocked(projectsApi.startProjectClone);
const mockGetCloneJobStatus = vi.mocked(projectsApi.getCloneJobStatus);
const mockCancelProjectClone = vi.mocked(projectsApi.cancelProjectClone);

const INPUT: StartProjectCloneInput = {
  url: "https://example.com/o/r.git",
  parentDirectory: "/tmp/projects",
};

const RUNNING_STATUS: CloneJobStatus = { state: "running", phase: "receiving", percent: 40 };
function completedStatus(destination: string): CloneJobStatus {
  return {
    state: "completed",
    destination,
    defaultBranch: "main",
    capability: { kind: "localOnly" },
  };
}

describe("useCloneJob", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    testEventBus = new MockEventBus();
    mockStartProjectClone.mockReset();
    mockGetCloneJobStatus.mockReset();
    mockCancelProjectClone.mockReset();
    // Default: never-resolving-to-terminal running status, overridden per test.
    mockGetCloneJobStatus.mockResolvedValue(RUNNING_STATUS);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("subscribes to all four clone events on mount, before any start() call", () => {
    renderHook(() => useCloneJob(), { wrapper });

    expect(testEventBus.getListenerCount("project:clone_progress")).toBe(1);
    expect(testEventBus.getListenerCount("project:clone_completed")).toBe(1);
    expect(testEventBus.getListenerCount("project:clone_failed")).toBe(1);
    expect(testEventBus.getListenerCount("project:clone_cancelled")).toBe(1);
    expect(mockStartProjectClone).not.toHaveBeenCalled();
  });

  it("rejects a cancelled job's late events after a newer job has started", async () => {
    mockStartProjectClone
      .mockResolvedValueOnce({ jobId: "job-a" })
      .mockResolvedValueOnce({ jobId: "job-b" });
    // Keep both jobs "running" via polling so only the emitted event can settle them.
    mockGetCloneJobStatus.mockResolvedValue(RUNNING_STATUS);

    const { result } = renderHook(() => useCloneJob(), { wrapper });

    await act(async () => {
      await result.current.start(INPUT);
    });
    expect(result.current.status?.state).toBe("running");

    // Job A is superseded by job B before A's terminal event arrives late.
    await act(async () => {
      await result.current.start(INPUT);
    });

    await act(async () => {
      testEventBus.emit("project:clone_cancelled", { jobId: "job-a", state: "cancelled", cleanedUp: true });
    });

    // Job A's late cancellation must never touch job B's state.
    expect(result.current.status?.state).toBe("running");
    expect(result.current.cleanedUp).toBeNull();

    await act(async () => {
      testEventBus.emit("project:clone_completed", {
        jobId: "job-b",
        state: "completed",
        destination: "/tmp/projects/r",
        defaultBranch: "main",
        capability: { kind: "local_only" },
      });
    });

    expect(result.current.status?.state).toBe("completed");
  });

  it("reaches terminal state via get_clone_job_status when every event is missed", async () => {
    mockStartProjectClone.mockResolvedValueOnce({ jobId: "job-x" });
    mockGetCloneJobStatus
      .mockResolvedValueOnce(RUNNING_STATUS)
      .mockResolvedValueOnce(completedStatus("/tmp/projects/r"));

    const { result } = renderHook(() => useCloneJob(), { wrapper });

    await act(async () => {
      await result.current.start(INPUT);
    });
    // Immediate post-start reconciliation call.
    expect(mockGetCloneJobStatus).toHaveBeenCalledTimes(1);
    expect(result.current.status?.state).toBe("running");

    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });

    expect(result.current.status?.state).toBe("completed");
    expect(mockGetCloneJobStatus).toHaveBeenCalledTimes(2);
  });

  it("stops the polling interval once a terminal state is reached", async () => {
    mockStartProjectClone.mockResolvedValueOnce({ jobId: "job-y" });
    mockGetCloneJobStatus
      .mockResolvedValueOnce(RUNNING_STATUS)
      .mockResolvedValueOnce(completedStatus("/tmp/projects/r"));

    const { result } = renderHook(() => useCloneJob(), { wrapper });

    await act(async () => {
      await result.current.start(INPUT);
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });
    expect(result.current.status?.state).toBe("completed");
    const callsAtSettle = mockGetCloneJobStatus.mock.calls.length;

    await act(async () => {
      await vi.advanceTimersByTimeAsync(10_000);
    });

    expect(mockGetCloneJobStatus).toHaveBeenCalledTimes(callsAtSettle);
  });

  it("stops the polling interval on unmount", async () => {
    mockStartProjectClone.mockResolvedValueOnce({ jobId: "job-z" });
    mockGetCloneJobStatus.mockResolvedValue(RUNNING_STATUS);

    const { result, unmount } = renderHook(() => useCloneJob(), { wrapper });

    await act(async () => {
      await result.current.start(INPUT);
    });
    const callsBeforeUnmount = mockGetCloneJobStatus.mock.calls.length;

    unmount();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(10_000);
    });

    expect(mockGetCloneJobStatus).toHaveBeenCalledTimes(callsBeforeUnmount);
  });

  it("surfaces a plain-language error for an unknown status instead of spinning", async () => {
    mockStartProjectClone.mockResolvedValueOnce({ jobId: "job-lost" });
    mockGetCloneJobStatus.mockResolvedValueOnce({ state: "unknown" });

    const { result } = renderHook(() => useCloneJob(), { wrapper });

    await act(async () => {
      await result.current.start(INPUT);
    });

    expect(result.current.status?.state).toBe("unknown");
    expect(result.current.error).toBe("We lost track of this clone. Try again.");
    expect(result.current.error).not.toMatch(/CLONE_|git |ref /i);

    // No further polling — the spinner never resumes for a settled unknown job.
    const callsAtSettle = mockGetCloneJobStatus.mock.calls.length;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(10_000);
    });
    expect(mockGetCloneJobStatus).toHaveBeenCalledTimes(callsAtSettle);
  });

  it("resolves start() to false and never assigns a job id when startProjectClone rejects", async () => {
    mockStartProjectClone.mockRejectedValueOnce(new Error("Folder name cannot contain '/'"));

    const { result } = renderHook(() => useCloneJob(), { wrapper });

    let started: boolean | undefined;
    await act(async () => {
      started = await result.current.start(INPUT);
    });

    expect(started).toBe(false);
    expect(result.current.error).toBe("Folder name cannot contain '/'");
    expect(mockGetCloneJobStatus).not.toHaveBeenCalled();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(10_000);
    });
    expect(mockGetCloneJobStatus).not.toHaveBeenCalled();
  });

  it("cancel() calls cancelProjectClone with the active job id", async () => {
    mockStartProjectClone.mockResolvedValueOnce({ jobId: "job-cancel-me" });
    mockCancelProjectClone.mockResolvedValueOnce(true);

    const { result } = renderHook(() => useCloneJob(), { wrapper });

    await act(async () => {
      await result.current.start(INPUT);
    });
    await act(async () => {
      await result.current.cancel();
    });

    expect(mockCancelProjectClone).toHaveBeenCalledWith("job-cancel-me");
  });
});
