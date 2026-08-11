import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { startupApi, type StartupStatus } from "@/api/startup";
import { useStartupStatus } from "./useStartupStatus";

vi.mock("@/api/startup", () => ({
  startupApi: {
    getStatus: vi.fn(),
    retry: vi.fn(),
    reportFrontendMilestone: vi.fn(),
  },
}));

function startupStatus(overrides: Partial<StartupStatus> = {}): StartupStatus {
  return {
    bootId: "boot-1",
    attemptId: 1,
    stage: "safety_recovery",
    startedAt: "2026-07-24T09:00:00Z",
    stageStartedAt: "2026-07-24T09:00:01Z",
    completedAt: null,
    appStateReady: true,
    runtimeReady: false,
    backgroundComplete: false,
    retryAllowed: false,
    progress: null,
    messageCode: "checking_interrupted_work",
    failureCode: null,
    diagnosticSummary: null,
    ...overrides,
  };
}

function createWrapper() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });

  return function Wrapper({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
  };
}

describe("useStartupStatus", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("does not let a stale poll revoke already accepted runtime readiness", async () => {
    vi.mocked(startupApi.getStatus)
      .mockResolvedValueOnce(startupStatus({ stage: "runtime_ready", runtimeReady: true }))
      .mockResolvedValueOnce(startupStatus({ stage: "safety_recovery", runtimeReady: false }));

    const { result } = renderHook(() => useStartupStatus(), {
      wrapper: createWrapper(),
    });

    await waitFor(() => expect(result.current.status?.runtimeReady).toBe(true));

    await act(async () => {
      await result.current.refetch();
    });

    expect(result.current.status?.stage).toBe("runtime_ready");
    expect(result.current.canMountApp).toBe(true);
  });

  it("accepts a newer retry attempt and exposes its retry operation", async () => {
    vi.mocked(startupApi.getStatus).mockResolvedValue(
      startupStatus({
        stage: "failed",
        failureCode: "database_open_failed",
        retryAllowed: true,
      }),
    );
    vi.mocked(startupApi.retry).mockResolvedValue(
      startupStatus({
        attemptId: 2,
        stage: "opening_database",
        appStateReady: false,
      }),
    );

    const { result } = renderHook(() => useStartupStatus(), {
      wrapper: createWrapper(),
    });

    await waitFor(() => expect(result.current.isTerminalFailure).toBe(true));

    await act(async () => {
      await result.current.retry();
    });

    expect(startupApi.retry).toHaveBeenCalledWith();
    await waitFor(() => {
      expect(result.current.status?.attemptId).toBe(2);
      expect(result.current.isTerminalFailure).toBe(false);
    });
  });

  it("does not let a late terminal snapshot overwrite a settled outcome", async () => {
    vi.mocked(startupApi.getStatus)
      .mockResolvedValueOnce(startupStatus({
        stage: "ready",
        runtimeReady: true,
        backgroundComplete: true,
      }))
      .mockResolvedValueOnce(startupStatus({
        stage: "failed",
        failureCode: "local_runtime_bind",
      }));

    const { result } = renderHook(() => useStartupStatus(), {
      wrapper: createWrapper(),
    });
    await waitFor(() => expect(result.current.status?.stage).toBe("ready"));

    await act(async () => {
      await result.current.refetch();
    });

    expect(result.current.status?.stage).toBe("ready");
    expect(result.current.status?.backgroundComplete).toBe(true);
  });
});
