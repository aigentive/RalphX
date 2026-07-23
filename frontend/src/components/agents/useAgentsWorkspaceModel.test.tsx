import { renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AGENT_MODEL_CATALOG } from "@/lib/agent-models";

import type { AgentConversation } from "./agentConversations";
import { useAgentsWorkspaceModel } from "./useAgentsWorkspaceModel";

const {
  getAgentConversationWorkspaceMock,
  getAgentConversationWorkspaceFreshnessMock,
} = vi.hoisted(() => ({
  getAgentConversationWorkspaceMock: vi.fn(),
  getAgentConversationWorkspaceFreshnessMock: vi.fn(),
}));

vi.mock("@/api/chat", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/chat")>();
  return {
    ...actual,
    chatApi: {
      ...actual.chatApi,
      getAgentConversationWorkspace: getAgentConversationWorkspaceMock,
      getAgentConversationWorkspaceFreshness:
        getAgentConversationWorkspaceFreshnessMock,
    },
  };
});

function projectConversation(
  overrides: Partial<AgentConversation> = {},
): AgentConversation {
  return {
    id: "conversation-1",
    contextType: "project",
    contextId: "project-1",
    projectId: "project-1",
    ideationSessionId: null,
    claudeSessionId: null,
    providerSessionId: null,
    providerHarness: null,
    upstreamProvider: null,
    providerProfile: null,
    logicalModel: null,
    effectiveModelId: null,
    logicalEffort: null,
    effectiveEffort: null,
    agentMode: "chat",
    parentConversationId: null,
    title: "Conversation",
    messageCount: 0,
    lastMessageAt: null,
    createdAt: "2026-05-22T00:00:00.000Z",
    updatedAt: "2026-05-22T00:00:00.000Z",
    archivedAt: null,
    ...overrides,
  };
}

function wrapper() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return function HookWrapper({ children }: { children: ReactNode }) {
    return (
      <QueryClientProvider client={queryClient}>
        {children}
      </QueryClientProvider>
    );
  };
}

describe("useAgentsWorkspaceModel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getAgentConversationWorkspaceMock.mockResolvedValue(null);
    getAgentConversationWorkspaceFreshnessMock.mockResolvedValue(null);
  });

  it("uses the backend-resolved Reviewer role runtime before the child runtime loads", () => {
    const { result } = renderHook(
      () =>
        useAgentsWorkspaceModel({
          activeConversation: projectConversation({
            providerHarness: "codex",
            logicalModel: "gpt-5.5",
            logicalEffort: "xhigh",
          }),
          focusedWorkspaceReviewConversationId: "review-conversation-1",
          modelRegistry: AGENT_MODEL_CATALOG,
          optimisticWorkspacesByConversationId: {},
          runtimeByConversationId: {},
          selectedConversationId: "conversation-1",
          workspaceReviewerRuntime: {
            provider: "codex",
            modelId: "gpt-5.6-terra",
            effort: "ultra",
          },
        }),
      { wrapper: wrapper() },
    );

    expect(result.current.normalizedActiveRuntime).toEqual({
      provider: "codex",
      modelId: "gpt-5.6-terra",
      effort: "max",
    });
  });

  it("keeps explicit runtime overrides scoped to the focused review conversation", () => {
    const { result } = renderHook(
      () =>
        useAgentsWorkspaceModel({
          activeConversation: projectConversation({
            providerHarness: "codex",
            logicalModel: "gpt-5.5",
            logicalEffort: "xhigh",
          }),
          focusedWorkspaceReviewConversationId: "review-conversation-1",
          modelRegistry: AGENT_MODEL_CATALOG,
          optimisticWorkspacesByConversationId: {},
          runtimeByConversationId: {
            "review-conversation-1": {
              provider: "claude",
              modelId: "sonnet",
              effort: "high",
            },
          },
          selectedConversationId: "conversation-1",
          workspaceReviewerRuntime: {
            provider: "codex",
            modelId: "gpt-5.6-terra",
            effort: "ultra",
          },
        }),
      { wrapper: wrapper() },
    );

    expect(result.current.normalizedActiveRuntime).toEqual({
      provider: "claude",
      modelId: "sonnet",
      effort: "high",
    });
  });

  it("uses the committed review focus hint before child summary hydration", () => {
    const { result } = renderHook(
      () =>
        useAgentsWorkspaceModel({
          activeConversation: projectConversation(),
          focusedWorkspaceReviewConversationId: "review-conversation-1",
          focusedWorkspaceReviewRuntimeHint: {
            provider: "codex",
            modelId: "gpt-5.6-terra",
            effort: "high",
          },
          modelRegistry: AGENT_MODEL_CATALOG,
          optimisticWorkspacesByConversationId: {},
          runtimeByConversationId: {},
          selectedConversationId: "conversation-1",
          workspaceReviewerRuntime: {
            provider: "codex",
            modelId: "gpt-5.6-terra",
            effort: "medium",
          },
        }),
      { wrapper: wrapper() },
    );

    expect(result.current.normalizedActiveRuntime).toEqual({
      provider: "codex",
      modelId: "gpt-5.6-terra",
      effort: "high",
    });
  });

  it("uses directly hydrated child metadata ahead of stale client runtime and the next-launch Reviewer default", () => {
    const { result } = renderHook(
      () =>
        useAgentsWorkspaceModel({
          activeConversation: projectConversation(),
          focusedWorkspaceReviewConversation: projectConversation({
            id: "review-conversation-1",
            parentConversationId: "conversation-1",
            providerHarness: "codex",
            logicalModel: "gpt-5.5",
            logicalEffort: "high",
            serviceTier: "fast",
          }),
          focusedWorkspaceReviewConversationId: "review-conversation-1",
          focusedWorkspaceReviewRuntimeHint: {
            provider: "codex",
            modelId: "gpt-5.6-terra",
            effort: "medium",
          },
          modelRegistry: AGENT_MODEL_CATALOG,
          optimisticWorkspacesByConversationId: {},
          runtimeByConversationId: {
            "review-conversation-1": {
              provider: "claude",
              modelId: "sonnet",
              effort: "medium",
            },
          },
          selectedConversationId: "conversation-1",
          workspaceReviewerRuntime: {
            provider: "codex",
            modelId: "gpt-5.6-terra",
            effort: "max",
          },
        }),
      { wrapper: wrapper() },
    );

    expect(result.current.normalizedActiveRuntime).toEqual({
      provider: "codex",
      modelId: "gpt-5.5",
      effort: "high",
    });
    expect(result.current.focusedWorkspaceReviewServiceTier).toBe("fast");
  });

  it("normalizes remembered Codex Ultra effort before alias-aware send checks", () => {
    const { result } = renderHook(
      () =>
        useAgentsWorkspaceModel({
          activeConversation: projectConversation({
            providerHarness: "codex",
            logicalModel: "gpt-5.5",
            logicalEffort: "xhigh",
          }),
          modelRegistry: AGENT_MODEL_CATALOG,
          optimisticWorkspacesByConversationId: {},
          runtimeByConversationId: {
            "conversation-1": {
              provider: "codex",
              modelId: "gpt-5.6-terra",
              effort: "ultra",
            },
          },
          selectedConversationId: "conversation-1",
          workspaceReviewerRuntime: null,
        }),
      { wrapper: wrapper() },
    );

    expect(result.current.normalizedActiveRuntime).toEqual({
      provider: "codex",
      modelId: "gpt-5.6-terra",
      effort: "max",
    });
  });
});
