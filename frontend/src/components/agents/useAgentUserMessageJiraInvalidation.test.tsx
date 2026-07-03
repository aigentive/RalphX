import { QueryClient } from "@tanstack/react-query";
import { renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { agentJiraIssueKeys } from "./agentJiraIssueQueries";
import { useAgentUserMessageJiraInvalidation } from "./useAgentUserMessageJiraInvalidation";

describe("useAgentUserMessageJiraInvalidation", () => {
  it("invalidates the resolved conversation Jira issue query after a user message", () => {
    const queryClient = new QueryClient();
    const invalidateQueriesSpy = vi.spyOn(queryClient, "invalidateQueries");

    const { result } = renderHook(() =>
      useAgentUserMessageJiraInvalidation({
        queryClient,
        selectedConversationId: null,
      })
    );

    result.current({
      content: "reference jira",
      result: { conversationId: "conversation-1" },
    });

    expect(invalidateQueriesSpy).toHaveBeenCalledWith({
      queryKey: agentJiraIssueKeys.issue("conversation-1"),
    });
  });

  it("falls back to the selected conversation when the send result is empty", () => {
    const queryClient = new QueryClient();
    const invalidateQueriesSpy = vi.spyOn(queryClient, "invalidateQueries");

    const { result } = renderHook(() =>
      useAgentUserMessageJiraInvalidation({
        queryClient,
        selectedConversationId: "selected-conversation",
      })
    );

    result.current({
      content: "reference jira",
      result: { conversationId: "" },
    });

    expect(invalidateQueriesSpy).toHaveBeenCalledWith({
      queryKey: agentJiraIssueKeys.issue("selected-conversation"),
    });
  });
});
