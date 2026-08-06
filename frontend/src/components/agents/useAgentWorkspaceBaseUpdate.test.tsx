import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { conversationWorkspaceFixture } from "./agentsTestFixtures";
import { agentWorkspaceKeys } from "./agentWorkspaceQueries";
import { useAgentWorkspaceBaseUpdate } from "./useAgentWorkspaceBaseUpdate";

const {
  getAgentConversationWorkspaceMock,
  updateAgentConversationWorkspaceFromBaseMock,
  watchAgentWorkspaceOperationMock,
  reportAgentWorkspaceOperationResultMock,
} = vi.hoisted(() => ({
  getAgentConversationWorkspaceMock: vi.fn(),
  updateAgentConversationWorkspaceFromBaseMock: vi.fn(),
  watchAgentWorkspaceOperationMock: vi.fn(),
  reportAgentWorkspaceOperationResultMock: vi.fn(),
}));

vi.mock("@/api/chat", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/chat")>();
  return {
    ...actual,
    chatApi: {
      ...actual.chatApi,
      getAgentConversationWorkspace: (...args: unknown[]) =>
        getAgentConversationWorkspaceMock(...args),
      updateAgentConversationWorkspaceFromBase: (...args: unknown[]) =>
        updateAgentConversationWorkspaceFromBaseMock(...args),
    },
  };
});

vi.mock("./agentWorkspaceOperationRegistry", () => ({
  watchAgentWorkspaceOperation: (...args: unknown[]) =>
    watchAgentWorkspaceOperationMock(...args),
  reportAgentWorkspaceOperationResult: (...args: unknown[]) =>
    reportAgentWorkspaceOperationResultMock(...args),
}));

const activeMaintenanceOperation = {
  operationId: "operation-live",
  generation: 1,
  source: "base_update" as const,
  stage: "repairing",
  status: "active" as const,
  summary: "Resolving a conflict",
  blocker: null,
  automaticContinuation: true,
  startedAt: "2026-07-25T10:00:00Z",
  updatedAt: "2026-07-25T10:01:00Z",
};

