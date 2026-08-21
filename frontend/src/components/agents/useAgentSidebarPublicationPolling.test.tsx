import { renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { createTestQueryClient, createWrapper } from "@/test/store-utils";

import { conversationFixture } from "./agentsTestFixtures";
import type { AgentConversation } from "./agentConversations";
import {
  useAgentSidebarPublicationPolling,
  workspacePublicationFingerprint,
} from "./useAgentSidebarPublicationPolling";

const SIDEBAR_FILTERS = { queryKey: ["agents", "sidebar-conversations"] };
const NON_CANCELLING = { cancelRefetch: false };

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
        review_state: null,
      },
      "conv-active": {
        publication_state: "active",
        publication_label: null,
        review_state: null,
      },
    });

    renderHook(
      () =>
        useAgentSidebarPublicationPolling(
          [conversation("conv-merged"), conversation("conv-active")],
          true,
          new Map([
            ["conv-merged", workspacePublicationFingerprint("draft", null, null)],
            ["conv-active", workspacePublicationFingerprint("active", null, null)],
          ]),
        ),
      { wrapper: createWrapper(queryClient) },
    );

    await waitFor(() =>
      expect(invalidateSpy).toHaveBeenCalledWith(SIDEBAR_FILTERS, NON_CANCELLING),
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
        review_state: null,
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
              workspacePublicationFingerprint("active", "blocked", null),
            ],
          ]),
        ),
      { wrapper: createWrapper(queryClient) },
    );

    await waitFor(() =>
      expect(invalidateSpy).toHaveBeenCalledWith(SIDEBAR_FILTERS, NON_CANCELLING),
    );
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["agents", "workspace-pr-review", "conv-label"],
    });
  });

  it("invalidates the sidebar when only the Review PR state changes", async () => {
    const queryClient = createTestQueryClient();
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");
    // Submitting an approval moves the monitor from awaiting_user to watching
    // without touching publication state or label.
    getBulkWorkspacePublicationStatesMock.mockResolvedValueOnce({
      "conv-review": {
        publication_state: "active",
        publication_label: null,
        review_state: "approved",
      },
    });

    renderHook(
      () =>
        useAgentSidebarPublicationPolling(
          [conversation("conv-review")],
          true,
          new Map([
            [
              "conv-review",
              workspacePublicationFingerprint("active", null, "needs_approval"),
            ],
          ]),
        ),
      { wrapper: createWrapper(queryClient) },
    );

    await waitFor(() =>
      expect(invalidateSpy).toHaveBeenCalledWith(SIDEBAR_FILTERS, NON_CANCELLING),
    );
  });

  it("leaves the sidebar alone when an unchanged review row is polled", async () => {
    const queryClient = createTestQueryClient();
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");
    getBulkWorkspacePublicationStatesMock.mockResolvedValueOnce({
      "conv-review": {
        publication_state: "active",
        publication_label: null,
        review_state: "approved",
      },
    });

    // Guards the two-producer drift in AgentsSidebar: a cached fingerprint
    // built from the same row must equal the polled one, or every 5s tick
    // would invalidate the whole sidebar query.
    renderHook(
      () =>
        useAgentSidebarPublicationPolling(
          [conversation("conv-review")],
          true,
          new Map([
            [
              "conv-review",
              workspacePublicationFingerprint("active", null, "approved"),
            ],
          ]),
        ),
      { wrapper: createWrapper(queryClient) },
    );

    await waitFor(() =>
      expect(getBulkWorkspacePublicationStatesMock).toHaveBeenCalled(),
    );
    expect(invalidateSpy).not.toHaveBeenCalledWith(SIDEBAR_FILTERS, NON_CANCELLING);
  });

  it("skips the sidebar invalidation while a listing is in flight, but still refreshes the changed workspaces", async () => {
    const queryClient = createTestQueryClient();
    // A multi-minute listing is already running; stacking invalidations on it is
    // exactly what livelocked the sidebar.
    vi.spyOn(queryClient, "isFetching").mockReturnValue(1);
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");
    getBulkWorkspacePublicationStatesMock.mockResolvedValue({
      "conv-merged": {
        publication_state: "merged",
        publication_label: "merged",
        review_state: null,
      },
    });

    renderHook(
      () =>
        useAgentSidebarPublicationPolling(
          [conversation("conv-merged")],
          true,
          new Map([
            ["conv-merged", workspacePublicationFingerprint("draft", null, null)],
          ]),
        ),
      { wrapper: createWrapper(queryClient) },
    );

    await waitFor(() =>
      expect(invalidateSpy).toHaveBeenCalledWith({
        queryKey: ["agents", "conversation-workspace", "conv-merged"],
      }),
    );
    expect(invalidateSpy).not.toHaveBeenCalledWith(SIDEBAR_FILTERS, NON_CANCELLING);
  });

  it("invalidates the sidebar on a later tick once the in-flight listing settles", async () => {
    const queryClient = createTestQueryClient();
    // The guard must be self-clearing: if it could outlive the fetch it would
    // wedge the cache, reproducing the bug it exists to fix.
    const isFetchingSpy = vi
      .spyOn(queryClient, "isFetching")
      .mockReturnValueOnce(1)
      .mockReturnValue(0);
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");
    getBulkWorkspacePublicationStatesMock.mockResolvedValue({
      "conv-merged": {
        publication_state: "merged",
        publication_label: "merged",
        review_state: null,
      },
    });

    const { rerender } = renderHook(
      ({ visible }: { visible: boolean }) =>
        useAgentSidebarPublicationPolling(
          [conversation("conv-merged")],
          visible,
          new Map([
            ["conv-merged", workspacePublicationFingerprint("draft", null, null)],
          ]),
        ),
      {
        wrapper: createWrapper(queryClient),
        initialProps: { visible: true },
      },
    );

    await waitFor(() => expect(isFetchingSpy).toHaveBeenCalled());
    expect(invalidateSpy).not.toHaveBeenCalledWith(SIDEBAR_FILTERS, NON_CANCELLING);

    // Remount the poll effect to stand in for the next 5s tick.
    rerender({ visible: false });
    rerender({ visible: true });

    await waitFor(() =>
      expect(invalidateSpy).toHaveBeenCalledWith(SIDEBAR_FILTERS, NON_CANCELLING),
    );
  });
});
