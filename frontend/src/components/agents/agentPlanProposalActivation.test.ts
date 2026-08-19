import { QueryClient } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { chatApi, type AgentConversationWorkspace } from "@/api/chat";

import {
  activateAgentPlanProposals,
  PlanContinuationCommittedError,
} from "./agentPlanProposalActivation";

vi.mock("@/api/chat", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/chat")>();
  return {
    ...actual,
    chatApi: {
      ...actual.chatApi,
      activateAgentTaskPipeline: vi.fn(),
      getAgentConversationWorkspace: vi.fn(),
      sendAgentMessage: vi.fn(),
    },
  };
});

const planWorkspace = {
  conversationId: "conversation-1",
  projectId: "project-1",
  mode: "plan",
  linkedIdeationSessionId: "session-1",
  taskPipelineSessionId: null,
} as AgentConversationWorkspace;
const tasksWorkspace = {
  ...planWorkspace,
  mode: "tasks",
  taskPipelineSessionId: "session-1",
} as AgentConversationWorkspace;
const runtime = {
  provider: "claude",
  model: "sonnet",
  effort: "high",
  serviceTier: "provider_default" as const,
  coordinationMode: "solo" as const,
  personaId: null,
};

describe("activateAgentPlanProposals", () => {
  beforeEach(() => {
    vi.mocked(chatApi.activateAgentTaskPipeline).mockResolvedValue(tasksWorkspace);
    vi.mocked(chatApi.getAgentConversationWorkspace).mockResolvedValue(tasksWorkspace);
    vi.mocked(chatApi.sendAgentMessage).mockReset();
  });

  it("does not replay the stale Plan workspace projection after a committed activation", async () => {
    const queryClient = new QueryClient();
    const onConversationModeSwitched = vi.fn();
    let activationCompleted = false;
    vi.mocked(chatApi.sendAgentMessage)
      .mockRejectedValueOnce(new Error("send failed"))
      .mockResolvedValueOnce({
        conversationId: "ideation-conversation-1",
        agentRunId: "run-1",
        isNewConversation: false,
        wasQueued: false,
        queuedMessageId: null,
        queuedAsPending: false,
      });

    await expect(
      activateAgentPlanProposals({
        sessionId: "session-1",
        workspace: planWorkspace,
        queryClient,
        canPromoteWorkspace: true,
        onConversationModeSwitched,
        runtimeOverride: runtime,
        onWorkspaceActivated: () => { activationCompleted = true; },
      }),
    ).rejects.toBeInstanceOf(PlanContinuationCommittedError);
    expect(activationCompleted).toBe(true);
    onConversationModeSwitched.mockClear();

    await activateAgentPlanProposals({
      sessionId: "session-1",
      workspace: planWorkspace,
      queryClient,
      canPromoteWorkspace: true,
      onConversationModeSwitched,
      runtimeOverride: runtime,
      workspaceActivationCompleted: activationCompleted,
    });

    expect(chatApi.activateAgentTaskPipeline).toHaveBeenCalledTimes(1);
    expect(onConversationModeSwitched).not.toHaveBeenCalled();
  });

  it("appends the message of an Error rejection to the retry dialog", async () => {
    vi.mocked(chatApi.sendAgentMessage).mockRejectedValue(new Error("send failed"));

    await expect(
      activateAgentPlanProposals({
        sessionId: "session-1",
        workspace: planWorkspace,
        queryClient: new QueryClient(),
        canPromoteWorkspace: true,
        runtimeOverride: runtime,
      }),
    ).rejects.toThrow(/it will not activate Tasks again\. send failed$/);
  });

  it("surfaces plain string rejections from the backend", async () => {
    // Tauri rejects with plain strings, which an `instanceof Error` check silently drops.
    vi.mocked(chatApi.sendAgentMessage).mockRejectedValue(
      "Failed to resolve manual default for workspace_edit: A complete role runtime override cannot be mixed with legacy provider or model overrides",
    );

    await expect(
      activateAgentPlanProposals({
        sessionId: "session-1",
        workspace: planWorkspace,
        queryClient: new QueryClient(),
        canPromoteWorkspace: true,
        runtimeOverride: runtime,
      }),
    ).rejects.toThrow(
      /A complete role runtime override cannot be mixed with legacy provider or model overrides$/,
    );
  });
});
