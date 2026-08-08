import { QueryClientProvider } from "@tanstack/react-query";
import type { QueryClient } from "@tanstack/react-query";
import { act, fireEvent, render, renderHook, screen, waitFor } from "@testing-library/react";
import type { ReactElement, ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { AgentWorkspaceMaintenanceOperation } from "@/api/chat";
import { createTestQueryClient } from "@/test/store-utils";

import { conversationWorkspaceFixture } from "./agentsTestFixtures";
import {
  agentWorkspaceMaintenanceOperationToastId,
  agentWorkspaceOperationToastId,
  resetAgentWorkspaceOperationToastStateForTests,
} from "./agentWorkspaceOperationToast";
import {
  agentWorkspaceRepairDismissalKey,
  isAgentWorkspaceOperationDismissed,
  rehydrateAgentWorkspaceOperationDismissalsForTests,
  resetAgentWorkspaceOperationDismissalsForTests,
} from "./agentWorkspaceOperationDismissals";
import {
  getWatchedAgentWorkspaceOperations,
  markAgentWorkspaceOperationAnnounced,
  rehydrateAgentWorkspaceOperationRegistryForTests,
  reportAgentWorkspaceOperationResult,
  resetAgentWorkspaceOperationRegistryForTests,
  watchAgentWorkspaceOperation,
} from "./agentWorkspaceOperationRegistry";
import { agentWorkspaceKeys } from "./agentWorkspaceQueries";
import { useAgentWorkspaceOperationToasts } from "./useAgentWorkspaceOperationToasts";

const {
  getAgentConversationWorkspaceMock,
  toastDismissMock,
  toastErrorMock,
  toastInfoMock,
  toastLoadingMock,
  toastSuccessMock,
  startupState,
  visibleScopeState,
} = vi.hoisted(() => ({
  getAgentConversationWorkspaceMock: vi.fn(),
  toastDismissMock: vi.fn(),
  toastErrorMock: vi.fn(),
  toastInfoMock: vi.fn(),
  toastLoadingMock: vi.fn(),
  toastSuccessMock: vi.fn(),
  startupState: { isBackgroundSettled: true },
  visibleScopeState: { visibleConversationId: null as string | null },
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
    dismiss: (...args: unknown[]) => toastDismissMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
    info: (...args: unknown[]) => toastInfoMock(...args),
    loading: (...args: unknown[]) => toastLoadingMock(...args),
    success: (...args: unknown[]) => toastSuccessMock(...args),
  },
}));

vi.mock("@/hooks/useStartupStatus", () => ({
  useStartupStatus: () => ({ isBackgroundSettled: startupState.isBackgroundSettled }),
}));

vi.mock("@/stores/agentSessionStore", () => ({
  useAgentSessionStore: (
    selector: (state: {
      visibleAgentScope: { visibleConversationId: string | null } | null;
    }) => unknown,
  ) =>
    selector({
      visibleAgentScope: { visibleConversationId: visibleScopeState.visibleConversationId },
    }),
}));

const DISMISSALS_STORAGE_KEY = "ralphx-workspace-operation-dismissals";
const CONVERSATION_ID = "conversation-1";

