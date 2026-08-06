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
  type AgentWorkspaceOperationResult,
  agentWorkspaceOperationResultView,
  createAgentWorkspacePublishAttempt,
  readAgentWorkspaceDurablePublishResult,
} from "./agentWorkspacePublishAttempt";
import { conversationFixture, conversationWorkspaceFixture } from "./agentsTestFixtures";
import { useAgentWorkspacePublisher } from "./useAgentWorkspacePublisher";

const {
  getAgentConversationWorkspaceMock,
  getAgentConversationWorkspaceFreshnessMock,
  listAgentConversationWorkspacePublicationEventsMock,
  publishAgentConversationWorkspaceMock,
  watchAgentWorkspaceOperationMock,
  reportAgentWorkspaceOperationResultMock,
} = vi.hoisted(() => ({
  getAgentConversationWorkspaceMock: vi.fn(),
  getAgentConversationWorkspaceFreshnessMock: vi.fn(),
  listAgentConversationWorkspacePublicationEventsMock: vi.fn(),
  publishAgentConversationWorkspaceMock: vi.fn(),
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
      getAgentConversationWorkspaceFreshness: (...args: unknown[]) =>
        getAgentConversationWorkspaceFreshnessMock(...args),
      listAgentConversationWorkspacePublicationEvents: (...args: unknown[]) =>
        listAgentConversationWorkspacePublicationEventsMock(...args),
      publishAgentConversationWorkspace: (...args: unknown[]) =>
        publishAgentConversationWorkspaceMock(...args),
    },
  };
});

