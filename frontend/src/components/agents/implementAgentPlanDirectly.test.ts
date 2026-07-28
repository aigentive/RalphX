import { QueryClient } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  chatApi,
  type AgentConversationWorkspace,
} from "@/api/chat";

import { agentWorkspaceKeys } from "./agentWorkspaceQueries";
import {
  implementAgentPlanDirectly,
  type DirectImplementationActivationSnapshot,
} from "./implementAgentPlanDirectly";
import { PlanContinuationCommittedError } from "./agentPlanProposalActivation";

vi.mock("@/api/chat", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/chat")>();
  return {
    ...actual,
    chatApi: {
      ...actual.chatApi,
      activateAgentPlanDirectImplementation: vi.fn(),
      sendAgentMessage: vi.fn(),
    },
  };
});

const activateAgentPlanDirectImplementationMock = vi.mocked(
  chatApi.activateAgentPlanDirectImplementation,
);
const sendAgentMessageMock = vi.mocked(chatApi.sendAgentMessage);

const approvedArtifactReferences = [
  {
    artifactId: "overview-1",
    kind: "plan",
    title: "Plan Overview",
    sessionId: "session-1",
    version: 3,
    status: "approved",
  },
  {
    artifactId: "blueprint-1",
    kind: "plan_blueprint",
    title: "Implementation Blueprint",
    sessionId: "session-1",
    version: 2,
    status: "approved",
  },
];

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
    const onActivated = vi.fn();
    activateAgentPlanDirectImplementationMock.mockResolvedValue({
      workspace: editWorkspace,
      artifactReferences: approvedArtifactReferences,
    });

    await implementAgentPlanDirectly({
      projectId: "project-1",
      workspace: workspace(),
      queryClient,
      onConversationModeSwitched,
      onActivated,
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
    expect(onActivated).toHaveBeenCalledWith({
      workspace: editWorkspace,
      artifactReferences: approvedArtifactReferences,
    });
    expect(sendAgentMessageMock).toHaveBeenCalledWith(
      "project",
      "project-1",
      expect.stringContaining("Implement the approved plan directly"),
      undefined,
      {
        conversationId: "conversation-1",
        providerHarness: "codex",
        modelId: "gpt-5.5",
        logicalEffort: "high",
        codexFastMode: true,
        requireApprovedLinkedPlan: true,
        suppressUserMessage: true,
      },
    );
    expect(sendAgentMessageMock.mock.invocationCallOrder[0]).toBeGreaterThan(
      onActivated.mock.invocationCallOrder[0]!,
    );
  });

  it("revalidates a retry and resends only the first pinned pair", async () => {
    const currentWorkspace = workspace({ mode: "edit" });
    const onConversationModeSwitched = vi.fn();
    const pinnedActivation: DirectImplementationActivationSnapshot = {
      workspace: currentWorkspace,
      artifactReferences: approvedArtifactReferences,
    };
    activateAgentPlanDirectImplementationMock.mockResolvedValue({
      workspace: currentWorkspace,
      artifactReferences: approvedArtifactReferences.map((reference) => ({
        ...reference,
        title: `Revalidated ${reference.title}`,
      })),
    });

    await implementAgentPlanDirectly({
      projectId: "project-1",
      workspace: workspace(),
      queryClient: new QueryClient(),
      onConversationModeSwitched,
      pinnedActivation,
    });

    expect(activateAgentPlanDirectImplementationMock).toHaveBeenCalledWith({
      conversationId: "conversation-1",
      sessionId: "session-1",
      retry: true,
    });
    expect(onConversationModeSwitched).toHaveBeenCalledWith(
      "conversation-1",
      "edit",
      currentWorkspace,
    );
    expect(sendAgentMessageMock).toHaveBeenCalledWith(
      "project",
      "project-1",
      expect.stringContaining("Implement the approved plan directly"),
      undefined,
      expect.objectContaining({
        conversationId: "conversation-1",
        requireApprovedLinkedPlan: true,
        suppressUserMessage: true,
      }),
    );
    expect(
      sendAgentMessageMock.mock.calls[0]?.[4],
    ).not.toHaveProperty("composerArtifactReferences");
  });

  it("rejects retry activation when the approved artifact tuple changed", async () => {
    const currentWorkspace = workspace({ mode: "edit" });
    const pinnedActivation: DirectImplementationActivationSnapshot = {
      workspace: currentWorkspace,
      artifactReferences: approvedArtifactReferences,
    };
    activateAgentPlanDirectImplementationMock.mockResolvedValue({
      workspace: currentWorkspace,
      artifactReferences: [
        approvedArtifactReferences[0]!,
        {
          ...approvedArtifactReferences[1]!,
          artifactId: "blueprint-2",
          version: 3,
        },
      ],
    });

    await expect(
      implementAgentPlanDirectly({
        projectId: "project-1",
        workspace: workspace(),
        queryClient: new QueryClient(),
        pinnedActivation,
      }),
    ).rejects.toThrow("plan changed");

    expect(activateAgentPlanDirectImplementationMock).toHaveBeenCalledWith({
      conversationId: "conversation-1",
      sessionId: "session-1",
      retry: true,
    });
    expect(sendAgentMessageMock).not.toHaveBeenCalled();
  });

  it("does not project or send when activation fails", async () => {
    const onConversationModeSwitched = vi.fn();
    const error = new Error("activation failed");
    activateAgentPlanDirectImplementationMock.mockRejectedValue(error);

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
    const onActivated = vi.fn();
    const error = new Error("send failed");
    activateAgentPlanDirectImplementationMock.mockResolvedValue({
      workspace: editWorkspace,
      artifactReferences: approvedArtifactReferences,
    });
    sendAgentMessageMock.mockRejectedValue(error);

    await expect(
      implementAgentPlanDirectly({
        projectId: "project-1",
        workspace: workspace(),
        queryClient,
        onConversationModeSwitched,
        onActivated,
      }),
    ).rejects.toBeInstanceOf(PlanContinuationCommittedError);

    expect(queryClient.getQueryData(agentWorkspaceKeys.workspace("conversation-1")))
      .toEqual(editWorkspace);
    expect(onConversationModeSwitched).toHaveBeenCalledWith(
      "conversation-1",
      "edit",
      editWorkspace,
    );
    expect(onActivated).toHaveBeenCalledWith({
      workspace: editWorkspace,
      artifactReferences: approvedArtifactReferences,
    });
  });
});
