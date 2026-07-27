import { QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type {
  AgentConversationWorkspaceFreshness,
  AgentConversationWorkspacePublicationEvent,
  PublishAgentConversationWorkspaceResult,
} from "@/api/chat";
import { createTestQueryClient } from "@/test/store-utils";

import {
  agentWorkspaceMaintenanceOperationToastId,
  agentWorkspaceOperationToastId,
} from "./agentWorkspaceOperationToast";
import { conversationFixture, conversationWorkspaceFixture } from "./agentsTestFixtures";
import { useAgentWorkspacePublisher } from "./useAgentWorkspacePublisher";

const {
  getAgentConversationWorkspaceMock,
  getAgentConversationWorkspaceFreshnessMock,
  listAgentConversationWorkspacePublicationEventsMock,
  publishAgentConversationWorkspaceMock,
  toastDismissMock,
  toastErrorMock,
  toastLoadingMock,
  toastInfoMock,
  toastSuccessMock,
} = vi.hoisted(() => ({
  getAgentConversationWorkspaceMock: vi.fn(),
  getAgentConversationWorkspaceFreshnessMock: vi.fn(),
  listAgentConversationWorkspacePublicationEventsMock: vi.fn(),
  publishAgentConversationWorkspaceMock: vi.fn(),
  toastDismissMock: vi.fn(),
  toastErrorMock: vi.fn(),
  toastLoadingMock: vi.fn(),
  toastInfoMock: vi.fn(),
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
      getAgentConversationWorkspaceFreshness: (...args: unknown[]) =>
        getAgentConversationWorkspaceFreshnessMock(...args),
      listAgentConversationWorkspacePublicationEvents: (...args: unknown[]) =>
        listAgentConversationWorkspacePublicationEventsMock(...args),
      publishAgentConversationWorkspace: (...args: unknown[]) =>
        publishAgentConversationWorkspaceMock(...args),
    },
  };
});

vi.mock("sonner", () => ({
  toast: {
    dismiss: (...args: unknown[]) => toastDismissMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
    loading: (...args: unknown[]) => toastLoadingMock(...args),
    info: (...args: unknown[]) => toastInfoMock(...args),
    success: (...args: unknown[]) => toastSuccessMock(...args),
  },
}));

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((promiseResolve, promiseReject) => {
    resolve = promiseResolve;
    reject = promiseReject;
  });
  return { promise, resolve, reject };
}

function publicationEvent(
  overrides: Partial<AgentConversationWorkspacePublicationEvent> = {},
): AgentConversationWorkspacePublicationEvent {
  return {
    id: "event-1",
    conversationId: "conversation-1",
    step: "checking",
    status: "started",
    summary: "Checking workspace",
    classification: null,
    createdAt: new Date().toISOString(),
    ...overrides,
  };
}

function fullFreshness(): AgentConversationWorkspaceFreshness {
  return {
    conversationId: "conversation-1",
    freshnessScope: "full",
    baseRef: "main",
    baseDisplayName: "Project default (main)",
    targetRef: "origin/main",
    capturedBaseCommit: "base-sha",
    targetBaseCommit: "base-sha",
    isBaseAhead: false,
    hasUncommittedChanges: false,
    unpublishedCommitCount: 0,
    remoteRefreshed: true,
    worktreeStatusChecked: true,
    baseStatus: "valid",
    effectiveBaseRef: "main",
    effectiveBaseDisplayName: "Project default (main)",
    baseBlockReason: null,
  };
}