vi.mock("./agentWorkspaceOperationRegistry", () => ({
  watchAgentWorkspaceOperation: (...args: unknown[]) =>
    watchAgentWorkspaceOperationMock(...args),
  reportAgentWorkspaceOperationResult: (...args: unknown[]) =>
    reportAgentWorkspaceOperationResultMock(...args),
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
    attemptId: null,
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
    watchAgentWorkspaceOperationMock.mockClear();
    reportAgentWorkspaceOperationResultMock.mockClear();
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

    expect(watchAgentWorkspaceOperationMock).toHaveBeenCalledWith({
      conversationId: "conversation-1",
      projectId: "project-1",
      conversationTitle: "Checkout flow fix",
      kind: "publish",
      startedAtMs: expect.any(Number),
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
    expect(reportAgentWorkspaceOperationResultMock).toHaveBeenCalledWith(
      "conversation-1",
      { kind: "success", workspace: publishedWorkspace },
    );

    await act(async () => {
      projectInvalidationDeferred.reject(new Error("Background refresh failed"));
      await projectInvalidationDeferred.promise.catch(() => undefined);
      await Promise.resolve();
    });

    expect(reportAgentWorkspaceOperationResultMock).toHaveBeenCalledTimes(1);
  });

  it("keeps the publish attempt open and watches an active durable maintenance operation", async () => {
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
      expect(watchAgentWorkspaceOperationMock).toHaveBeenCalledWith({
        conversationId: "conversation-1",
        projectId: "project-1",
        conversationTitle: null,
        kind: "observed",
        startedAtMs: null,
      }),
    );
    expect(result.current.publishAttemptsByConversationId["conversation-1"]).toEqual(
      expect.objectContaining({ conversationId: "conversation-1" }),
    );
    expect(reportAgentWorkspaceOperationResultMock).not.toHaveBeenCalled();
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
    expect(reportAgentWorkspaceOperationResultMock).toHaveBeenCalledWith(
      "conversation-1",
      { kind: "terminal", status: "merged" },
    );
    expect(watchAgentWorkspaceOperationMock).toHaveBeenCalledTimes(1);
    expect(watchAgentWorkspaceOperationMock).toHaveBeenCalledWith(
      expect.objectContaining({ kind: "publish" }),
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
    expect(reportAgentWorkspaceOperationResultMock).toHaveBeenCalledWith(
      "conversation-1",
      { kind: "terminal", status: "closed" },
    );
    expect(watchAgentWorkspaceOperationMock).toHaveBeenCalledTimes(1);
    expect(watchAgentWorkspaceOperationMock).toHaveBeenCalledWith(
      expect.objectContaining({ kind: "publish" }),
    );
  });

  it("watches a durable publish operation before baseline fetch and RPC invocation", async () => {
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

    expect(watchAgentWorkspaceOperationMock).toHaveBeenCalledWith({
      conversationId: "conversation-1",
      projectId: "project-1",
      conversationTitle: "Checkout flow fix",
      kind: "publish",
      startedAtMs: expect.any(Number),
    });
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
      id: "metadata-settled",
      step: "metadata_settled",
      status: "succeeded",
      summary: "PR metadata update was applied",
      classification: "applied",
      attemptId: "attempt-current",
      createdAt: new Date(Date.now() + 1_000).toISOString(),
    });
    const stalePublished = publicationEvent({
      id: "metadata-settled-stale",
      step: "metadata_settled",
      status: "succeeded",
      summary: "Stale PR metadata update",
      classification: "applied",
      attemptId: "attempt-stale",
      createdAt: new Date(Date.now() + 500).toISOString(),
    });
    const publishedWorkspace = conversationWorkspaceFixture({
      publicationPushStatus: "pushed",
      publicationPrNumber: 404,
      publicationMetadataAttemptId: "attempt-current",
      publicationMetadataPhase: "settled",
      publicationMetadataState: "reconciled",
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
        [baseline, stalePublished, published],
      );
    });

    await waitFor(() => expect(completed).toBe(true));
    expect(reportAgentWorkspaceOperationResultMock).toHaveBeenCalledWith(
      "conversation-1",
      { kind: "success", workspace: publishedWorkspace },
    );
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
    expect(reportAgentWorkspaceOperationResultMock).toHaveBeenCalledTimes(1);
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
    expect(watchAgentWorkspaceOperationMock).toHaveBeenCalledWith(
      expect.objectContaining({ conversationId: "conversation-2", kind: "publish" }),
    );
    expect(watchAgentWorkspaceOperationMock).toHaveBeenCalledWith(
      expect.objectContaining({ conversationId: "conversation-1", kind: "publish" }),
    );
    expect(watchAgentWorkspaceOperationMock).toHaveBeenCalledTimes(2);
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

    expect(reportAgentWorkspaceOperationResultMock).toHaveBeenCalledWith(
      "conversation-1",
      { kind: "success", workspace: publishedWorkspace },
    );
    expect(getAgentConversationWorkspaceFreshnessMock).not.toHaveBeenCalled();
  });

  it("keeps the local attempt open when a direct RPC result still has an unsettled receipt", async () => {
    const queryClient = createTestQueryClient();
    const reconcilingWorkspace = conversationWorkspaceFixture({
      publicationPushStatus: "pushing",
      publicationMetadataAttemptId: "attempt-current",
      publicationMetadataPhase: "reconciling",
      publicationMetadataState: "unknown",
    });
    listAgentConversationWorkspacePublicationEventsMock.mockResolvedValue([]);
    publishAgentConversationWorkspaceMock.mockResolvedValue({
      workspace: reconcilingWorkspace,
      commitSha: "commit-sha",
      pushed: false,
      createdPr: false,
      prNumber: 404,
      prUrl: "https://github.com/aigentive/ralphx.app/pull/404",
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

    act(() => {
      void result.current.handlePublishWorkspace("conversation-1");
    });

    await waitFor(() =>
      expect(publishAgentConversationWorkspaceMock).toHaveBeenCalledWith(
        "conversation-1",
      ),
    );
    await waitFor(() =>
      expect(
        result.current.publishAttemptsByConversationId["conversation-1"],
      ).toEqual(expect.objectContaining({ conversationId: "conversation-1" })),
    );
    expect(reportAgentWorkspaceOperationResultMock).not.toHaveBeenCalled();
  });

  it("keeps the local attempt open when the RPC errors while its receipt is unsettled", async () => {
    const queryClient = createTestQueryClient();
    const reconcilingWorkspace = conversationWorkspaceFixture({
      publicationPushStatus: "pushing",
      publicationMetadataAttemptId: "attempt-current",
      publicationMetadataPhase: "reconciling",
      publicationMetadataState: "unknown",
    });
    listAgentConversationWorkspacePublicationEventsMock.mockResolvedValue([]);
    publishAgentConversationWorkspaceMock.mockRejectedValue(
      new Error("PR metadata outcome is unknown"),
    );
    getAgentConversationWorkspaceMock.mockResolvedValue(reconcilingWorkspace);

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

    await waitFor(() =>
      expect(getAgentConversationWorkspaceMock).toHaveBeenCalledWith(
        "conversation-1",
      ),
    );
    expect(
      result.current.publishAttemptsByConversationId["conversation-1"],
    ).toEqual(expect.objectContaining({ conversationId: "conversation-1" }));
    expect(reportAgentWorkspaceOperationResultMock).not.toHaveBeenCalled();
  });

  it("reattaches an errored publish request to the refreshed maintenance operation", async () => {
    const queryClient = createTestQueryClient();
    const repairingWorkspace = conversationWorkspaceFixture({
      maintenanceOperation: {
        operationId: "maintenance-after-error",
        generation: 1,
        source: "publish",
        stage: "repairing",
        status: "active",
        summary: "Repairing after publish failed",
        blocker: null,
        automaticContinuation: true,
        startedAt: "2026-07-25T10:00:00Z",
        updatedAt: "2026-07-25T10:01:00Z",
      },
    });
    listAgentConversationWorkspacePublicationEventsMock.mockResolvedValue([]);
    publishAgentConversationWorkspaceMock.mockRejectedValue(new Error("repair routed"));
    getAgentConversationWorkspaceMock.mockResolvedValue(repairingWorkspace);
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

    await waitFor(() =>
      expect(watchAgentWorkspaceOperationMock).toHaveBeenCalledWith({
        conversationId: "conversation-1",
        projectId: "project-1",
        conversationTitle: null,
        kind: "observed",
        startedAtMs: null,
      }),
    );
    expect(result.current.publishAttemptsByConversationId["conversation-1"]).toBeDefined();
    expect(reportAgentWorkspaceOperationResultMock).not.toHaveBeenCalled();
  });

  it("settles an errored publish request when the refreshed PR is terminal", async () => {
    const queryClient = createTestQueryClient();
    listAgentConversationWorkspacePublicationEventsMock.mockResolvedValue([]);
    publishAgentConversationWorkspaceMock.mockRejectedValue(new Error("stale publish error"));
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspaceFixture({ publicationPrStatus: "closed" }),
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

    await act(async () => {
      await result.current.handlePublishWorkspace("conversation-1");
    });

    expect(reportAgentWorkspaceOperationResultMock).toHaveBeenCalledWith(
      "conversation-1",
      { kind: "terminal", status: "closed" },
    );
  });

  it("settles a push failure when an earlier metadata receipt remains settled", async () => {
    const queryClient = createTestQueryClient();
    const baseline = publicationEvent({ id: "baseline" });
    const pushFailure = publicationEvent({
      id: "push-failure",
      step: "failed",
      status: "failed",
      summary: "Remote rejected the branch push",
      classification: "operational",
      attemptId: null,
      createdAt: new Date(Date.now() + 1_000).toISOString(),
    });
    listAgentConversationWorkspacePublicationEventsMock.mockResolvedValue([baseline]);
    publishAgentConversationWorkspaceMock.mockReturnValue(new Promise(() => undefined));
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspaceFixture({
        publicationPushStatus: "failed",
        publicationMetadataAttemptId: "attempt-previous",
        publicationMetadataPhase: "settled",
        publicationMetadataState: "applied",
      }),
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
        [baseline, pushFailure],
      );
    });

    await waitFor(() => expect(completed).toBe(true));
    expect(reportAgentWorkspaceOperationResultMock).toHaveBeenCalledWith(
      "conversation-1",
      { kind: "failure", detail: "Remote rejected the branch push" },
    );
    expect(reportAgentWorkspaceOperationResultMock).toHaveBeenCalledTimes(1);
    expect(
      result.current.publishAttemptsByConversationId["conversation-1"],
    ).toBeUndefined();
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
    expect(reportAgentWorkspaceOperationResultMock).toHaveBeenCalledWith(
      "conversation-1",
      { kind: "needs_agent", detail: "Typecheck failed" },
    );
  });

  it("passes the raw durable classification detail through to the result mailbox unmodified", async () => {
    const queryClient = createTestQueryClient();
    const baseline = publicationEvent({ id: "baseline" });
    const verboseSummary = [
      "[1mGuard 1: pre-commit design token guards[0m",
      "src/components/ui/notice-banner.tsx:21: backgroundColor: var(--status-warning-muted, rgba(224, 179, 65, 0.1))",
      "src/components/automations/automationRunView.ts:497: backgroundColor: var(--status-success-muted, rgba(63, 191, 127, 0.08))",
    ].join("\n");
    const failure = publicationEvent({
      id: "failure",
      step: "needs_agent",
      status: "failed",
      summary: verboseSummary,
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
      expect(reportAgentWorkspaceOperationResultMock).toHaveBeenCalledWith(
        "conversation-1",
        { kind: "needs_agent", detail: verboseSummary },
      ),
    );
  });

  it.each([
    ["ready", "ready", "Ready for explicit publication"],
    ["blocked", "blocked", "Resolve the protected branch"],
  ] as const)(
    "reads a durable %s maintenance result and maps it to the expected view",
    async (status, kind, detail) => {
      const queryClient = createTestQueryClient();
      const attempt = createAgentWorkspacePublishAttempt({
        conversationId: "conversation-1",
        projectId: "project-1",
        startedAtMs: Date.now(),
        token: 1,
      });
      attempt.baseline = { state: "available", lastEventId: null };
      getAgentConversationWorkspaceMock.mockResolvedValue(
        conversationWorkspaceFixture({
          maintenanceOperation: {
            operationId: `operation-${status}`,
            generation: 1,
            source: "base_update",
            stage: status,
            status,
            summary: detail,
            blocker: status === "blocked" ? detail : null,
            automaticContinuation: false,
            startedAt: "2026-07-25T10:00:00Z",
            updatedAt: "2026-07-25T10:01:00Z",
          },
        }),
      );

      const durable = await readAgentWorkspaceDurablePublishResult(
        queryClient,
        attempt,
        [publicationEvent()],
      );
      expect(durable).toEqual({ kind, detail });

      const view = agentWorkspaceOperationResultView(durable!);
      if (status === "ready") {
        expect(view).toEqual({
          tone: "info",
          message: "Base updated — ready to publish",
          detail,
        });
      } else {
        expect(view).toEqual({ tone: "error", message: "Repair blocked", detail });
      }
    },
  );

  it("settles a blocked maintenance operation when the baseline is unmatchable", async () => {
    const queryClient = createTestQueryClient();
    const attempt = createAgentWorkspacePublishAttempt({
      conversationId: "conversation-1",
      projectId: "project-1",
      startedAtMs: Date.now(),
      token: 1,
    });
    attempt.baseline = { state: "available", lastEventId: "missing-baseline" };
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspaceFixture({
        maintenanceOperation: {
          operationId: "operation-blocked-fallback",
          generation: 1,
          source: "base_update",
          stage: "blocked",
          status: "blocked",
          summary: "Repair needs help",
          blocker: "Resolve the protected branch",
          automaticContinuation: false,
          startedAt: "2026-07-25T10:00:00Z",
          updatedAt: "2026-07-25T10:01:00Z",
        },
      }),
    );

    await expect(
      readAgentWorkspaceDurablePublishResult(queryClient, attempt, [publicationEvent()]),
    ).resolves.toEqual({
      kind: "blocked",
      detail: "Resolve the protected branch",
    });
  });
});