function wrapper(queryClient: QueryClient) {
  return function TestWrapper({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
  };
}

function lastCallContent(mock: typeof toastLoadingMock): ReactElement {
  return mock.mock.calls.at(-1)?.[0] as ReactElement;
}

function maintenanceOperationFixture(
  overrides: Partial<AgentWorkspaceMaintenanceOperation> = {},
): AgentWorkspaceMaintenanceOperation {
  return {
    operationId: "op-1",
    generation: 1,
    source: "base_update",
    stage: "repairing",
    status: "active",
    holdReason: null,
    summary: "Repairing workspace",
    blocker: null,
    automaticContinuation: false,
    startedAt: "2026-08-05T09:00:00Z",
    updatedAt: "2026-08-05T09:00:00Z",
    ...overrides,
  };
}

function seedWatchedEntry(
  overrides: Partial<{
    conversationId: string;
    kind: "publish" | "base-update" | "observed";
    publishAttemptKey: string;
    startedAtMs: number | null;
  }> = {},
): void {
  watchAgentWorkspaceOperation({
    conversationId: overrides.conversationId ?? CONVERSATION_ID,
    projectId: "project-1",
    conversationTitle: "Checkout flow fix",
    kind: overrides.kind ?? "observed",
    startedAtMs: overrides.startedAtMs ?? null,
    ...(overrides.publishAttemptKey !== undefined
      ? { publishAttemptKey: overrides.publishAttemptKey }
      : {}),
  });
}

async function flush(): Promise<void> {
  await act(async () => {
    await Promise.resolve();
  });
}

async function dismissLastToast(mock: typeof toastLoadingMock): Promise<void> {
  const { unmount } = render(lastCallContent(mock));
  fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
  unmount();
}

describe("useAgentWorkspaceOperationToasts", () => {
  beforeEach(() => {
    resetAgentWorkspaceOperationRegistryForTests();
    resetAgentWorkspaceOperationDismissalsForTests();
    resetAgentWorkspaceOperationToastStateForTests();
    getAgentConversationWorkspaceMock.mockReset();
    toastDismissMock.mockClear();
    toastErrorMock.mockClear();
    toastInfoMock.mockClear();
    toastLoadingMock.mockClear();
    toastSuccessMock.mockClear();
    startupState.isBackgroundSettled = true;
    visibleScopeState.visibleConversationId = null;
  });

  afterEach(() => {
    resetAgentWorkspaceOperationRegistryForTests();
    resetAgentWorkspaceOperationDismissalsForTests();
  });

  it("1. renders nothing and starts no fetch while isBackgroundSettled is false", async () => {
    startupState.isBackgroundSettled = false;
    seedWatchedEntry();
    const queryClient = createTestQueryClient();

    renderHook(() => useAgentWorkspaceOperationToasts(), { wrapper: wrapper(queryClient) });
    await flush();

    expect(getAgentConversationWorkspaceMock).not.toHaveBeenCalled();
    expect(toastLoadingMock).not.toHaveBeenCalled();
    expect(toastSuccessMock).not.toHaveBeenCalled();
    expect(toastErrorMock).not.toHaveBeenCalled();
    expect(toastInfoMock).not.toHaveBeenCalled();
  });

  it("2. renders one loading toast with the stage title for an active maintenance operation", async () => {
    seedWatchedEntry();
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspaceFixture({
        maintenanceOperation: maintenanceOperationFixture({ stage: "repairing", status: "active" }),
      }),
    );
    const queryClient = createTestQueryClient();

    renderHook(() => useAgentWorkspaceOperationToasts(), { wrapper: wrapper(queryClient) });

    await waitFor(() => expect(toastLoadingMock).toHaveBeenCalledTimes(1));
    render(lastCallContent(toastLoadingMock));
    expect(screen.getByText("Repairing workspace")).toBeInTheDocument();
    expect(toastSuccessMock).not.toHaveBeenCalled();
    expect(toastErrorMock).not.toHaveBeenCalled();
    expect(toastInfoMock).not.toHaveBeenCalled();
  });

  it("3. dismisses the loading toast and unwatches once the operation clears with no invented message", async () => {
    seedWatchedEntry();
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspaceFixture({
        maintenanceOperation: maintenanceOperationFixture({ stage: "repairing", status: "active" }),
      }),
    );
    const queryClient = createTestQueryClient();

    renderHook(() => useAgentWorkspaceOperationToasts(), { wrapper: wrapper(queryClient) });
    await waitFor(() => expect(toastLoadingMock).toHaveBeenCalledTimes(1));

    await act(async () => {
      queryClient.setQueryData(
        agentWorkspaceKeys.workspace(CONVERSATION_ID),
        conversationWorkspaceFixture({ maintenanceOperation: null }),
      );
      await Promise.resolve();
    });

    await waitFor(() =>
      expect(toastDismissMock).toHaveBeenCalledWith(
        agentWorkspaceMaintenanceOperationToastId(CONVERSATION_ID, "op-1"),
      ),
    );
    await waitFor(() => expect(getWatchedAgentWorkspaceOperations()).toHaveLength(0));
    expect(toastSuccessMock).not.toHaveBeenCalled();
    expect(toastInfoMock).not.toHaveBeenCalled();
    expect(toastErrorMock).not.toHaveBeenCalled();
  });

  it("4. dismiss stops rendering and the dismissal is readable from localStorage", async () => {
    seedWatchedEntry();
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspaceFixture({
        maintenanceOperation: maintenanceOperationFixture({ stage: "blocked", status: "blocked" }),
      }),
    );
    const queryClient = createTestQueryClient();

    renderHook(() => useAgentWorkspaceOperationToasts(), { wrapper: wrapper(queryClient) });
    await waitFor(() => expect(toastErrorMock).toHaveBeenCalledTimes(1));
    await dismissLastToast(toastErrorMock);

    const dismissalKey = agentWorkspaceRepairDismissalKey(CONVERSATION_ID, "op-1", 1);
    expect(isAgentWorkspaceOperationDismissed(dismissalKey)).toBe(true);
    const stored = JSON.parse(localStorage.getItem(DISMISSALS_STORAGE_KEY) ?? "{}") as Record<
      string,
      number
    >;
    expect(stored[dismissalKey]).toEqual(expect.any(Number));

    toastErrorMock.mockClear();
    await act(async () => {
      queryClient.setQueryData(
        agentWorkspaceKeys.workspace(CONVERSATION_ID),
        conversationWorkspaceFixture({
          maintenanceOperation: maintenanceOperationFixture({ stage: "blocked", status: "blocked" }),
        }),
      );
      await Promise.resolve();
    });
    expect(toastErrorMock).not.toHaveBeenCalled();
  });

  it("5. does not resurrect a dismissed toast after a simulated restart against identical durable state", async () => {
    seedWatchedEntry();
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspaceFixture({
        maintenanceOperation: maintenanceOperationFixture({ stage: "blocked", status: "blocked" }),
      }),
    );
    const queryClient = createTestQueryClient();

    const { unmount } = renderHook(() => useAgentWorkspaceOperationToasts(), {
      wrapper: wrapper(queryClient),
    });
    await waitFor(() => expect(toastErrorMock).toHaveBeenCalledTimes(1));
    await dismissLastToast(toastErrorMock);
    unmount();

    // Simulate an app restart: drop in-memory caches, keep localStorage.
    rehydrateAgentWorkspaceOperationRegistryForTests();
    rehydrateAgentWorkspaceOperationDismissalsForTests();
    toastErrorMock.mockClear();
    toastLoadingMock.mockClear();

    const queryClient2 = createTestQueryClient();
    renderHook(() => useAgentWorkspaceOperationToasts(), { wrapper: wrapper(queryClient2) });

    await waitFor(() => expect(getAgentConversationWorkspaceMock).toHaveBeenCalled());
    await flush();
    expect(toastErrorMock).not.toHaveBeenCalled();
    expect(toastLoadingMock).not.toHaveBeenCalled();
  });

  it("6. shows a fresh toast for a new generation of the same operation after a dismissal", async () => {
    seedWatchedEntry();
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspaceFixture({
        maintenanceOperation: maintenanceOperationFixture({
          generation: 1,
          stage: "blocked",
          status: "blocked",
        }),
      }),
    );
    const queryClient = createTestQueryClient();

    renderHook(() => useAgentWorkspaceOperationToasts(), { wrapper: wrapper(queryClient) });
    await waitFor(() => expect(toastErrorMock).toHaveBeenCalledTimes(1));
    await dismissLastToast(toastErrorMock);
    toastErrorMock.mockClear();

    await act(async () => {
      queryClient.setQueryData(
        agentWorkspaceKeys.workspace(CONVERSATION_ID),
        conversationWorkspaceFixture({
          maintenanceOperation: maintenanceOperationFixture({
            generation: 2,
            stage: "blocked",
            status: "blocked",
          }),
        }),
      );
      await Promise.resolve();
    });

    await waitFor(() => expect(toastErrorMock).toHaveBeenCalledTimes(1));
    render(lastCallContent(toastErrorMock));
    expect(screen.getByText("Repair blocked")).toBeInTheDocument();
  });

  it("7. announces a session-reported result exactly once and lets the idle settle path leave the terminal toast alone", async () => {
    seedWatchedEntry({ kind: "publish", publishAttemptKey: "attempt-1" });
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspaceFixture());
    reportAgentWorkspaceOperationResult(CONVERSATION_ID, {
      kind: "success",
      workspace: conversationWorkspaceFixture({ publicationPrNumber: 42 }),
    });
    const queryClient = createTestQueryClient();

    renderHook(() => useAgentWorkspaceOperationToasts(), { wrapper: wrapper(queryClient) });

    await waitFor(() => expect(toastSuccessMock).toHaveBeenCalledTimes(1));
    render(lastCallContent(toastSuccessMock));
    expect(screen.getByText("Published #42")).toBeInTheDocument();

    toastSuccessMock.mockClear();
    const publishToastId = agentWorkspaceOperationToastId(CONVERSATION_ID, "publish");
    await act(async () => {
      queryClient.setQueryData(
        agentWorkspaceKeys.workspace(CONVERSATION_ID),
        conversationWorkspaceFixture(),
      );
      await Promise.resolve();
    });
    expect(toastSuccessMock).not.toHaveBeenCalled();
    // The following idle decision (durable state now reports nothing in
    // flight) must unwatch the conversation without tearing down the
    // terminal toast it just announced — that toast owns a finite duration
    // and settles on its own.
    expect(toastDismissMock).not.toHaveBeenCalledWith(publishToastId);
    await waitFor(() => expect(getWatchedAgentWorkspaceOperations()).toHaveLength(0));
  });

  it("8. settles and unwatches after three consecutive workspace fetch failures instead of spinning forever", async () => {
    seedWatchedEntry();
    getAgentConversationWorkspaceMock.mockRejectedValue(new Error("network down"));
    const queryClient = createTestQueryClient();
    const queryKey = agentWorkspaceKeys.workspace(CONVERSATION_ID);

    renderHook(() => useAgentWorkspaceOperationToasts(), { wrapper: wrapper(queryClient) });
    // Each refetch must wait for the previous fetch attempt to fully settle
    // (via its own errorUpdateCount bump) before starting the next one, or a
    // still-in-flight fetch dedupes with the new refetch call instead of
    // producing a distinct consecutive failure.
    await waitFor(() => expect(queryClient.getQueryState(queryKey)?.errorUpdateCount).toBe(1));

    for (let attempt = 2; attempt <= 3; attempt += 1) {
      await act(async () => {
        await queryClient.refetchQueries({ queryKey, exact: true }).catch(() => undefined);
      });
      if (attempt < 3) {
        await waitFor(() =>
          expect(queryClient.getQueryState(queryKey)?.errorUpdateCount).toBe(attempt),
        );
      }
    }

    await waitFor(() => expect(getWatchedAgentWorkspaceOperations()).toHaveLength(0));
    expect(toastLoadingMock).not.toHaveBeenCalled();
    expect(toastSuccessMock).not.toHaveBeenCalled();
    expect(toastErrorMock).not.toHaveBeenCalled();
    expect(toastInfoMock).not.toHaveBeenCalled();
  });

  it("9. renders a blocked error toast exactly once across multiple poll ticks", async () => {
    seedWatchedEntry();
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspaceFixture({
        maintenanceOperation: maintenanceOperationFixture({ stage: "blocked", status: "blocked" }),
      }),
    );
    const queryClient = createTestQueryClient();

    renderHook(() => useAgentWorkspaceOperationToasts(), { wrapper: wrapper(queryClient) });
    await waitFor(() => expect(toastErrorMock).toHaveBeenCalledTimes(1));

    for (let tick = 0; tick < 3; tick += 1) {
      await act(async () => {
        queryClient.setQueryData(
          agentWorkspaceKeys.workspace(CONVERSATION_ID),
          conversationWorkspaceFixture({
            maintenanceOperation: maintenanceOperationFixture({
              stage: "blocked",
              status: "blocked",
            }),
          }),
        );
        await Promise.resolve();
      });
    }

    expect(toastErrorMock).toHaveBeenCalledTimes(1);
  });

  it("10. does not re-announce a standing terminal state that is already marked announced", async () => {
    seedWatchedEntry();
    markAgentWorkspaceOperationAnnounced(CONVERSATION_ID, "repair:op-1:1:blocked:none");
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspaceFixture({
        maintenanceOperation: maintenanceOperationFixture({ stage: "blocked", status: "blocked" }),
      }),
    );
    const queryClient = createTestQueryClient();

    renderHook(() => useAgentWorkspaceOperationToasts(), { wrapper: wrapper(queryClient) });
    await waitFor(() => expect(getAgentConversationWorkspaceMock).toHaveBeenCalled());
    await flush();

    expect(toastErrorMock).not.toHaveBeenCalled();
    expect(toastLoadingMock).not.toHaveBeenCalled();
  });

  it("11. renders a progress toast again when a held operation returns to active", async () => {
    seedWatchedEntry();
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspaceFixture({
        maintenanceOperation: maintenanceOperationFixture({
          stage: "held",
          status: "held",
          holdReason: "health_evidence",
        }),
      }),
    );
    const queryClient = createTestQueryClient();

    renderHook(() => useAgentWorkspaceOperationToasts(), { wrapper: wrapper(queryClient) });
    await waitFor(() => expect(toastInfoMock).toHaveBeenCalledTimes(1));

    await act(async () => {
      queryClient.setQueryData(
        agentWorkspaceKeys.workspace(CONVERSATION_ID),
        conversationWorkspaceFixture({
          maintenanceOperation: maintenanceOperationFixture({
            stage: "repairing",
            status: "active",
            holdReason: null,
          }),
        }),
      );
      await Promise.resolve();
    });

    await waitFor(() => expect(toastLoadingMock).toHaveBeenCalledTimes(1));
    render(lastCallContent(toastLoadingMock));
    expect(screen.getByText("Repairing workspace")).toBeInTheDocument();
  });

  it("12. shows the 'Pushing rebased branch…' progress title for a publish_redrive hold, not the ready copy", async () => {
    seedWatchedEntry();
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspaceFixture({
        maintenanceOperation: maintenanceOperationFixture({
          stage: "ready",
          status: "active",
          holdReason: "publish_redrive",
        }),
      }),
    );
    const queryClient = createTestQueryClient();

    renderHook(() => useAgentWorkspaceOperationToasts(), { wrapper: wrapper(queryClient) });

    await waitFor(() => expect(toastLoadingMock).toHaveBeenCalledTimes(1));
    render(lastCallContent(toastLoadingMock));
    expect(screen.getByText("Pushing rebased branch…")).toBeInTheDocument();
    expect(screen.queryByText("Base updated — ready to publish")).not.toBeInTheDocument();
  });

  it("13. suppresses both progress and terminal toasts while the target conversation is visible", async () => {
    visibleScopeState.visibleConversationId = CONVERSATION_ID;
    seedWatchedEntry();
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspaceFixture({
        maintenanceOperation: maintenanceOperationFixture({ stage: "repairing", status: "active" }),
      }),
    );
    const queryClient = createTestQueryClient();

    renderHook(() => useAgentWorkspaceOperationToasts(), { wrapper: wrapper(queryClient) });
    await waitFor(() => expect(getAgentConversationWorkspaceMock).toHaveBeenCalled());
    await flush();
    expect(toastLoadingMock).not.toHaveBeenCalled();

    await act(async () => {
      queryClient.setQueryData(
        agentWorkspaceKeys.workspace(CONVERSATION_ID),
        conversationWorkspaceFixture({
          maintenanceOperation: maintenanceOperationFixture({ stage: "blocked", status: "blocked" }),
        }),
      );
      await Promise.resolve();
    });
    expect(toastErrorMock).not.toHaveBeenCalled();
  });

  it("13b. visible-conversation suppression writes no durable dismissal, and the toast reappears once the conversation is no longer visible", async () => {
    visibleScopeState.visibleConversationId = CONVERSATION_ID;
    seedWatchedEntry();
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspaceFixture({
        maintenanceOperation: maintenanceOperationFixture({ stage: "repairing", status: "active" }),
      }),
    );
    const queryClient = createTestQueryClient();

    renderHook(() => useAgentWorkspaceOperationToasts(), { wrapper: wrapper(queryClient) });
    await waitFor(() => expect(getAgentConversationWorkspaceMock).toHaveBeenCalled());
    await flush();
    expect(toastLoadingMock).not.toHaveBeenCalled();
    expect(localStorage.getItem(DISMISSALS_STORAGE_KEY)).toBeNull();

    visibleScopeState.visibleConversationId = null;
    await act(async () => {
      queryClient.setQueryData(
        agentWorkspaceKeys.workspace(CONVERSATION_ID),
        conversationWorkspaceFixture({
          maintenanceOperation: maintenanceOperationFixture({ stage: "repairing", status: "active" }),
        }),
      );
      await Promise.resolve();
    });

    await waitFor(() => expect(toastLoadingMock).toHaveBeenCalledTimes(1));
    expect(localStorage.getItem(DISMISSALS_STORAGE_KEY)).toBeNull();
  });

  it("13c. settling an operation writes no durable dismissal", async () => {
    seedWatchedEntry();
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspaceFixture({
        maintenanceOperation: maintenanceOperationFixture({ stage: "repairing", status: "active" }),
      }),
    );
    const queryClient = createTestQueryClient();

    renderHook(() => useAgentWorkspaceOperationToasts(), { wrapper: wrapper(queryClient) });
    await waitFor(() => expect(toastLoadingMock).toHaveBeenCalledTimes(1));

    await act(async () => {
      queryClient.setQueryData(
        agentWorkspaceKeys.workspace(CONVERSATION_ID),
        conversationWorkspaceFixture({ maintenanceOperation: null }),
      );
      await Promise.resolve();
    });

    await waitFor(() => expect(getWatchedAgentWorkspaceOperations()).toHaveLength(0));
    expect(localStorage.getItem(DISMISSALS_STORAGE_KEY)).toBeNull();
  });

  it("14. does not produce a terminal toast for an operation identity that was already dismissed", async () => {
    seedWatchedEntry({ kind: "publish", publishAttemptKey: "attempt-1" });
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspaceFixture({ publicationPushStatus: "pushing" }),
    );
    const queryClient = createTestQueryClient();

    renderHook(() => useAgentWorkspaceOperationToasts(), { wrapper: wrapper(queryClient) });
    await waitFor(() => expect(toastLoadingMock).toHaveBeenCalledTimes(1));
    await dismissLastToast(toastLoadingMock);

    reportAgentWorkspaceOperationResult(CONVERSATION_ID, {
      kind: "success",
      workspace: conversationWorkspaceFixture({ publicationPrNumber: 7 }),
    });

    await act(async () => {
      queryClient.setQueryData(
        agentWorkspaceKeys.workspace(CONVERSATION_ID),
        conversationWorkspaceFixture({ publicationPrNumber: 7 }),
      );
      await Promise.resolve();
    });

    expect(toastSuccessMock).not.toHaveBeenCalled();
  });

  it("15. keeps the live repair progress toast and announces no terminal result when a base update settles with a maintenance operation present", async () => {
    seedWatchedEntry({ kind: "base-update", publishAttemptKey: "attempt-1" });
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspaceFixture({
        maintenanceOperation: maintenanceOperationFixture({ stage: "updating_base", status: "active" }),
      }),
    );
    const queryClient = createTestQueryClient();

    renderHook(() => useAgentWorkspaceOperationToasts(), { wrapper: wrapper(queryClient) });
    await waitFor(() => expect(toastLoadingMock).toHaveBeenCalledTimes(1));
    render(lastCallContent(toastLoadingMock));
    expect(screen.getByText("Updating base")).toBeInTheDocument();

    await act(async () => {
      queryClient.setQueryData(
        agentWorkspaceKeys.workspace(CONVERSATION_ID),
        conversationWorkspaceFixture({
          maintenanceOperation: maintenanceOperationFixture({ stage: "updating_base", status: "active" }),
        }),
      );
      await Promise.resolve();
    });

    expect(toastSuccessMock).not.toHaveBeenCalled();
    expect(toastInfoMock).not.toHaveBeenCalled();
    expect(toastErrorMock).not.toHaveBeenCalled();
  });

  it("16. keeps watching a session-started base-update entry until its pending result is drained, even once durable state shows nothing in flight", async () => {
    seedWatchedEntry({
      kind: "base-update",
      publishAttemptKey: "attempt-1",
      startedAtMs: Date.now(),
    });
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspaceFixture());
    const queryClient = createTestQueryClient();

    renderHook(() => useAgentWorkspaceOperationToasts(), { wrapper: wrapper(queryClient) });
    await waitFor(() => expect(getAgentConversationWorkspaceMock).toHaveBeenCalled());
    await flush();

    // Durable state already shows nothing in flight, but the session that
    // started this operation has not reported its result yet — the entry
    // must stay watched instead of being orphaned.
    expect(getWatchedAgentWorkspaceOperations()).toHaveLength(1);
    expect(toastSuccessMock).not.toHaveBeenCalled();

    reportAgentWorkspaceOperationResult(CONVERSATION_ID, {
      kind: "base-updated",
      targetRef: "origin/main",
    });
    await act(async () => {
      queryClient.setQueryData(
        agentWorkspaceKeys.workspace(CONVERSATION_ID),
        conversationWorkspaceFixture(),
      );
      await Promise.resolve();
    });

    await waitFor(() => expect(toastSuccessMock).toHaveBeenCalledTimes(1));
    render(lastCallContent(toastSuccessMock));
    expect(screen.getByText("Updated from origin/main")).toBeInTheDocument();
    await waitFor(() => expect(getWatchedAgentWorkspaceOperations()).toHaveLength(0));
  });

  it("17. drains a reported result without waiting for the next workspace poll tick", async () => {
    seedWatchedEntry({
      kind: "base-update",
      publishAttemptKey: "attempt-1",
      startedAtMs: Date.now(),
    });
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspaceFixture());
    const queryClient = createTestQueryClient();

    renderHook(() => useAgentWorkspaceOperationToasts(), { wrapper: wrapper(queryClient) });
    await waitFor(() => expect(getAgentConversationWorkspaceMock).toHaveBeenCalled());
    await flush();

    // No query update follows: a poll that returns structurally-equal data in
    // the same millisecond produces no new query result, so the report itself
    // has to wake the driver or the toast stalls until the next poll tick.
    reportAgentWorkspaceOperationResult(CONVERSATION_ID, {
      kind: "base-updated",
      targetRef: "origin/main",
    });

    await waitFor(() => expect(toastSuccessMock).toHaveBeenCalledTimes(1));
    render(lastCallContent(toastSuccessMock));
    expect(screen.getByText("Updated from origin/main")).toBeInTheDocument();
    await waitFor(() => expect(getWatchedAgentWorkspaceOperations()).toHaveLength(0));
  });

  it("18. unwatches a session-started entry once its result grace window elapses with nothing reported", async () => {
    seedWatchedEntry({
      kind: "base-update",
      publishAttemptKey: "attempt-1",
      startedAtMs: Date.now() - 130_000,
    });
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspaceFixture());
    const queryClient = createTestQueryClient();

    renderHook(() => useAgentWorkspaceOperationToasts(), { wrapper: wrapper(queryClient) });

    await waitFor(() => expect(getWatchedAgentWorkspaceOperations()).toHaveLength(0));
    expect(toastSuccessMock).not.toHaveBeenCalled();
  });
});
