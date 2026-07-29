import { QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { createTestQueryClient } from "@/test/store-utils";

import { conversationWorkspaceFixture } from "./agentsTestFixtures";
import { useAgentWorkspaceBaseUpdate } from "./useAgentWorkspaceBaseUpdate";

const {
  getAgentConversationWorkspaceMock,
  toastErrorMock,
  toastInfoMock,
  toastLoadingMock,
  toastSuccessMock,
} = vi.hoisted(() => ({
    getAgentConversationWorkspaceMock: vi.fn(),
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
    },
  };
});

vi.mock("sonner", () => ({
  toast: {
    dismiss: vi.fn(),
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

describe("useAgentWorkspaceBaseUpdate", () => {
  beforeEach(() => {
    getAgentConversationWorkspaceMock.mockReset();
    toastErrorMock.mockClear();
    toastInfoMock.mockClear();
    toastLoadingMock.mockClear();
    toastSuccessMock.mockClear();
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