describe("agentWorkspaceOperationResultView", () => {
  it("maps every result kind to its tone and message", () => {
    const workspaceWithPr = conversationWorkspaceFixture({ publicationPrNumber: 12 });
    const workspaceWithUrl = conversationWorkspaceFixture({
      publicationPrNumber: null,
      publicationPrUrl: "https://github.com/aigentive/ralphx.app/pull/99",
    });
    const workspaceBranchOnly = conversationWorkspaceFixture({
      publicationPrNumber: null,
      publicationPrUrl: null,
    });

    const cases: Array<
      [AgentWorkspaceOperationResult, { tone: "success" | "error" | "info"; message: string }]
    > = [
      [
        { kind: "success", workspace: workspaceWithPr },
        { tone: "success", message: "Published #12" },
      ],
      [
        { kind: "success", workspace: workspaceWithUrl },
        {
          tone: "success",
          message: `Published ${workspaceWithUrl.publicationPrUrl}`,
        },
      ],
      [
        { kind: "success", workspace: workspaceBranchOnly },
        { tone: "success", message: "Published branch" },
      ],
      [
        { kind: "needs_agent" },
        { tone: "error", message: "Publish failed. Sent the error to the agent to fix." },
      ],
      [{ kind: "failure" }, { tone: "error", message: "Failed to publish branch" }],
      [{ kind: "no_changes" }, { tone: "info", message: "No changes to publish" }],
      [{ kind: "ready" }, { tone: "info", message: "Base updated — ready to publish" }],
      [{ kind: "blocked" }, { tone: "error", message: "Repair blocked" }],
      [
        { kind: "terminal", status: "merged" },
        { tone: "info", message: "Publish stopped because the pull request was merged" },
      ],
      [
        { kind: "terminal", status: "closed" },
        { tone: "info", message: "Publish stopped because the pull request was closed" },
      ],
      [
        { kind: "base-updated", targetRef: "origin/main" },
        { tone: "success", message: "Updated from origin/main" },
      ],
      [
        { kind: "base-already-current", targetRef: "origin/main" },
        { tone: "success", message: "Already current with origin/main" },
      ],
      [
        { kind: "base-update-failed" },
        { tone: "error", message: "Failed to update from base" },
      ],
      [{ kind: "repair-started" }, { tone: "info", message: "Repair started" }],
    ];

    for (const [result, expected] of cases) {
      expect(agentWorkspaceOperationResultView(result)).toEqual({
        ...expected,
        detail: null,
      });
    }
  });

  it("carries a supplied detail string through unchanged", () => {
    expect(
      agentWorkspaceOperationResultView({
        kind: "blocked",
        detail: "Resolve the protected branch",
      }),
    ).toEqual({
      tone: "error",
      message: "Repair blocked",
      detail: "Resolve the protected branch",
    });
  });
});
