import { renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { createTestQueryClient, createWrapper } from "@/test/store-utils";

import { conversationFixture } from "./agentsTestFixtures";
import type { AgentConversation } from "./agentConversations";
import {
  useAgentSidebarPublicationPolling,
  workspacePublicationFingerprint,
} from "./useAgentSidebarPublicationPolling";

const { getBulkWorkspacePublicationStatesMock } = vi.hoisted(() => ({
  getBulkWorkspacePublicationStatesMock: vi.fn(),
}));

const { toastErrorMock, toastDismissMock } = vi.hoisted(() => ({
  toastErrorMock: vi.fn(),
  toastDismissMock: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: { error: toastErrorMock, dismiss: toastDismissMock },
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
    toastErrorMock.mockReset();
    toastDismissMock.mockReset();
  });

  it("surfaces a failed publication read instead of silently freezing badges", async () => {
    const queryClient = createTestQueryClient();
    getBulkWorkspacePublicationStatesMock.mockRejectedValueOnce({
      outcome: "commandError",
      error: "REMOTE_INTERNAL_ERROR: publication state unavailable",
    });

    renderHook(
      () =>
        useAgentSidebarPublicationPolling(
          [conversation("conv-failed")],
          true,
          new Map([["conv-failed", workspacePublicationFingerprint("active", null)]]),
        ),
      { wrapper: createWrapper(queryClient) },
    );

    await waitFor(() =>
      expect(toastErrorMock).toHaveBeenCalledWith(
        "Pull request status could not be refreshed.",
        { id: "agent-sidebar-publication-poll-error" },
      ),
    );
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
            ["conv-merged", workspacePublicationFingerprint("draft", null)],
            ["conv-active", workspacePublicationFingerprint("active", null)],
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

  it("invalidates sidebar and Review PR caches when only the publication label changes", async () => {
    const queryClient = createTestQueryClient();
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");
    getBulkWorkspacePublicationStatesMock.mockResolvedValueOnce({
      "conv-label": {
        publication_state: "active",
        publication_label: "merged",
      },
    });

    renderHook(
      () =>
        useAgentSidebarPublicationPolling(
          [conversation("conv-label")],
          true,
          new Map([
            [
              "conv-label",
              workspacePublicationFingerprint("active", "blocked"),
            ],
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
      queryKey: ["agents", "workspace-pr-review", "conv-label"],
    });
  });
});
