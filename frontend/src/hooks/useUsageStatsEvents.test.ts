import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const subscriptions = new Map<string, ((payload: unknown) => void)[]>();
const invalidateQueries = vi.fn();

vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => ({
    subscribe: (event: string, handler: (payload: unknown) => void) => {
      const handlers = subscriptions.get(event) ?? [];
      handlers.push(handler);
      subscriptions.set(event, handlers);
      return () => {
        subscriptions.set(event, (subscriptions.get(event) ?? []).filter((item) => item !== handler));
      };
    },
  }),
}));

vi.mock("@tanstack/react-query", () => ({
  useQueryClient: () => ({ invalidateQueries }),
}));

import { useUsageStatsEvents } from "./useUsageStatsEvents";

function fireUsageUpdated(payload: {
  conversation_id: string;
  context_id?: string;
  context_type?: string;
}) {
  act(() => {
    for (const handler of subscriptions.get("agent:usage_updated") ?? []) {
      handler(payload);
    }
  });
}

describe("useUsageStatsEvents", () => {
  beforeEach(() => {
    subscriptions.clear();
    invalidateQueries.mockClear();
  });

  it("invalidates a background conversation and its exact project scope", () => {
    renderHook(() => useUsageStatsEvents());

    fireUsageUpdated({
      conversation_id: "background-conversation",
      context_type: "project",
      context_id: "project-1",
    });

    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: ["chat", "conversation-stats", "background-conversation"],
    });
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: ["project-chat-usage-stats", "project-1"],
    });
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: ["insights-chat-usage-stats"],
    });
  });

  it("invalidates task, project-family, and insights scope caches", () => {
    renderHook(() => useUsageStatsEvents());

    fireUsageUpdated({
      conversation_id: "task-conversation",
      context_type: "review",
      context_id: "task-1",
    });

    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: ["task-chat-usage-stats", "task-1"],
    });
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: ["project-chat-usage-stats"],
    });
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: ["insights-chat-usage-stats"],
    });
  });

  it("invalidates all project totals for ideation usage", () => {
    renderHook(() => useUsageStatsEvents());

    fireUsageUpdated({
      conversation_id: "ideation-conversation",
      context_type: "ideation",
      context_id: "ideation-session-1",
    });

    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: ["project-chat-usage-stats"],
    });
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: ["insights-chat-usage-stats"],
    });
  });
});
