import { renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { chatApi } from "@/api/chat";
import { useChatStore } from "@/stores/chatStore";
import { useQueuedMessagesHydration } from "./useQueuedMessagesHydration";

vi.mock("@/api/chat", () => ({
  chatApi: {
    getQueuedAgentMessages: vi.fn(),
  },
}));

describe("useQueuedMessagesHydration", () => {
  beforeEach(() => {
    vi.mocked(chatApi.getQueuedAgentMessages).mockReset();
    useChatStore.setState({
      messages: {},
      context: null,
      isLoading: false,
      activeConversationIds: {},
      activeAgentRunIds: {},
      queuedMessages: {},
      agentStatus: {},
      agentActivityLabels: {},
      isSending: {},
      isTeamActive: {},
      lastAgentEventTimestamp: {},
      toolCallStartTimes: {},
      lastToolCallCompletionTimestamp: {},
      toolCallCompletionTimestamps: {},
      effectiveModel: {},
    });
  });

  it("hydrates backend queued messages into the provided store key", async () => {
    vi.mocked(chatApi.getQueuedAgentMessages).mockResolvedValue([
      {
        id: "queued-1",
        content: "Continue this run",
        createdAt: "2026-06-19T10:00:00Z",
        isEditing: false,
        attachmentIds: ["att-1"],
      },
    ]);

    renderHook(() =>
      useQueuedMessagesHydration({
        contextType: "project",
        contextId: "conversation-1",
        storeContextKey: "project:conversation-1",
      })
    );

    await waitFor(() => {
      expect(useChatStore.getState().queuedMessages["project:conversation-1"]).toEqual([
        {
          id: "queued-1",
          content: "Continue this run",
          createdAt: "2026-06-19T10:00:00Z",
          isEditing: false,
          attachmentIds: ["att-1"],
        },
      ]);
    });
    expect(chatApi.getQueuedAgentMessages).toHaveBeenCalledWith(
      "project",
      "conversation-1"
    );
  });

  it("does not fetch when disabled", () => {
    renderHook(() =>
      useQueuedMessagesHydration({
        contextType: "project",
        contextId: "conversation-1",
        storeContextKey: "project:conversation-1",
        enabled: false,
      })
    );

    expect(chatApi.getQueuedAgentMessages).not.toHaveBeenCalled();
  });
});
