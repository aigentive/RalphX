import { QueryClient } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  chatApi,
  type AgentConversationWorkspace,
} from "@/api/chat";

import { agentWorkspaceKeys } from "./agentWorkspaceQueries";
import { implementAgentPlanDirectly } from "./implementAgentPlanDirectly";

vi.mock("@/api/chat", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/chat")>();
  return {
    ...actual,
    chatApi: {
      ...actual.chatApi,
      switchAgentConversationMode: vi.fn(),
      sendAgentMessage: vi.fn(),
    },
  };
});

const switchAgentConversationModeMock = vi.mocked(
  chatApi.switchAgentConversationMode,
);
const sendAgentMessageMock = vi.mocked(chatApi.sendAgentMessage);

function workspace(
  overrides: Partial<AgentConversationWorkspace> = {},
): AgentConversationWorkspace {
  return {
    conversationId: "conversation-1",
    projectId: "project-1",
    mode: "plan",
    baseRefKind: "current_branch",
    baseRef: "main",
    baseDisplayName: "Current branch (main)",
    baseCommit: null,
    branchName: "ralphx/project/agent-conversation",
    worktreePath: "/tmp/agent-conversation",
    linkedIdeationSessionId: "session-1",
    linkedPlanBranchId: null,
    publicationPrNumber: null,
    publicationPrUrl: null,
    publicationPrStatus: null,
    publicationPushStatus: null,
    status: "active",
    createdAt: "2026-07-21T00:00:00.000Z",
    updatedAt: "2026-07-21T00:00:00.000Z",
    ...overrides,
  };
}

describe("implementAgentPlanDirectly", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    sendAgentMessageMock.mockResolvedValue({
      conversationId: "conversation-1",
      agentRunId: "run-1",
      isNewConversation: false,
      wasQueued: false,
      queuedAsPending: false,
      queuedMessageId: null,
    });
  });

  it("switches to edit, synchronizes readers, and preserves caller runtime fields", async () => {
    const queryClient = new QueryClient();
    const editWorkspace = workspace({ mode: "edit" });
    const onConversationModeSwitched = vi.fn();
    switchAgentConversationModeMock.mockResolvedValue({ workspace: editWorkspace });

    await implementAgentPlanDirectly({
      projectId: "project-1",
      workspace: workspace(),
      queryClient,
      onConversationModeSwitched,
      sendOptions: {
        providerHarness: "codex",
        modelId: "gpt-5.5",
        logicalEffort: "high",
        codexFastMode: true,
      },
    });

    expect(queryClient.getQueryData(agentWorkspaceKeys.workspace("conversation-1")))
      .toEqual(editWorkspace);
    expect(onConversationModeSwitched).toHaveBeenCalledWith(
      "conversation-1",
      "edit",
      editWorkspace,
    );
    expect(sendAgentMessageMock).toHaveBeenCalledWith(
      "project",
      "project-1",
      expect.stringContaining("Implement the approved plan directly"),
      undefined,
      undefined,
      {
        conversationId: "conversation-1",
        providerHarness: "codex",
        modelId: "gpt-5.5",
        logicalEffort: "high",
        codexFastMode: true,
        suppressUserMessage: true,
      },
    );
    expect(sendAgentMessageMock.mock.invocationCallOrder[0]).toBeGreaterThan(
      onConversationModeSwitched.mock.invocationCallOrder[0]!,
    );
  });

  it("synchronizes an already-edit workspace without another switch", async () => {
    const currentWorkspace = workspace({ mode: "edit" });
    const onConversationModeSwitched = vi.fn();

    await implementAgentPlanDirectly({
      projectId: "project-1",
      workspace: currentWorkspace,
      queryClient: new QueryClient(),
      onConversationModeSwitched,
    });

    expect(switchAgentConversationModeMock).not.toHaveBeenCalled();
    expect(onConversationModeSwitched).toHaveBeenCalledWith(
      "conversation-1",
      "edit",
      currentWorkspace,
    );
    expect(sendAgentMessageMock).toHaveBeenCalledTimes(1);
  });

  it("propagates a nullable switched workspace to the parent projection", async () => {
    const queryClient = new QueryClient();
    const onConversationModeSwitched = vi.fn();
    switchAgentConversationModeMock.mockResolvedValue({ workspace: null });

    await implementAgentPlanDirectly({
      projectId: "project-1",
      workspace: workspace(),
      queryClient,
      onConversationModeSwitched,
    });

    expect(
      queryClient.getQueryData(agentWorkspaceKeys.workspace("conversation-1")),
    ).toBeUndefined();
    expect(onConversationModeSwitched).toHaveBeenCalledWith(
      "conversation-1",
      "edit",
      null,
    );
    expect(sendAgentMessageMock).toHaveBeenCalledTimes(1);
  });

  it("does not project or send when the mode switch fails", async () => {
    const onConversationModeSwitched = vi.fn();
    const error = new Error("switch failed");
    switchAgentConversationModeMock.mockRejectedValue(error);

    await expect(
      implementAgentPlanDirectly({
        projectId: "project-1",
        workspace: workspace(),
        queryClient: new QueryClient(),
        onConversationModeSwitched,
      }),
    ).rejects.toBe(error);

    expect(onConversationModeSwitched).not.toHaveBeenCalled();
    expect(sendAgentMessageMock).not.toHaveBeenCalled();
  });

  it("keeps the confirmed edit projection when the send fails", async () => {
    const queryClient = new QueryClient();
    const editWorkspace = workspace({ mode: "edit" });
    const onConversationModeSwitched = vi.fn();
    const error = new Error("send failed");
    switchAgentConversationModeMock.mockResolvedValue({ workspace: editWorkspace });
    sendAgentMessageMock.mockRejectedValue(error);

    await expect(
      implementAgentPlanDirectly({
        projectId: "project-1",
        workspace: workspace(),
        queryClient,
        onConversationModeSwitched,
      }),
    ).rejects.toBe(error);

    expect(queryClient.getQueryData(agentWorkspaceKeys.workspace("conversation-1")))
      .toEqual(editWorkspace);
    expect(onConversationModeSwitched).toHaveBeenCalledWith(
      "conversation-1",
      "edit",
      editWorkspace,
    );
  });
});
