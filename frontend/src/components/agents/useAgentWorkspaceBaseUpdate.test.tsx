import { QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { createTestQueryClient } from "@/test/store-utils";

import { conversationWorkspaceFixture } from "./agentsTestFixtures";
import { useAgentWorkspaceBaseUpdate } from "./useAgentWorkspaceBaseUpdate";

const { getAgentConversationWorkspaceMock, toastLoadingMock, toastSuccessMock } =
  vi.hoisted(() => ({
    getAgentConversationWorkspaceMock: vi.fn(),
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
    error: vi.fn(),
    info: vi.fn(),
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
});
