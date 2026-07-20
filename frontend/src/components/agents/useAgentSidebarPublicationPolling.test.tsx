import { renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { createTestQueryClient, createWrapper } from "@/test/store-utils";

import { conversationFixture } from "./agentsTestFixtures";
import type { AgentConversation } from "./agentConversations";
import { useAgentSidebarPublicationPolling } from "./useAgentSidebarPublicationPolling";

const { getBulkWorkspacePublicationStatesMock } = vi.hoisted(() => ({
  getBulkWorkspacePublicationStatesMock: vi.fn(),
}));

vi.mock("@/api/chat", () => ({
  chatApi: {
    getBulkWorkspacePublicationStates: getBulkWorkspacePublicationStatesMock,
  },
}));

function conversation(id: string): AgentConversation {
  return conversationFixture({ id, title: id });
}

describe("useAgentSidebarPublicationPolling", () => {
  beforeEach(() => {
    getBulkWorkspacePublicationStatesMock.mockReset();
  });

  it("invalidates workspace publish caches when sidebar publication state changes", async () => {
    const queryClient = createTestQueryClient();
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");
    getBulkWorkspacePublicationStatesMock.mockResolvedValueOnce({
      "conv-merged": {
        publication_state: "merged",
        publication_label: "merged",
      },
      "conv-active": {
        publication_state: "active",
        publication_label: null,
      },
    });

    renderHook(
      () =>
        useAgentSidebarPublicationPolling(
          [conversation("conv-merged"), conversation("conv-active")],
          true,
          new Map([
            ["conv-merged", "draft"],
            ["conv-active", "active"],
          ]),
        ),
      { wrapper: createWrapper(queryClient) },
    );

    await waitFor(() =>
      expect(invalidateSpy).toHaveBeenCalledWith({
        queryKey: ["agents", "sidebar-conversations"],
      }),
    );

    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["agents", "conversation-workspace", "conv-merged"],
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["agents", "conversation-workspace-freshness", "conv-merged"],
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["agents", "conversation-workspace-publication-events", "conv-merged"],
    });
    expect(invalidateSpy).not.toHaveBeenCalledWith({
      queryKey: ["agents", "workspace-review-context", "conv-merged"],
    });
    expect(invalidateSpy).not.toHaveBeenCalledWith({
      queryKey: ["agents", "conversation-workspace", "conv-active"],
    });
  });
});
