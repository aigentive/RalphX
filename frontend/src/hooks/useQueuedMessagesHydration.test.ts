import { renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { chatApi } from "@/api/chat";
import { logger } from "@/lib/logger";
import { RemoteTransportError } from "@/lib/remote/transport-errors";
import {
  resetTransportEnvironmentId,
  setTransportEnvironmentId,
} from "@/lib/remote/active-environment";
import { useChatStore } from "@/stores/chatStore";
import {
  loadQueuedMessagesForHydration,
  useQueuedMessagesHydration,
} from "./useQueuedMessagesHydration";

vi.mock("@/api/chat", () => ({
  chatApi: {
    getQueuedAgentMessages: vi.fn(),
    listRemoteQueuedAgentMessages: vi.fn(),
  },
}));

vi.mock("@/lib/logger", () => ({
  logger: {
    debug: vi.fn(),
  },
}));

describe("useQueuedMessagesHydration", () => {
  beforeEach(() => {
    vi.mocked(chatApi.getQueuedAgentMessages).mockReset();
    vi.mocked(chatApi.listRemoteQueuedAgentMessages).mockReset();
    resetTransportEnvironmentId();
    vi.mocked(logger.debug).mockReset();
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
        composerSelectionSnapshot: {
          sourceType: "artifact",
          sourceKind: "plan",
          sourceId: "plan-v2",
          artifactVersion: 2,
          startLine: 10,
          endLine: 10,
          content: "Frozen plan line",
        },
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
          source: "backend",
          composerSelectionSnapshot: {
            sourceType: "artifact",
            sourceKind: "plan",
            sourceId: "plan-v2",
            artifactVersion: 2,
            startLine: 10,
            endLine: 10,
            content: "Frozen plan line",
          },
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

  it("hydrates a remote project conversation through the list twin", async () => {
    setTransportEnvironmentId("env-remote");
    vi.mocked(chatApi.listRemoteQueuedAgentMessages).mockResolvedValue([
      {
        id: "queued-remote",
        content: "Remote turn",
        createdAt: "2026-08-05T09:30:00Z",
        isEditing: false,
        attachmentIds: [],
      },
    ]);

    renderHook(() =>
      useQueuedMessagesHydration({
        contextType: "project",
        contextId: "conversation-remote",
        storeContextKey: "project:conversation-remote",
      }),
    );

    await waitFor(() => {
      expect(chatApi.listRemoteQueuedAgentMessages).toHaveBeenCalledWith(
        "conversation-remote",
      );
    });
    expect(chatApi.getQueuedAgentMessages).not.toHaveBeenCalled();
  });

  it("preserves a typed remote hydration failure instead of converting it to an empty list", async () => {
    setTransportEnvironmentId("env-remote");
    const error = new RemoteTransportError({
      code: "REMOTE_COMMAND_UNAVAILABLE",
      message: "host lacks the list twin",
      environmentId: "env-remote",
      cmd: "list_remote_queued_agent_messages",
    });
    vi.mocked(chatApi.listRemoteQueuedAgentMessages).mockRejectedValueOnce(error);

    await expect(
      loadQueuedMessagesForHydration("project", "conversation-remote"),
    ).rejects.toBe(error);
    expect(logger.debug).not.toHaveBeenCalled();
  });

  it("logs and leaves state unchanged when hydration fails", async () => {
    vi.mocked(chatApi.getQueuedAgentMessages).mockRejectedValue(new Error("offline"));

    renderHook(() =>
      useQueuedMessagesHydration({
        contextType: "project",
        contextId: "conversation-1",
        storeContextKey: "project:conversation-1",
      })
    );

    await waitFor(() => {
      expect(logger.debug).toHaveBeenCalledWith(
        "[chat] Failed to hydrate queued messages",
        expect.objectContaining({
          contextType: "project",
          contextId: "conversation-1",
          storeContextKey: "project:conversation-1",
        })
      );
    });
    expect(useChatStore.getState().queuedMessages["project:conversation-1"]).toBeUndefined();
  });
});
