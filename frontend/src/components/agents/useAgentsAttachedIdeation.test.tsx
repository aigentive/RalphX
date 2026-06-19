import { QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { AgentConversationWorkspace } from "@/api/chat";
import { createTestQueryClient } from "@/test/store-utils";
import type { AgentConversation } from "./agentConversations";
import { useAgentsAttachedIdeation } from "./useAgentsAttachedIdeation";

const { getIdeationSessionMock, useConversationHistoryWindowMock } = vi.hoisted(() => ({
  getIdeationSessionMock: vi.fn(),
  useConversationHistoryWindowMock: vi.fn(),
}));

vi.mock("@/api/ideation", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/ideation")>();
  return {
    ...actual,
    ideationApi: {
      ...actual.ideationApi,
      sessions: {
        ...actual.ideationApi.sessions,
        get: (...args: unknown[]) => getIdeationSessionMock(...args),
      },
    },
  };
});

vi.mock("@/hooks/useChat", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/hooks/useChat")>();
  return {
    ...actual,
    useConversationHistoryWindow: (...args: unknown[]) =>
      useConversationHistoryWindowMock(...args),
  };
});

const conversation = (): AgentConversation =>
  ({
    id: "conversation-1",
    projectId: "project-1",
    contextType: "project",
    contextId: "project-1",
    title: "ClickUp integration",
    archivedAt: null,
  }) as AgentConversation;

const workspace = (): AgentConversationWorkspace =>
  ({
    conversationId: "conversation-1",
    projectId: "project-1",
    mode: "ideation",
    linkedIdeationSessionId: "stale-shell-session",
    linkedPlanBranchId: null,
  }) as AgentConversationWorkspace;

function wrapper({ children }: { children: ReactNode }) {
  return (
    <QueryClientProvider client={createTestQueryClient()}>
      {children}
    </QueryClientProvider>
  );
}

describe("useAgentsAttachedIdeation", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useConversationHistoryWindowMock.mockReturnValue({
      data: {
        conversation: conversation(),
        messages: [
          {
            id: "message-1",
            conversationId: "conversation-1",
            role: "assistant",
            content: "",
            toolCalls: [
              {
                id: "tool-1",
                name: "ralphx::v1_list_ideation_sessions",
                arguments: { project_id: "project-1" },
                result: {
                  sessions: [
                    {
                      id: "stale-shell-session",
                      title: "Continue ClickUp integration implementation",
                      status: "active",
                      proposal_count: 0,
                    },
                    {
                      id: "productive-session",
                      title: "Implement ClickUp integration",
                      status: "accepted",
                      proposal_count: 4,
                      plan_artifact_id: "artifact-1",
                    },
                  ],
                },
              },
            ],
            contentBlocks: [],
            createdAt: "2026-06-19T09:00:00Z",
          },
        ],
      },
    });
    getIdeationSessionMock.mockResolvedValue({
      id: "productive-session",
      projectId: "project-1",
      title: "Implement ClickUp integration",
      status: "accepted",
      planArtifactId: "artifact-1",
      inheritedPlanArtifactId: null,
      acceptanceStatus: "accepted",
      convertedAt: "2026-06-19T09:10:00Z",
      verificationStatus: "unverified",
      verificationInProgress: false,
      gapScore: null,
      archivedAt: null,
    });
  });

  it("uses recent history to expose productive ideation artifacts when the workspace link is stale", async () => {
    const { result } = renderHook(
      () =>
        useAgentsAttachedIdeation({
          activeConversation: conversation(),
          activeConversationMode: "ideation",
          activeWorkspace: workspace(),
          invalidateProjectConversations: vi.fn(),
          selectedConversationMessages: [],
        }),
      { wrapper },
    );

    await waitFor(() =>
      expect(getIdeationSessionMock).toHaveBeenCalledWith("productive-session"),
    );
    await waitFor(() =>
      expect(result.current.availableArtifactTabs).toEqual([
        "plan",
        "verification",
        "proposal",
        "tasks",
      ]),
    );
    expect(result.current.attachedIdeationSessionId).toBe("productive-session");
    expect(result.current.hasAutoOpenArtifacts).toBe(true);
  });
});
