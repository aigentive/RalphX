import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render } from "@testing-library/react";
import type { ReactNode } from "react";

import type { AgentConversationWorkspace } from "@/api/chat";
import { TooltipProvider } from "@/components/ui/tooltip";
import type { AgentRuntimeSelection } from "@/stores/agentSessionStore";

import type { AgentConversation } from "./agentConversations";

export const agentRuntimeFixture: AgentRuntimeSelection = {
  provider: "codex",
  modelId: "gpt-5.5",
  effort: "xhigh",
};

export const agentProjectFixture = {
  id: "project-1",
  name: "ralphx",
  workingDirectory: "/tmp/ralphx",
  gitMode: "worktree" as const,
  baseBranch: null,
  worktreeParentDirectory: null,
  useFeatureBranches: true,
  mergeValidationMode: "block" as const,
  detectedAnalysis: null,
  customAnalysis: null,
  analyzedAt: null,
  githubPrEnabled: false,
  createdAt: "2026-04-23T09:00:00Z",
  updatedAt: "2026-04-23T09:00:00Z",
};

export const conversationFixture = (
  overrides: Partial<AgentConversation> = {}
): AgentConversation => ({
  id: "conversation-1",
  contextType: "project",
  contextId: "project-1",
  claudeSessionId: null,
  providerSessionId: "thread-1",
  providerHarness: "codex",
  upstreamProvider: null,
  providerProfile: null,
  title: "Untitled agent",
  messageCount: 1,
  lastMessageAt: "2026-04-23T09:00:00Z",
  createdAt: "2026-04-23T09:00:00Z",
  updatedAt: "2026-04-23T09:00:00Z",
  archivedAt: null,
  projectId: "project-1",
  ideationSessionId: null,
  ...overrides,
});

export const conversationWorkspaceFixture = (
  overrides: Partial<AgentConversationWorkspace> = {}
): AgentConversationWorkspace => ({
  conversationId: "conversation-1",
  projectId: "project-1",
  mode: "edit",
  baseRefKind: "project_default",
  baseRef: "main",
  baseDisplayName: "Project default (main)",
  baseCommit: null,
  branchName: "ralphx/ralphx/agent-abcdef12",
  worktreePath: "/tmp/ralphx/conversation-1",
  linkedIdeationSessionId: null,
  linkedPlanBranchId: null,
  publicationPrNumber: null,
  publicationPrUrl: null,
  publicationPrStatus: null,
  publicationPushStatus: null,
  status: "active",
  createdAt: "2026-04-23T09:00:00Z",
  updatedAt: "2026-04-23T09:00:00Z",
  ...overrides,
});

export function renderWithAgentProviders(ui: ReactNode) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  });

  return {
    ...render(
      <QueryClientProvider client={queryClient}>
        <TooltipProvider>{ui}</TooltipProvider>
      </QueryClientProvider>
    ),
    queryClient,
  };
}
