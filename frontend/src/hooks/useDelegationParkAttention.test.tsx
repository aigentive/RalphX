import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ReactNode } from "react";

const { subscribers, toastError } = vi.hoisted(() => ({
  subscribers: new Map<string, (payload: unknown) => void>(),
  toastError: vi.fn(),
}));

vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => ({
    subscribe: (event: string, callback: (payload: unknown) => void) => {
      subscribers.set(event, callback);
      return vi.fn();
    },
  }),
}));
vi.mock("sonner", () => ({ toast: { error: toastError } }));

import { useDelegationParkAttention } from "./useDelegationParkAttention";

let queryClient: QueryClient;

function wrapper({ children }: { children: ReactNode }) {
  return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
}

function render() {
  queryClient = new QueryClient();
  return renderHook(() => useDelegationParkAttention(), { wrapper });
}

describe("useDelegationParkAttention", () => {
  beforeEach(() => {
    subscribers.clear();
    toastError.mockReset();
    render();
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it("names the coordinator that will never resume", () => {
    subscribers.get("delegation_park:needs_attention")?.({
      park_id: "park-1",
      parent_conversation_id: "conversation-1",
      conversation_title: "Refactor merge engine",
      context_type: "project",
      context_id: "project-1",
      delegate_count: 3,
      error: "chat service unavailable",
    });

    expect(toastError).toHaveBeenCalledTimes(1);
    expect(toastError).toHaveBeenCalledWith(
      "Delegates finished but “Refactor merge engine” could not be resumed",
      expect.objectContaining({
        description:
          "3 delegates settled. Send a message in that conversation to continue. (chat service unavailable)",
      }),
    );
  });

  it("falls back to the conversation id when the title is unavailable", () => {
    subscribers.get("delegation_park:needs_attention")?.({
      park_id: "park-1",
      parent_conversation_id: "conversation-1",
      conversation_title: null,
      context_type: null,
      context_id: null,
      delegate_count: 1,
      error: "parent conversation not found",
    });

    expect(toastError).toHaveBeenCalledWith(
      "Delegates finished but conversation-1 could not be resumed",
      expect.objectContaining({
        description:
          "1 delegate settled. Send a message in that conversation to continue. (parent conversation not found)",
      }),
    );
  });

  it("refreshes the agents sidebar so the row leaves the parked lane", () => {
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");

    subscribers.get("delegation_park:needs_attention")?.({
      park_id: "park-1",
      parent_conversation_id: "conversation-1",
      delegate_count: 1,
      error: "boom",
    });

    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ["agents", "sidebar-conversations"],
    });
  });

  it("alerts once per park so a retried dispatcher cannot spam the user", () => {
    const payload = {
      park_id: "park-1",
      parent_conversation_id: "conversation-1",
      delegate_count: 1,
      error: "boom",
    };

    subscribers.get("delegation_park:needs_attention")?.(payload);
    subscribers.get("delegation_park:needs_attention")?.(payload);

    expect(toastError).toHaveBeenCalledTimes(1);
  });

  it("ignores a payload without a park id", () => {
    subscribers.get("delegation_park:needs_attention")?.({ error: "boom" });

    expect(toastError).not.toHaveBeenCalled();
  });
});