function wrapper(queryClient: ReturnType<typeof createTestQueryClient>) {
  return function TestWrapper({ children }: { children: ReactNode }) {
    return (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
  };
}

describe("useAgentWorkspacePublisher", () => {
  beforeEach(() => {
    getAgentConversationWorkspaceMock.mockReset();
    getAgentConversationWorkspaceFreshnessMock.mockReset();
    listAgentConversationWorkspacePublicationEventsMock.mockReset();
    publishAgentConversationWorkspaceMock.mockReset();
    toastDismissMock.mockClear();
    toastErrorMock.mockClear();
    toastLoadingMock.mockClear();
    toastInfoMock.mockClear();
    toastSuccessMock.mockClear();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("clears the publish loading state once the backend publish command resolves", async () => {
    const queryClient = createTestQueryClient();
    const workspace = conversationWorkspaceFixture({
      mode: "edit",
      publicationPushStatus: null,
    });
    const publishedWorkspace = conversationWorkspaceFixture({
      mode: "edit",
      publicationPushStatus: "pushed",
      publicationPrNumber: 204,
    });
    const publishDeferred = deferred<PublishAgentConversationWorkspaceResult>();
    const projectInvalidationDeferred = deferred<unknown>();
    const setQueryDataSpy = vi.spyOn(queryClient, "setQueryData");
    publishAgentConversationWorkspaceMock.mockReturnValue(publishDeferred.promise);
    listAgentConversationWorkspacePublicationEventsMock.mockResolvedValue([]);
    getAgentConversationWorkspaceMock.mockResolvedValue(workspace);

    const { result } = renderHook(
      () =>
        useAgentWorkspacePublisher({
        activeWorkspace: workspace,
        findConversationById: () =>
          conversationFixture({
            agentMode: "edit",
            title: "Checkout flow fix",
          }),
        invalidateProjectConversations: () => projectInvalidationDeferred.promise,
        optimisticWorkspacesByConversationId: {},
        queryClient,
        selectedConversationId: "conversation-1",
        }),
      { wrapper: wrapper(queryClient) },
    );

    act(() => {
      void result.current.handlePublishWorkspace("conversation-1");
    });

    await waitFor(() =>
      expect(result.current.publishAttemptsByConversationId["conversation-1"]).toEqual(
        expect.objectContaining({ conversationId: "conversation-1" }),
      ),
    );

    await act(async () => {
      publishDeferred.resolve({
        workspace: publishedWorkspace,
        commitSha: "commit-sha",
        pushed: true,
        createdPr: false,
        prNumber: 204,
        prUrl: "https://github.com/aigentive/ralphx.app/pull/204",
      });
      await publishDeferred.promise;
      await Promise.resolve();
    });

    await waitFor(() =>
      expect(
        result.current.publishAttemptsByConversationId["conversation-1"],
      ).toBeUndefined(),
    );
    expect(setQueryDataSpy).toHaveBeenCalledWith(
      ["agents", "conversation-workspace", "conversation-1"],
      publishedWorkspace,
    );
    expect(toastSuccessMock).toHaveBeenCalledWith("Published #204", {
      description: "Checkout flow fix",
      duration: 8_000,
      id: "agent-workspace-operation:conversation-1:publish",
    });
    expect(toastErrorMock).not.toHaveBeenCalled();

    await act(async () => {
      projectInvalidationDeferred.reject(new Error("Background refresh failed"));
      await projectInvalidationDeferred.promise.catch(() => undefined);
      await Promise.resolve();
    });

    expect(toastErrorMock).not.toHaveBeenCalled();
  });

  it("keeps the publish toast attached to an active durable maintenance operation", async () => {
    const queryClient = createTestQueryClient();
    const workspace = conversationWorkspaceFixture({ mode: "edit" });
    const repairingWorkspace = conversationWorkspaceFixture({
      mode: "edit",
      maintenanceOperation: {
        operationId: "maintenance-1",
        generation: 1,
        source: "publish",
        stage: "repairing",
        status: "active",
        summary: "Resolving the base conflict",
        blocker: null,
        automaticContinuation: true,
        startedAt: "2026-07-25T10:00:00Z",
        updatedAt: "2026-07-25T10:01:00Z",
      },
    });
    listAgentConversationWorkspacePublicationEventsMock.mockResolvedValue([]);
    getAgentConversationWorkspaceMock.mockResolvedValue(repairingWorkspace);
    publishAgentConversationWorkspaceMock.mockResolvedValue({
      workspace: repairingWorkspace,
      commitSha: null,
      pushed: false,
      createdPr: false,
      prNumber: null,
      prUrl: null,
    });

    const { result } = renderHook(
      () =>
        useAgentWorkspacePublisher({
          activeWorkspace: workspace,
          findConversationById: () => conversationFixture(),
          invalidateProjectConversations: () => Promise.resolve(),
          optimisticWorkspacesByConversationId: {},
          queryClient,
          selectedConversationId: "conversation-1",
        }),
      { wrapper: wrapper(queryClient) },
    );

    act(() => {
      void result.current.handlePublishWorkspace("conversation-1");
    });

    await waitFor(() =>
      expect(toastLoadingMock).toHaveBeenLastCalledWith(
        "Repairing workspace",
        expect.objectContaining({
          id: agentWorkspaceMaintenanceOperationToastId(
            "conversation-1",
            "maintenance-1",
          ),
        }),
      ),
    );
    expect(result.current.publishAttemptsByConversationId["conversation-1"]).toEqual(
      expect.objectContaining({ conversationId: "conversation-1" }),
    );
    expect(toastErrorMock).not.toHaveBeenCalled();
  });

  it("settles a direct publish response when a terminal PR accompanies stale active maintenance", async () => {
    const queryClient = createTestQueryClient();
    const workspace = conversationWorkspaceFixture({ mode: "edit" });
    const terminalWorkspace = conversationWorkspaceFixture({
      mode: "edit",
      publicationPrStatus: "merged",
      maintenanceOperation: {
        operationId: "maintenance-1",
        generation: 1,
        source: "publish",
        stage: "repairing",
        status: "active",
        summary: "Stale repair",
        blocker: null,
        automaticContinuation: true,
        startedAt: "2026-07-25T10:00:00Z",
        updatedAt: "2026-07-25T10:01:00Z",
      },
    });
    listAgentConversationWorkspacePublicationEventsMock.mockResolvedValue([]);
    publishAgentConversationWorkspaceMock.mockResolvedValue({
      workspace: terminalWorkspace,
      commitSha: null,
      pushed: false,
      createdPr: false,
      prNumber: null,
      prUrl: null,
    });

    const { result } = renderHook(
      () =>
        useAgentWorkspacePublisher({
          activeWorkspace: workspace,
          findConversationById: () => conversationFixture(),
          invalidateProjectConversations: () => Promise.resolve(),
          optimisticWorkspacesByConversationId: {},
          queryClient,
          selectedConversationId: "conversation-1",
        }),
      { wrapper: wrapper(queryClient) },
    );

    await act(async () => {
      await result.current.handlePublishWorkspace("conversation-1");
    });

    expect(result.current.publishAttemptsByConversationId["conversation-1"]).toBeUndefined();
    expect(toastInfoMock).toHaveBeenCalledWith(
      "Publish stopped because the pull request was merged",
      expect.objectContaining({
        id: agentWorkspaceOperationToastId("conversation-1", "publish"),
      }),
    );
    expect(toastLoadingMock).not.toHaveBeenCalledWith(
      "Repairing workspace",
      expect.anything(),
    );
  });

  it("stops publish polling on a terminal PR before stale active maintenance", async () => {
    const queryClient = createTestQueryClient();
    const baseline = publicationEvent({ id: "baseline" });
    const progress = publicationEvent({
      id: "progress",
      createdAt: new Date(Date.now() + 1_000).toISOString(),
    });
    const terminalWorkspace = conversationWorkspaceFixture({
      publicationPrStatus: "closed",
      maintenanceOperation: {
        operationId: "maintenance-1",
        generation: 1,
        source: "publish",
        stage: "repairing",
        status: "active",
        summary: "Stale repair",
        blocker: null,
        automaticContinuation: true,
        startedAt: "2026-07-25T10:00:00Z",
        updatedAt: "2026-07-25T10:01:00Z",
      },
    });
    listAgentConversationWorkspacePublicationEventsMock.mockResolvedValue([baseline]);
    publishAgentConversationWorkspaceMock.mockReturnValue(new Promise(() => undefined));
    getAgentConversationWorkspaceMock.mockResolvedValue(terminalWorkspace);

    const { result } = renderHook(
      () =>
        useAgentWorkspacePublisher({
          activeWorkspace: conversationWorkspaceFixture(),
          findConversationById: () => conversationFixture(),
          invalidateProjectConversations: () => Promise.resolve(),
          optimisticWorkspacesByConversationId: {},
          queryClient,
          selectedConversationId: "conversation-1",
        }),
      { wrapper: wrapper(queryClient) },
    );

    let completed = false;
    act(() => {
      void result.current.handlePublishWorkspace("conversation-1").then(() => {
        completed = true;
      });
    });
    await waitFor(() => expect(publishAgentConversationWorkspaceMock).toHaveBeenCalled());
    act(() => {
      queryClient.setQueryData(
        ["agents", "conversation-workspace-publication-events", "conversation-1"],
        [baseline, progress],
      );
    });

    await waitFor(() => expect(completed).toBe(true));
    expect(result.current.publishAttemptsByConversationId["conversation-1"]).toBeUndefined();
    expect(toastInfoMock).toHaveBeenCalledWith(
      "Publish stopped because the pull request was closed",
      expect.objectContaining({
        id: agentWorkspaceOperationToastId("conversation-1", "publish"),
      }),
    );
    expect(toastLoadingMock).not.toHaveBeenCalledWith(
      "Repairing workspace",
      expect.anything(),
    );
  });

  it("starts the owned toast before baseline fetch and RPC invocation", async () => {
    const queryClient = createTestQueryClient();
    const baselineDeferred = deferred<AgentConversationWorkspacePublicationEvent[]>();
    listAgentConversationWorkspacePublicationEventsMock.mockReturnValue(
      baselineDeferred.promise,
    );
    publishAgentConversationWorkspaceMock.mockReturnValue(new Promise(() => undefined));

    const { result } = renderHook(
      () =>
        useAgentWorkspacePublisher({
        activeWorkspace: conversationWorkspaceFixture(),
        findConversationById: () =>
          conversationFixture({
            agentMode: "edit",
            title: "Checkout flow fix",
          }),
        invalidateProjectConversations: () => Promise.resolve(),
        optimisticWorkspacesByConversationId: {},
        queryClient,
        selectedConversationId: "conversation-1",
        }),
      { wrapper: wrapper(queryClient) },
    );

    let completion!: Promise<void>;
    act(() => {
      completion = result.current.handlePublishWorkspace("conversation-1");
    });

    expect(toastLoadingMock).toHaveBeenCalledWith(
      "Publishing workspace",
      expect.objectContaining({
        description: expect.stringContaining("Checkout flow fix • Check workspace"),
        id: agentWorkspaceOperationToastId("conversation-1", "publish"),
      }),
    );
    expect(publishAgentConversationWorkspaceMock).not.toHaveBeenCalled();

    await act(async () => {
      baselineDeferred.resolve([]);
      await baselineDeferred.promise;
    });
    await waitFor(() =>
      expect(publishAgentConversationWorkspaceMock).toHaveBeenCalledWith(
        "conversation-1",
      ),
    );
    expect(completion).toBeInstanceOf(Promise);
  });

  it("finalizes from post-baseline durable success while the RPC is unresolved", async () => {
    const queryClient = createTestQueryClient();
    const baseline = publicationEvent({ id: "baseline" });
    const published = publicationEvent({
      id: "published",
      step: "published",
      status: "succeeded",
      summary: "Published pull request",
      createdAt: new Date(Date.now() + 1_000).toISOString(),
    });
    const publishedWorkspace = conversationWorkspaceFixture({
      publicationPushStatus: "pushed",
      publicationPrNumber: 404,
    });
    const firstPublishDeferred = deferred<PublishAgentConversationWorkspaceResult>();
    listAgentConversationWorkspacePublicationEventsMock.mockResolvedValue([baseline]);
    publishAgentConversationWorkspaceMock
      .mockReturnValueOnce(firstPublishDeferred.promise)
      .mockReturnValueOnce(new Promise(() => undefined));
    getAgentConversationWorkspaceMock.mockResolvedValue(publishedWorkspace);
    getAgentConversationWorkspaceFreshnessMock.mockResolvedValue(fullFreshness());

    const { result } = renderHook(
      () =>
        useAgentWorkspacePublisher({
          activeWorkspace: conversationWorkspaceFixture(),
          findConversationById: () =>
            conversationFixture({ title: "Checkout flow fix" }),
          invalidateProjectConversations: () => Promise.resolve(),
          optimisticWorkspacesByConversationId: {},
          queryClient,
          selectedConversationId: "conversation-1",
        }),
      { wrapper: wrapper(queryClient) },
    );

    let completed = false;
    act(() => {
      void result.current.handlePublishWorkspace("conversation-1").then(() => {
        completed = true;
      });
    });
    await waitFor(() =>
      expect(publishAgentConversationWorkspaceMock).toHaveBeenCalled(),
    );

    act(() => {
      queryClient.setQueryData(
        ["agents", "conversation-workspace-publication-events", "conversation-1"],
        [baseline, published],
      );
    });

    await waitFor(() => expect(completed).toBe(true));
    expect(toastSuccessMock).toHaveBeenCalledWith("Published #404", {
      description: "Checkout flow fix",
      duration: 8_000,
      id: agentWorkspaceOperationToastId("conversation-1", "publish"),
    });
    expect(
      result.current.publishAttemptsByConversationId["conversation-1"],
    ).toBeUndefined();

    act(() => {
      void result.current.handlePublishWorkspace("conversation-1");
    });
    await waitFor(() =>
      expect(publishAgentConversationWorkspaceMock).toHaveBeenCalledTimes(2),
    );
    await act(async () => {
      firstPublishDeferred.resolve({
        workspace: conversationWorkspaceFixture({
          publicationPushStatus: "pushed",
          publicationPrNumber: 405,
        }),
        commitSha: "stale-commit",
        pushed: true,
        createdPr: false,
        prNumber: 405,
        prUrl: "https://github.com/aigentive/ralphx.app/pull/405",
      });
      await firstPublishDeferred.promise;
    });
    expect(
      result.current.publishAttemptsByConversationId["conversation-1"],
    ).toEqual(expect.objectContaining({ conversationId: "conversation-1" }));
    expect(toastSuccessMock).toHaveBeenCalledTimes(1);
  });

  it("deduplicates the same conversation while allowing independent attempts", async () => {
    const queryClient = createTestQueryClient();
    listAgentConversationWorkspacePublicationEventsMock.mockResolvedValue([]);
    publishAgentConversationWorkspaceMock.mockReturnValue(new Promise(() => undefined));
    const { result } = renderHook(
      () =>
        useAgentWorkspacePublisher({
          activeWorkspace: conversationWorkspaceFixture(),
          findConversationById: (conversationId) =>
            conversationFixture({ id: conversationId, title: conversationId }),
          invalidateProjectConversations: () => Promise.resolve(),
          optimisticWorkspacesByConversationId: {
            "conversation-2": conversationWorkspaceFixture({
              conversationId: "conversation-2",
            }),
          },
          queryClient,
          selectedConversationId: "conversation-1",
        }),
      { wrapper: wrapper(queryClient) },
    );

    let first!: Promise<void>;
    let duplicate!: Promise<void>;
    act(() => {
      first = result.current.handlePublishWorkspace("conversation-1");
      duplicate = result.current.handlePublishWorkspace("conversation-1");
      void result.current.handlePublishWorkspace("conversation-2");
    });

    expect(duplicate).toBe(first);
    await waitFor(() =>
      expect(Object.keys(result.current.publishAttemptsByConversationId).sort()).toEqual(
        ["conversation-1", "conversation-2"],
      ),
    );
    await waitFor(() =>
      expect(publishAgentConversationWorkspaceMock).toHaveBeenCalledTimes(2),
    );
    expect(toastLoadingMock).toHaveBeenCalledWith(
      "Publishing workspace",
      expect.objectContaining({
        id: agentWorkspaceOperationToastId("conversation-2", "publish"),
      }),
    );
  });

  it("falls back to direct RPC settlement when the baseline read fails", async () => {
    const queryClient = createTestQueryClient();
    const publishedWorkspace = conversationWorkspaceFixture({
      publicationPushStatus: "pushed",
      publicationPrNumber: 501,
    });
    listAgentConversationWorkspacePublicationEventsMock.mockRejectedValue(
      new Error("event read failed"),
    );
    publishAgentConversationWorkspaceMock.mockResolvedValue({
      workspace: publishedWorkspace,
      commitSha: "commit-sha",
      pushed: true,
      createdPr: true,
      prNumber: 501,
      prUrl: "https://github.com/aigentive/ralphx.app/pull/501",
    });
    const { result } = renderHook(
      () =>
        useAgentWorkspacePublisher({
          activeWorkspace: conversationWorkspaceFixture(),
          findConversationById: () => conversationFixture(),
          invalidateProjectConversations: () => Promise.resolve(),
          optimisticWorkspacesByConversationId: {},
          queryClient,
          selectedConversationId: "conversation-1",
        }),
      { wrapper: wrapper(queryClient) },
    );

    await act(async () => {
      await result.current.handlePublishWorkspace("conversation-1");
    });

    expect(toastSuccessMock).toHaveBeenCalledWith("Published #501", {
      description: "Untitled agent",
      duration: 8_000,
      id: agentWorkspaceOperationToastId("conversation-1", "publish"),
    });
    expect(getAgentConversationWorkspaceFreshnessMock).not.toHaveBeenCalled();
  });

  it("settles a durable needs-agent failure before later repair events", async () => {
    const queryClient = createTestQueryClient();
    const baseline = publicationEvent({ id: "baseline" });
    const failure = publicationEvent({
      id: "failure",
      step: "needs_agent",
      status: "failed",
      summary: "Typecheck failed",
      classification: "agent_fixable",
      createdAt: new Date(Date.now() + 1_000).toISOString(),
    });
    const repair = publicationEvent({
      id: "repair",
      step: "repair_sent",
      status: "succeeded",
      createdAt: new Date(Date.now() + 2_000).toISOString(),
    });
    listAgentConversationWorkspacePublicationEventsMock.mockResolvedValue([baseline]);
    publishAgentConversationWorkspaceMock.mockReturnValue(new Promise(() => undefined));
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspaceFixture({ publicationPushStatus: "needs_agent" }),
    );
    const { result } = renderHook(
      () =>
        useAgentWorkspacePublisher({
          activeWorkspace: conversationWorkspaceFixture(),
          findConversationById: () => conversationFixture(),
          invalidateProjectConversations: () => Promise.resolve(),
          optimisticWorkspacesByConversationId: {},
          queryClient,
          selectedConversationId: "conversation-1",
        }),
      { wrapper: wrapper(queryClient) },
    );

    let completed = false;
    act(() => {
      void result.current.handlePublishWorkspace("conversation-1").then(() => {
        completed = true;
      });
    });
    await waitFor(() => expect(publishAgentConversationWorkspaceMock).toHaveBeenCalled());
    act(() => {
      queryClient.setQueryData(
        ["agents", "conversation-workspace-publication-events", "conversation-1"],
        [baseline, failure, repair],
      );
    });

    await waitFor(() => expect(completed).toBe(true));
    expect(toastErrorMock).toHaveBeenCalledWith(
      "Publish failed. Sent the error to the agent to fix.",
      {
        closeButton: true,
        description: "Untitled agent • Typecheck failed",
        dismissible: true,
        duration: 12_000,
        id: agentWorkspaceOperationToastId("conversation-1", "publish"),
      },
    );
    expect(toastSuccessMock).not.toHaveBeenCalled();
  });

  it("does not put verbose durable publish output in the terminal toast", async () => {
    const queryClient = createTestQueryClient();
    const baseline = publicationEvent({ id: "baseline" });
    const failure = publicationEvent({
      id: "failure",
      step: "needs_agent",
      status: "failed",
      summary: [
        "\u001B[1mGuard 1: pre-commit design token guards\u001B[0m",
        "src/components/ui/notice-banner.tsx:21: backgroundColor: var(--status-warning-muted, rgba(224, 179, 65, 0.1))",
        "src/components/automations/automationRunView.ts:497: backgroundColor: var(--status-success-muted, rgba(63, 191, 127, 0.08))",
      ].join("\n"),
      classification: "agent_fixable",
      createdAt: new Date(Date.now() + 1_000).toISOString(),
    });
    listAgentConversationWorkspacePublicationEventsMock.mockResolvedValue([baseline]);
    publishAgentConversationWorkspaceMock.mockReturnValue(new Promise(() => undefined));
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspaceFixture({ publicationPushStatus: "needs_agent" }),
    );
    const { result } = renderHook(
      () =>
        useAgentWorkspacePublisher({
          activeWorkspace: conversationWorkspaceFixture(),
          findConversationById: () => conversationFixture(),
          invalidateProjectConversations: () => Promise.resolve(),
          optimisticWorkspacesByConversationId: {},
          queryClient,
          selectedConversationId: "conversation-1",
        }),
      { wrapper: wrapper(queryClient) },
    );

    act(() => {
      void result.current.handlePublishWorkspace("conversation-1");
    });
    await waitFor(() => expect(publishAgentConversationWorkspaceMock).toHaveBeenCalled());
    act(() => {
      queryClient.setQueryData(
        ["agents", "conversation-workspace-publication-events", "conversation-1"],
        [baseline, failure],
      );
    });

    await waitFor(() =>
      expect(toastErrorMock).toHaveBeenCalledWith(
        "Publish failed. Sent the error to the agent to fix.",
        {
          closeButton: true,
          description: "Untitled agent • Full output is available in the workspace.",
          dismissible: true,
          duration: 12_000,
          id: agentWorkspaceOperationToastId("conversation-1", "publish"),
        },
      ),
    );
  });
});
