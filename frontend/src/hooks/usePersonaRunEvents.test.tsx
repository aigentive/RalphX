import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { chatKeys } from "./useChat";
import { usePersonaRunEvents } from "./usePersonaRunEvents";

const testState = vi.hoisted(() => ({
  subscriptions: new Map<string, Array<(payload: unknown) => void>>(),
  invalidateQueries: vi.fn(),
}));

function firePersonaEvent(event: string, payload: unknown) {
  for (const handler of testState.subscriptions.get(event) ?? []) {
    handler(payload);
  }
}

vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => ({
    subscribe: (event: string, handler: (payload: unknown) => void) => {
      const handlers = testState.subscriptions.get(event) ?? [];
      handlers.push(handler);
      testState.subscriptions.set(event, handlers);
      return () => undefined;
    },
  }),
}));

vi.mock("@tanstack/react-query", () => ({
  useQueryClient: () => ({ invalidateQueries: testState.invalidateQueries }),
}));

describe("usePersonaRunEvents", () => {
  beforeEach(() => {
    testState.subscriptions.clear();
    testState.invalidateQueries.mockClear();
  });

  it("invalidates the active conversation run for persona events", () => {
    renderHook(() => usePersonaRunEvents("conversation-1"));

    act(() => {
      firePersonaEvent("persona:applied", {
        conversation_id: "conversation-1",
        run_id: "run-1",
        persona_id: "persona-1",
        persona_slug: "design-voice",
        version: 2,
      });
      firePersonaEvent("persona:injection_skipped", {
        conversation_id: "conversation-1",
        run_id: "run-2",
        persona_id: "persona-2",
        persona_slug: "review-voice",
        version: 1,
        reason: "persona_not_injected",
      });
    });

    expect(testState.invalidateQueries).toHaveBeenNthCalledWith(1, {
      queryKey: chatKeys.agentRun("conversation-1"),
    });
    expect(testState.invalidateQueries).toHaveBeenNthCalledWith(2, {
      queryKey: chatKeys.agentRun("conversation-1"),
    });
  });

  it("ignores malformed persona events without a conversation id", () => {
    renderHook(() => usePersonaRunEvents("conversation-1"));

    act(() => {
      firePersonaEvent("persona:applied", { persona_id: "persona-1" });
      firePersonaEvent("persona:injection_skipped", {
        conversation_id: "conversation-2",
      });
    });

    expect(testState.invalidateQueries).not.toHaveBeenCalled();
  });
});
