import { createElement, type ReactNode } from "react";
import {
  QueryClient,
  QueryClientProvider,
  type Query,
} from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { VerificationStatusResponse } from "@/api/ideation";
import {
  useVerificationStatus,
  verificationRefetchInterval,
  verificationStatusKey,
} from "./useVerificationStatus";

const { getStatusMock, subscribers } = vi.hoisted(() => ({
  getStatusMock: vi.fn(),
  subscribers: new Map<string, (payload: unknown) => void>(),
}));

vi.mock("@/api/ideation", () => ({
  ideationApi: {
    verification: {
      getStatus: (...args: unknown[]) => getStatusMock(...args),
    },
  },
}));

vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => ({
    subscribe: (event: string, callback: (payload: unknown) => void) => {
      subscribers.set(event, callback);
      return vi.fn();
    },
  }),
}));

afterEach(() => {
  subscribers.clear();
  vi.clearAllMocks();
});

function queryWithStatus(
  status: VerificationStatusResponse | undefined,
): Query<VerificationStatusResponse, Error> {
  return {
    state: { data: status },
  } as Query<VerificationStatusResponse, Error>;
}

describe("verificationRefetchInterval", () => {
  it("polls only while verification is queued or running", () => {
    expect(
      verificationRefetchInterval(
        queryWithStatus({
          sessionId: "session-1",
          status: "queued",
          inProgress: true,
          planArtifactId: "plan-1",
          verifiedPlanArtifactId: null,
          agentRunId: null,
          startedAt: null,
          completedAt: null,
          error: null,
        }),
      ),
    ).toBe(2_000);

    expect(
      verificationRefetchInterval(
        queryWithStatus({
          sessionId: "session-1",
          status: "verified",
          inProgress: false,
          planArtifactId: "plan-1",
          verifiedPlanArtifactId: "plan-1",
          agentRunId: "run-1",
          startedAt: null,
          completedAt: null,
          error: null,
        }),
      ),
    ).toBe(false);
  });
});

describe("useVerificationStatus", () => {
  it("refreshes the exact session on verification and ideation lifecycle events", async () => {
    getStatusMock.mockResolvedValue({
      sessionId: "session-1",
      status: "unverified",
      inProgress: false,
    });
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const wrapper = ({ children }: { children: ReactNode }) =>
      createElement(QueryClientProvider, { client: queryClient }, children);

    renderHook(() => useVerificationStatus("session-1"), { wrapper });

    await waitFor(() => expect(getStatusMock).toHaveBeenCalledWith("session-1"));
    expect([...subscribers.keys()]).toEqual([
      "plan_verification:status_changed",
      "agent:run_started",
      "agent:turn_completed",
      "agent:run_completed",
      "agent:error",
    ]);

    subscribers.get("agent:turn_completed")?.({
      context_type: "ideation",
      context_id: "session-1",
    });
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: verificationStatusKey("session-1"),
    });

    invalidate.mockClear();
    subscribers.get("agent:error")?.({
      context_type: "ideation",
      context_id: "another-session",
    });
    expect(invalidate).not.toHaveBeenCalled();
  });
});
