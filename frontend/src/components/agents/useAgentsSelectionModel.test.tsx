import { renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useAgentsSelectionModel } from "./useAgentsSelectionModel";
import type { AgentConversation } from "./agentConversations";

const { conversationSummaryMock, projectConversationsMock } = vi.hoisted(() => ({
  conversationSummaryMock: vi.fn(),
  projectConversationsMock: vi.fn(),
}));

vi.mock("@/hooks/useChat", () => ({
  useConversationSummary: conversationSummaryMock,
}));

vi.mock("./useProjectAgentConversations", () => ({
  useProjectAgentConversations: projectConversationsMock,
}));

function standaloneConversation(): AgentConversation {
  return {
    id: "standalone-1",
    contextType: "standalone",
    contextId: "standalone-1",
    projectId: null,
    ideationSessionId: null,
    providerSessionId: null,
    providerHarness: null,
    coordinationMode: "solo",
    title: "Private exploration",
    messageCount: 1,
    lastMessageAt: "2026-07-17T10:00:00.000Z",
    createdAt: "2026-07-17T10:00:00.000Z",
    updatedAt: "2026-07-17T10:00:00.000Z",
    archivedAt: null,
  };
}

describe("useAgentsSelectionModel", () => {
  beforeEach(() => {
    projectConversationsMock.mockReturnValue({
      data: [],
      isLoading: false,
    });
  });

  it("keeps a standalone selection active and clears the panel project even when another project is focused", () => {
    conversationSummaryMock.mockReturnValue({
      data: standaloneConversation(),
      isLoading: false,
    });
    const clearSelection = vi.fn();

    const { result } = renderHook(() =>
      useAgentsSelectionModel({
        clearAgentConversationSelection: clearSelection,
        focusedProjectId: "project-1",
        optimisticConversationsById: {},
        optimisticSelectedConversationId: null,
        projectId: "project-1",
        projects: [],
        selectedProjectId: null,
        showArchived: false,
        storedSelectedConversationId: "standalone-1",
      }),
    );

    expect(result.current.activeConversation).toEqual(
      expect.objectContaining({ id: "standalone-1", projectId: null }),
    );
    expect(result.current.activeProjectId).toBeNull();
    expect(clearSelection).not.toHaveBeenCalled();
  });
});
