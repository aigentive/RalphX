import { QueryClient } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { attentionKeys } from "@/hooks/useAttentionItems";
import { notificationKeys } from "@/hooks/useNotificationHistory";

import { useAgentConversationInvalidation } from "./useAgentConversationInvalidation";

describe("useAgentConversationInvalidation", () => {
  it("refreshes attention and notification counts after conversation state changes", async () => {
    const queryClient = new QueryClient();
    const invalidateQueries = vi
      .spyOn(queryClient, "invalidateQueries")
      .mockResolvedValue(undefined);
    const { result } = renderHook(() =>
      useAgentConversationInvalidation(queryClient),
    );

    await act(async () => result.current("project-1"));

    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: attentionKeys.all,
    });
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: notificationKeys.all,
    });
  });
});
