import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createTestQueryClient } from "@/test/store-utils";

import { conversationWorkspaceFixture } from "./agentsTestFixtures";
import { agentWorkspaceKeys } from "./agentWorkspaceQueries";
import { useAgentWorkspaceBaseUpdate } from "./useAgentWorkspaceBaseUpdate";

const {
  getAgentConversationWorkspaceMock,
  updateAgentConversationWorkspaceFromBaseMock,
  toastDismissMock,
  toastErrorMock,
  toastInfoMock,
  toastLoadingMock,
  toastSuccessMock,
} = vi.hoisted(() => ({
    getAgentConversationWorkspaceMock: vi.fn(),
    updateAgentConversationWorkspaceFromBaseMock: vi.fn(),
    toastDismissMock: vi.fn(),
    toastErrorMock: vi.fn(),
    toastInfoMock: vi.fn(),
    toastLoadingMock: vi.fn(),
    toastSuccessMock: vi.fn(),
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

vi.mock("sonner", () => ({
  toast: {
    dismiss: (...args: unknown[]) => toastDismissMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
    info: (...args: unknown[]) => toastInfoMock(...args),
    loading: (...args: unknown[]) => toastLoadingMock(...args),
    success: (...args: unknown[]) => toastSuccessMock(...args),
  },
}));

function wrapper(queryClient: ReturnType<typeof createTestQueryClient>) {
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
    toastDismissMock.mockClear();
    toastErrorMock.mockClear();
    toastInfoMock.mockClear();
    toastLoadingMock.mockClear();
    toastSuccessMock.mockClear();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it.each([false, true])(
    "reconciles existing full and local freshness caches after a successful base update (updated: %s)",
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

      await waitFor(() => expect(toastSuccessMock).toHaveBeenCalled());
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

    await waitFor(() => expect(toastSuccessMock).toHaveBeenCalled());
    expect(queryClient.getQueryData(agentWorkspaceKeys.scopedFreshness("conversation-1", "full"))).toBeUndefined();
    expect(queryClient.getQueryData(agentWorkspaceKeys.scopedFreshness("conversation-1", "local"))).toBeUndefined();
  });

  it("shows an informational repair-started toast for a successful base-update retry", async () => {
    const queryClient = freshnessCacheQueryClient();
    const workspace = conversationWorkspaceFixture({
      publicationPushStatus: "needs_agent",
    });
    queryClient.setQueryData(
      agentWorkspaceKeys.scopedFreshness(workspace.conversationId, "full"),
      staleFreshness("full"),
    );
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
      expect(toastInfoMock).toHaveBeenCalledWith(
        "Repair started",
        expect.objectContaining({
          description: expect.stringContaining("will continue automatically"),
        }),
      ),
    );
    expect(toastErrorMock).not.toHaveBeenCalled();
    expect(
      queryClient.getQueryData(
        agentWorkspaceKeys.scopedFreshness(workspace.conversationId, "full"),
      ),
    ).toEqual(expect.objectContaining({ isBaseAhead: true }));
  });

  it("adds merged pull-request retarget context to the rebase progress toast", async () => {
    const queryClient = createTestQueryClient();
    const workspace = conversationWorkspaceFixture();
    updateAgentConversationWorkspaceFromBaseMock.mockResolvedValue({
      workspace,
      updated: true,
      repairStarted: false,
      targetRef: "release/next",
      baseCommit: "base-sha",
      baseStatus: "retargeted",
      effectiveBaseDisplayName: "release/next",
    });
    const { result } = renderHook(
      () => useAgentWorkspaceBaseUpdate({ conversationTitle: "Checkout flow fix" }),
      { wrapper: wrapper(queryClient) },
    );

    act(() => {
      result.current.runUpdateFromBase({
        baseSelection: {
          kind: "local_branch",
          ref: "release/next",
          displayName: "release/next",
          retargetedFromPullRequest: 88,
        },
        conversationId: "conversation-1",
        detail: "From release/next",
        kind: "rebase",
        title: "Rebasing onto release/next",
      });
    });

    expect(toastLoadingMock).toHaveBeenCalledWith(
      "Rebasing onto release/next",
      expect.objectContaining({
        description: expect.stringContaining(
          "PR #88 merged — rebasing onto release/next",
        ),
      }),
    );
  });

  it("dismisses live maintenance progress when the hook unmounts", () => {
    vi.useFakeTimers();
    const queryClient = createTestQueryClient();
    const activeWorkspace = conversationWorkspaceFixture({
      maintenanceOperation: {
        operationId: "operation-unmount",
        generation: 1,
        source: "base_update",
        stage: "repairing",
        status: "active",
        summary: "Resolving a conflict",
        blocker: null,
        automaticContinuation: true,
        startedAt: "2026-07-25T10:00:00Z",
        updatedAt: "2026-07-25T10:01:00Z",
      },
    });
    const { result, unmount } = renderHook(
      () => useAgentWorkspaceBaseUpdate({ conversationTitle: "Checkout flow fix" }),
      { wrapper: wrapper(queryClient) },
    );

    act(() => {
      result.current.syncMaintenanceOperation(activeWorkspace);
    });
    const loadingCallCount = toastLoadingMock.mock.calls.length;

    unmount();
    vi.advanceTimersByTime(2_000);

    expect(toastDismissMock).toHaveBeenCalledWith(
      "agent-workspace-maintenance:conversation-1:operation-unmount",
    );
    expect(toastLoadingMock).toHaveBeenCalledTimes(loadingCallCount);
  });

  it("refreshes once before settling a disappeared active maintenance operation", async () => {
    const queryClient = createTestQueryClient();
    const activeWorkspace = conversationWorkspaceFixture({
      maintenanceOperation: {
        operationId: "operation-1",
        generation: 1,
        source: "base_update",
        stage: "repairing",
        status: "active",
        summary: "Resolving a conflict",
        blocker: null,
        automaticContinuation: true,
        startedAt: "2026-07-25T10:00:00Z",
        updatedAt: "2026-07-25T10:01:00Z",
      },
    });
    const publishedWorkspace = conversationWorkspaceFixture({
      publicationPrNumber: 204,
      publicationPushStatus: "pushed",
    });
    getAgentConversationWorkspaceMock.mockResolvedValue(publishedWorkspace);

    const { result } = renderHook(
      () => useAgentWorkspaceBaseUpdate({ conversationTitle: "Checkout flow fix" }),
      { wrapper: wrapper(queryClient) },
    );

    act(() => {
      result.current.syncMaintenanceOperation(activeWorkspace);
    });
    await waitFor(() => expect(toastLoadingMock).toHaveBeenCalled());

    act(() => {
      result.current.syncMaintenanceOperation(null);
      result.current.syncMaintenanceOperation(null);
    });

    await waitFor(() =>
      expect(getAgentConversationWorkspaceMock).toHaveBeenCalledWith("conversation-1"),
    );
    await waitFor(() =>
      expect(toastSuccessMock).toHaveBeenCalledWith(
        "Published pull request #204",
        expect.objectContaining({
          id: "agent-workspace-maintenance:conversation-1:operation-1",
        }),
      ),
    );
    expect(getAgentConversationWorkspaceMock).toHaveBeenCalledTimes(1);
  });

  it.each([
    ["ready", "Base updated — ready to publish"],
    ["blocked", "Repair blocked"],
  ] as const)(
    "settles a disappeared operation from a refreshed %s durable state",
    async (status, message) => {
      const queryClient = createTestQueryClient();
      const activeWorkspace = conversationWorkspaceFixture({
        maintenanceOperation: {
          operationId: "operation-refresh",
          generation: 1,
          source: "base_update",
          stage: "repairing",
          status: "active",
          summary: "Resolving a conflict",
          blocker: null,
          automaticContinuation: true,
          startedAt: "2026-07-25T10:00:00Z",
          updatedAt: "2026-07-25T10:01:00Z",
        },
      });
      const refreshedWorkspace = conversationWorkspaceFixture({
        maintenanceOperation: {
          ...activeWorkspace.maintenanceOperation!,
          stage: status,
          status,
          summary: status === "ready" ? "Ready for publish" : "Repair needs help",
          blocker: status === "blocked" ? "Protected branch" : null,
        },
      });
      getAgentConversationWorkspaceMock.mockResolvedValue(refreshedWorkspace);
      const { result } = renderHook(
        () => useAgentWorkspaceBaseUpdate({ conversationTitle: "Checkout flow fix" }),
        { wrapper: wrapper(queryClient) },
      );

      act(() => {
        result.current.syncMaintenanceOperation(activeWorkspace);
        result.current.syncMaintenanceOperation(null);
      });

      const terminalMock = status === "ready" ? toastSuccessMock : toastErrorMock;
      await waitFor(() =>
        expect(terminalMock).toHaveBeenCalledWith(
          message,
          expect.objectContaining({
            id: "agent-workspace-maintenance:conversation-1:operation-refresh",
          }),
        ),
      );
    },
  );

  it("settles an unverified disappeared operation without claiming completion", async () => {
    const queryClient = createTestQueryClient();
    const activeWorkspace = conversationWorkspaceFixture({
      maintenanceOperation: {
        operationId: "operation-active-refresh",
        generation: 1,
        source: "base_update",
        stage: "repairing",
        status: "active",
        summary: "Resolving a conflict",
        blocker: null,
        automaticContinuation: true,
        startedAt: "2026-07-25T10:00:00Z",
        updatedAt: "2026-07-25T10:01:00Z",
      },
    });
    getAgentConversationWorkspaceMock.mockResolvedValueOnce(activeWorkspace);
    const { result } = renderHook(
      () => useAgentWorkspaceBaseUpdate({ conversationTitle: null }),
      { wrapper: wrapper(queryClient) },
    );

    act(() => {
      result.current.syncMaintenanceOperation(activeWorkspace);
      result.current.syncMaintenanceOperation(null);
    });
    await waitFor(() => expect(getAgentConversationWorkspaceMock).toHaveBeenCalledTimes(1));
    expect(toastSuccessMock).not.toHaveBeenCalled();
    expect(toastErrorMock).not.toHaveBeenCalled();
    expect(toastInfoMock).not.toHaveBeenCalled();

    getAgentConversationWorkspaceMock.mockRejectedValueOnce(new Error("offline"));
    act(() => {
      result.current.syncMaintenanceOperation(activeWorkspace);
      result.current.syncMaintenanceOperation(null);
    });
    await waitFor(() =>
      expect(toastInfoMock).toHaveBeenCalledWith(
        "Couldn't verify workspace operation",
        expect.objectContaining({
          description: expect.stringContaining(
            "Check the workspace publish panel, then retry after reconnecting.",
          ),
          id: "agent-workspace-maintenance:conversation-1:operation-active-refresh",
        }),
      ),
    );
    expect(toastSuccessMock).not.toHaveBeenCalled();
    expect(toastInfoMock).not.toHaveBeenCalledWith(
      "Workspace operation completed",
      expect.anything(),
    );
  });

  it("updates one active toast and settles terminal operations only once", async () => {
    const queryClient = createTestQueryClient();
    const activeWorkspace = conversationWorkspaceFixture({
      maintenanceOperation: {
        operationId: "operation-direct",
        generation: 1,
        source: "base_update",
        stage: "repairing",
        status: "active",
        summary: "Resolving",
        blocker: null,
        automaticContinuation: true,
        startedAt: "2026-07-25T10:00:00Z",
        updatedAt: "2026-07-25T10:01:00Z",
      },
    });
    const { result } = renderHook(
      () => useAgentWorkspaceBaseUpdate({ conversationTitle: "Checkout flow fix" }),
      { wrapper: wrapper(queryClient) },
    );

    act(() => {
      result.current.syncMaintenanceOperation(activeWorkspace);
      result.current.syncMaintenanceOperation({
        ...activeWorkspace,
        maintenanceOperation: {
          ...activeWorkspace.maintenanceOperation!,
          stage: "validating",
          summary: "Checking the repair",
        },
      });
    });
    expect(toastLoadingMock).toHaveBeenLastCalledWith(
      "Validating repair",
      expect.objectContaining({
        id: "agent-workspace-maintenance:conversation-1:operation-direct",
      }),
    );

    const readyWorkspace = conversationWorkspaceFixture({
      maintenanceOperation: {
        ...activeWorkspace.maintenanceOperation!,
        stage: "ready",
        status: "ready",
        summary: "Ready for publish",
      },
    });
    act(() => {
      result.current.syncMaintenanceOperation(readyWorkspace, true);
      result.current.syncMaintenanceOperation(readyWorkspace, true);
    });
    expect(toastSuccessMock).toHaveBeenCalledTimes(1);

    act(() => {
      result.current.syncMaintenanceOperation(
        conversationWorkspaceFixture({
          maintenanceOperation: {
            ...activeWorkspace.maintenanceOperation!,
            operationId: "operation-blocked",
            stage: "blocked",
            status: "blocked",
            blocker: "Resolve the conflict",
          },
        }),
        true,
      );
    });
    expect(toastErrorMock).toHaveBeenCalledWith(
      "Repair blocked",
      expect.objectContaining({
        id: "agent-workspace-maintenance:conversation-1:operation-blocked",
      }),
    );
  });
});
