import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useAgentWorkflowEvents } from "./useAgentWorkflowEvents";

const testState = vi.hoisted(() => ({
  subscriptions: new Map<string, (payload: unknown) => void>(),
  invalidateQueries: vi.fn(),
}));

vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => ({
    subscribe: (event: string, handler: (payload: unknown) => void) => {
      testState.subscriptions.set(event, handler);
      return () => testState.subscriptions.delete(event);
    },
  }),
}));

vi.mock("@tanstack/react-query", () => ({
  useQueryClient: () => ({ invalidateQueries: testState.invalidateQueries }),
}));

describe("useAgentWorkflowEvents", () => {
  beforeEach(() => {
    testState.subscriptions.clear();
    testState.invalidateQueries.mockReset();
  });

  it("invalidates only the durable run identified by a workflow progress event", () => {
    const { unmount } = renderHook(() => useAgentWorkflowEvents());

    act(() => {
      testState.subscriptions.get("agent:workflow_progress")?.({
        runId: "run-1",
        emittedAt: "2026-07-16T00:00:00Z",
      });
    });

    expect(testState.invalidateQueries).toHaveBeenNthCalledWith(1, {
      queryKey: ["agent-workflow-progress", "run-1"],
    });
    expect(testState.invalidateQueries).toHaveBeenNthCalledWith(2, {
      queryKey: ["agent-workflow-latest-run"],
    });

    unmount();
    expect(testState.subscriptions.has("agent:workflow_progress")).toBe(false);
  });
});
