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

  it("uses workspace review utility model while inheriting the workspace provider", () => {
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
        }),
      { wrapper: wrapper() },
    );

    expect(result.current.normalizedActiveRuntime).toEqual({
      provider: "codex",
      modelId: "gpt-5.4-mini",
      effort: "medium",
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
        }),
      { wrapper: wrapper() },
    );

    expect(result.current.normalizedActiveRuntime).toEqual({
      provider: "claude",
      modelId: "sonnet",
      effort: "high",
    });
  });

  it("preserves selected Codex GPT-5.6 workspace runtime before alias-aware send checks", () => {
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
        }),
      { wrapper: wrapper() },
    );

    expect(result.current.normalizedActiveRuntime).toEqual({
      provider: "codex",
      modelId: "gpt-5.6-terra",
      effort: "ultra",
    });
  });
});