function wrapper(queryClient: QueryClient) {
  return function TestWrapper({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
  };
}

function freshnessCacheQueryClient() {
  return new QueryClient({
    defaultOptions: { queries: { gcTime: Infinity, retry: false } },
  });
}

function staleFreshness(scope: "full" | "local") {
  return {
    conversationId: "conversation-1",
    freshnessScope: scope,
    baseRef: "main",
    baseDisplayName: "Project default (main)",
    targetRef: "origin/main",
    capturedBaseCommit: "old-base",
    targetBaseCommit: "new-base",
    isBaseAhead: true,
    hasUncommittedChanges: false,
    unpublishedCommitCount: 0,
    remoteRefreshed: true,
    worktreeStatusChecked: true,
    baseStatus: "valid" as const,
    effectiveBaseRef: null,
    effectiveBaseDisplayName: null,
    baseBlockReason: null,
  };
}

describe("useAgentWorkspaceBaseUpdate", () => {
  beforeEach(() => {
    getAgentConversationWorkspaceMock.mockReset();
    updateAgentConversationWorkspaceFromBaseMock.mockReset();
    watchAgentWorkspaceOperationMock.mockClear();
    reportAgentWorkspaceOperationResultMock.mockClear();
  });

  it.each([false, true])(
    "reconciles existing full and local freshness caches and reports the base-update/base-already-current result (updated: %s)",
    async (updated) => {
      const queryClient = freshnessCacheQueryClient();
      const workspace = conversationWorkspaceFixture();
      for (const scope of ["full", "local"] as const) {
        queryClient.setQueryData(
          agentWorkspaceKeys.scopedFreshness(workspace.conversationId, scope),
          staleFreshness(scope),
        );
      }
      updateAgentConversationWorkspaceFromBaseMock.mockResolvedValue({
        workspace,
        updated,
        repairStarted: false,
        targetRef: "origin/main",
        baseCommit: "updated-base",
        baseStatus: "valid",
        effectiveBaseDisplayName: null,
      });
      const { result } = renderHook(
        () => useAgentWorkspaceBaseUpdate({ conversationTitle: "Checkout flow fix" }),
        { wrapper: wrapper(queryClient) },
      );

      act(() => {
        result.current.runUpdateFromBase({
          conversationId: workspace.conversationId,
          detail: "Update workspace from main",
          kind: "update-from-base",
          title: "Updating from base",
        });
      });

      await waitFor(() =>
        expect(reportAgentWorkspaceOperationResultMock).toHaveBeenCalledWith(
          workspace.conversationId,
          {
            kind: updated ? "base-updated" : "base-already-current",
            targetRef: "origin/main",
          },
        ),
      );
      for (const scope of ["full", "local"] as const) {
        expect(
          queryClient.getQueryData(
            agentWorkspaceKeys.scopedFreshness(workspace.conversationId, scope),
          ),
        ).toEqual(
          expect.objectContaining({
            isBaseAhead: false,
            capturedBaseCommit: "updated-base",
            targetBaseCommit: "updated-base",
            targetRef: "origin/main",
            baseStatus: "valid",
          }),
        );
      }
    },
  );

  it("does not fabricate freshness cache entries after a successful base update", async () => {
    const queryClient = freshnessCacheQueryClient();
    const workspace = conversationWorkspaceFixture();
    updateAgentConversationWorkspaceFromBaseMock.mockResolvedValue({
      workspace,
      updated: false,
      repairStarted: false,
      targetRef: "origin/main",
      baseCommit: "updated-base",
      baseStatus: "valid",
      effectiveBaseDisplayName: null,
    });
    const { result } = renderHook(
      () => useAgentWorkspaceBaseUpdate({ conversationTitle: "Checkout flow fix" }),
      { wrapper: wrapper(queryClient) },
    );

    act(() => {
      result.current.runUpdateFromBase({
        conversationId: workspace.conversationId,
        detail: "Update workspace from main",
        kind: "update-from-base",
        title: "Updating from base",
      });
    });

    await waitFor(() =>
      expect(reportAgentWorkspaceOperationResultMock).toHaveBeenCalledWith(
        workspace.conversationId,
        { kind: "base-already-current", targetRef: "origin/main" },
      ),
    );
    expect(queryClient.getQueryData(agentWorkspaceKeys.scopedFreshness("conversation-1", "full"))).toBeUndefined();
    expect(queryClient.getQueryData(agentWorkspaceKeys.scopedFreshness("conversation-1", "local"))).toBeUndefined();
  });

  it("registers the conversation in the watch registry before the mutation resolves", async () => {
    const queryClient = freshnessCacheQueryClient();
    const workspace = conversationWorkspaceFixture();
    updateAgentConversationWorkspaceFromBaseMock.mockResolvedValue({
      workspace,
      updated: true,
      repairStarted: false,
      targetRef: "origin/main",
      baseCommit: "updated-base",
      baseStatus: "valid",
      effectiveBaseDisplayName: null,
    });
    const { result } = renderHook(
      () => useAgentWorkspaceBaseUpdate({ conversationTitle: "Checkout flow fix" }),
      { wrapper: wrapper(queryClient) },
    );

    act(() => {
      result.current.runUpdateFromBase({
        conversationId: "conversation-1",
        detail: "Update workspace from main",
        kind: "update-from-base",
        title: "Updating from base",
      });
    });

    // The watch registration is synchronous inside runUpdateFromBase, while
    // the mutation settles asynchronously — asserting immediately after the
    // synchronous act() call proves ordering without racing the mutation.
    expect(watchAgentWorkspaceOperationMock).toHaveBeenCalledWith(
      expect.objectContaining({
        conversationId: "conversation-1",
        projectId: null,
        conversationTitle: "Checkout flow fix",
        kind: "base-update",
        startedAtMs: expect.any(Number),
      }),
    );
    expect(reportAgentWorkspaceOperationResultMock).not.toHaveBeenCalled();

    await waitFor(() =>
      expect(reportAgentWorkspaceOperationResultMock).toHaveBeenCalled(),
    );
  });

  it("reports repair-started for a successful base-update retry with no live maintenance operation", async () => {
    const queryClient = freshnessCacheQueryClient();
    const workspace = conversationWorkspaceFixture({
      publicationPushStatus: "needs_agent",
    });
    updateAgentConversationWorkspaceFromBaseMock.mockResolvedValue({
      workspace,
      updated: false,
      repairStarted: true,
      targetRef: "main",
      baseCommit: "base-sha",
      baseStatus: "valid",
      effectiveBaseDisplayName: null,
    });
    const { result } = renderHook(
      () => useAgentWorkspaceBaseUpdate({ conversationTitle: "Checkout flow fix" }),
      { wrapper: wrapper(queryClient) },
    );

    act(() => {
      result.current.runUpdateFromBase({
        conversationId: "conversation-1",
        detail: "Update workspace from main",
        kind: "update-from-base",
        title: "Updating from base",
      });
    });

    await waitFor(() =>
      expect(reportAgentWorkspaceOperationResultMock).toHaveBeenCalledWith(
        "conversation-1",
        { kind: "repair-started" },
      ),
    );
  });

  it("does NOT report a result when the repair-started branch hands off to a live maintenance operation", async () => {
    const queryClient = freshnessCacheQueryClient();
    const workspace = conversationWorkspaceFixture({
      maintenanceOperation: activeMaintenanceOperation,
    });
    updateAgentConversationWorkspaceFromBaseMock.mockResolvedValue({
      workspace,
      updated: false,
      repairStarted: true,
      targetRef: "main",
      baseCommit: "base-sha",
      baseStatus: "valid",
      effectiveBaseDisplayName: null,
    });
    const { result } = renderHook(
      () => useAgentWorkspaceBaseUpdate({ conversationTitle: "Checkout flow fix" }),
      { wrapper: wrapper(queryClient) },
    );

    act(() => {
      result.current.runUpdateFromBase({
        conversationId: "conversation-1",
        detail: "Update workspace from main",
        kind: "update-from-base",
        title: "Updating from base",
      });
    });

    await waitFor(() =>
      expect(queryClient.getQueryData(agentWorkspaceKeys.workspace(workspace.conversationId))).toEqual(
        workspace,
      ),
    );
    expect(reportAgentWorkspaceOperationResultMock).not.toHaveBeenCalled();
  });

  it("does NOT report a result when a successful update hands off to a live maintenance operation", async () => {
    const queryClient = freshnessCacheQueryClient();
    const workspace = conversationWorkspaceFixture({
      maintenanceOperation: activeMaintenanceOperation,
    });
    updateAgentConversationWorkspaceFromBaseMock.mockResolvedValue({
      workspace,
      updated: true,
      repairStarted: false,
      targetRef: "origin/main",
      baseCommit: "updated-base",
      baseStatus: "valid",
      effectiveBaseDisplayName: null,
    });
    const { result } = renderHook(
      () => useAgentWorkspaceBaseUpdate({ conversationTitle: "Checkout flow fix" }),
      { wrapper: wrapper(queryClient) },
    );

    act(() => {
      result.current.runUpdateFromBase({
        conversationId: "conversation-1",
        detail: "Update workspace from main",
        kind: "update-from-base",
        title: "Updating from base",
      });
    });

    await waitFor(() =>
      expect(queryClient.getQueryData(agentWorkspaceKeys.workspace(workspace.conversationId))).toEqual(
        workspace,
      ),
    );
    expect(reportAgentWorkspaceOperationResultMock).not.toHaveBeenCalled();
    expect(
      queryClient.getQueryData(agentWorkspaceKeys.scopedFreshness(workspace.conversationId, "full")),
    ).toBeUndefined();
  });

  it("reports base-update-failed for a plain error with no repair handoff", async () => {
    const queryClient = freshnessCacheQueryClient();
    const refreshedWorkspace = conversationWorkspaceFixture();
    updateAgentConversationWorkspaceFromBaseMock.mockRejectedValue(
      new Error("Failed to reach git host"),
    );
    getAgentConversationWorkspaceMock.mockResolvedValue(refreshedWorkspace);
    const { result } = renderHook(
      () => useAgentWorkspaceBaseUpdate({ conversationTitle: "Checkout flow fix" }),
      { wrapper: wrapper(queryClient) },
    );

    act(() => {
      result.current.runUpdateFromBase({
        conversationId: "conversation-1",
        detail: "Update workspace from main",
        kind: "update-from-base",
        title: "Updating from base",
      });
    });

    await waitFor(() =>
      expect(reportAgentWorkspaceOperationResultMock).toHaveBeenCalledWith(
        "conversation-1",
        { kind: "base-update-failed", detail: "Failed to reach git host" },
      ),
    );
  });

  it("reports repair-started for an error retry when the refreshed workspace needs an agent", async () => {
    const queryClient = freshnessCacheQueryClient();
    const refreshedWorkspace = conversationWorkspaceFixture({
      publicationPushStatus: "needs_agent",
    });
    updateAgentConversationWorkspaceFromBaseMock.mockRejectedValue(
      new Error("Push rejected"),
    );
    getAgentConversationWorkspaceMock.mockResolvedValue(refreshedWorkspace);
    const { result } = renderHook(
      () => useAgentWorkspaceBaseUpdate({ conversationTitle: "Checkout flow fix" }),
      { wrapper: wrapper(queryClient) },
    );

    act(() => {
      result.current.runUpdateFromBase({
        conversationId: "conversation-1",
        detail: "Update workspace from main",
        kind: "update-from-base",
        title: "Updating from base",
      });
    });

    await waitFor(() =>
      expect(reportAgentWorkspaceOperationResultMock).toHaveBeenCalledWith(
        "conversation-1",
        { kind: "repair-started", detail: "Push rejected" },
      ),
    );
  });

  it("does NOT report a result when an error hands off to a live maintenance operation, but still invalidates workspace queries", async () => {
    const queryClient = freshnessCacheQueryClient();
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");
    const refreshedWorkspace = conversationWorkspaceFixture({
      maintenanceOperation: activeMaintenanceOperation,
    });
    updateAgentConversationWorkspaceFromBaseMock.mockRejectedValue(
      new Error("Repair required"),
    );
    getAgentConversationWorkspaceMock.mockResolvedValue(refreshedWorkspace);
    const { result } = renderHook(
      () => useAgentWorkspaceBaseUpdate({ conversationTitle: "Checkout flow fix" }),
      { wrapper: wrapper(queryClient) },
    );

    act(() => {
      result.current.runUpdateFromBase({
        conversationId: "conversation-1",
        detail: "Update workspace from main",
        kind: "update-from-base",
        title: "Updating from base",
      });
    });

    await waitFor(() =>
      expect(queryClient.getQueryData(agentWorkspaceKeys.workspace("conversation-1"))).toEqual(
        refreshedWorkspace,
      ),
    );
    await waitFor(() =>
      expect(invalidateSpy).toHaveBeenCalledWith(
        expect.objectContaining({ queryKey: agentWorkspaceKeys.workspace("conversation-1") }),
      ),
    );
    expect(reportAgentWorkspaceOperationResultMock).not.toHaveBeenCalled();
  });
});
