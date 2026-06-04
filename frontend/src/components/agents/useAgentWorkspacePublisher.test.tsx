import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { PublishAgentConversationWorkspaceResult } from "@/api/chat";
import { createTestQueryClient } from "@/test/store-utils";

import { conversationFixture, conversationWorkspaceFixture } from "./agentsTestFixtures";
import { useAgentWorkspacePublisher } from "./useAgentWorkspacePublisher";

const {
  getAgentConversationWorkspaceMock,
  publishAgentConversationWorkspaceMock,
  toastErrorMock,
  toastSuccessMock,
} = vi.hoisted(() => ({
  getAgentConversationWorkspaceMock: vi.fn(),
  publishAgentConversationWorkspaceMock: vi.fn(),
  toastErrorMock: vi.fn(),
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
      publishAgentConversationWorkspace: (...args: unknown[]) =>
        publishAgentConversationWorkspaceMock(...args),
    },
  };
});

vi.mock("sonner", () => ({
  toast: {
    error: (...args: unknown[]) => toastErrorMock(...args),
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

describe("useAgentWorkspacePublisher", () => {
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

    const { result } = renderHook(() =>
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
    );

    act(() => {
      void result.current.handlePublishWorkspace("conversation-1");
    });

    await waitFor(() =>
      expect(result.current.publishingConversationId).toBe("conversation-1"),
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

    await waitFor(() => expect(result.current.publishingConversationId).toBeNull());
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
});
