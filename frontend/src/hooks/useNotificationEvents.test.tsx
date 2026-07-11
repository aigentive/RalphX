import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ReactNode } from "react";

import { useNotificationEvents } from "./useNotificationEvents";
import { attentionKeys } from "./useAttentionItems";
import { notificationKeys } from "./useNotificationHistory";

const subscribers = new Map<string, () => void>();
vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => ({ subscribe: (event: string, callback: () => void) => { subscribers.set(event, callback); return vi.fn(); } }),
}));

describe("useNotificationEvents", () => {
  afterEach(() => { subscribers.clear(); vi.restoreAllMocks(); });

  it("invalidates attention for every emitted Tier 1 source event", () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const wrapper = ({ children }: { children: ReactNode }) => <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
    renderHook(() => useNotificationEvents(), { wrapper });

    expect([...subscribers.keys()]).toEqual([
      "review:update", "task:status_changed", "permission:request", "permission:expired",
      "permission:resolved",
      "agent:ask_user_question", "agent:question_resolved", "automation:updated",
      "automation:run:updated", "plan_artifact:created", "plan_artifact:approved",
      "pr_review_artifact:created", "pr_review_artifact:updated",
      "notification:created", "notification:updated",
    ]);
    subscribers.get("permission:request")?.();
    expect(invalidate).toHaveBeenCalledWith({ queryKey: attentionKeys.all });

    subscribers.get("permission:resolved")?.();
    expect(invalidate).toHaveBeenCalledWith({ queryKey: attentionKeys.all });

    subscribers.get("notification:created")?.();
    expect(invalidate).toHaveBeenCalledWith({ queryKey: notificationKeys.all });
  });
});
