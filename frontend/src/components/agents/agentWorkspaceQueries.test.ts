import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";

import type {
  AgentWorkspacePrReviewAction,
  AgentWorkspacePrReviewContext,
  AgentWorkspacePrReviewMonitor,
} from "@/api/chat";

import { conversationWorkspaceFixture } from "./agentsTestFixtures";
import {
  agentWorkspaceKeys,
  canInspectAgentWorkspaceFreshness,
  invalidateWorkspaceQueries,
  refreshWorkspaceReviewContext,
  prReviewContextForConversation,
  resolveWorkspaceReviewOwnerConversationId,
} from "./agentWorkspaceQueries";

describe("canInspectAgentWorkspaceFreshness", () => {
  it("inspects active plan workspaces but keeps missing workspaces ineligible", () => {
    expect(
      canInspectAgentWorkspaceFreshness(
        conversationWorkspaceFixture({ mode: "plan" }),
      ),
    ).toBe(true);
    expect(
      canInspectAgentWorkspaceFreshness(
        conversationWorkspaceFixture({ mode: "plan", status: "missing" }),
      ),
    ).toBe(false);
  });

  it("does not inspect Git freshness while a durable mutation is active", () => {
    expect(
      canInspectAgentWorkspaceFreshness(
        conversationWorkspaceFixture({
          maintenanceOperation: {
            operationId: "maintenance-1",
            generation: 1,
            source: "base_update",
            stage: "validating",
            status: "active",
            summary: "Validating the repair",
            blocker: null,
            automaticContinuation: true,
            startedAt: now,
            updatedAt: now,
          },
        }),
      ),
    ).toBe(false);
  });

  it("keeps Workspace Review inspectable while its durable operation is active", () => {
    expect(
      canInspectAgentWorkspaceFreshness(
        conversationWorkspaceFixture({
          maintenanceOperation: {
            operationId: "maintenance-1",
            generation: 1,
            source: "base_update",
            stage: "reviewing",
            status: "active",
            summary: "Waiting for Workspace Review",
            blocker: null,
            automaticContinuation: true,
            startedAt: now,
            updatedAt: now,
          },
        }),
      ),
    ).toBe(true);
  });
});

const now = "2026-06-18T12:00:00.000Z";

function monitor(
  overrides: Partial<AgentWorkspacePrReviewMonitor> = {},
): AgentWorkspacePrReviewMonitor {
  return {
    conversationId: "conversation-1",
    projectId: "project-1",
    prNumber: 471,
    status: "awaiting_user",
    monitorEnabled: true,
    autoApproveEnabled: true,
    firstReviewCompleted: false,
    firstActionResolved: false,
    lastSeenHeadSha: "abcdef1234567890",
    lastReviewedHeadSha: null,
    lastReviewRunId: null,
    lastReviewOutcome: null,
    lastSubmittedReviewId: null,
    reviewArtifactId: "review-artifact-1",
    reviewArtifactHeadSha: "abcdef1234567890",
    reviewArtifactVersion: 1,
    reviewArtifactUpdatedAt: now,
    lastError: null,
    createdAt: now,
    updatedAt: now,
    ...overrides,
  };
}

function reviewAction(
  overrides: Partial<AgentWorkspacePrReviewAction> = {},
): AgentWorkspacePrReviewAction {
  return {
    id: "action-1",
    conversationId: "conversation-1",
    prNumber: 471,
    headSha: "abcdef1234567890",
    proposedAction: "request_changes",
    summary: "Found a blocking regression in the PR.",
    reviewBody: "Please fix the regression before merge.",
    findingsJson: null,
    status: "pending",
    submittedReviewId: null,
    createdByRunId: "run-1",
    createdAt: now,
    updatedAt: now,
    resolvedAt: null,
    ...overrides,
  };
}

function reviewContext(
  overrides: Partial<AgentWorkspacePrReviewContext> = {},
): AgentWorkspacePrReviewContext {
  return {
    success: true,
    workspace: conversationWorkspaceFixture({
      conversationId: "conversation-1",
      mode: "review_pr",
      publicationPrNumber: 471,
      publicationPrUrl: "https://github.com/aigentive/ralphx.app/pull/471",
    }),
    events: [],
    prNumber: 471,
    prUrl: "https://github.com/aigentive/ralphx.app/pull/471",
    currentHeadSha: "abcdef1234567890",
    pendingActionHeadStatus: "current",
    health: null,
    reviewFeedback: null,
    monitor: monitor(),
    pendingAction: reviewAction(),
    recentActions: [reviewAction({ id: "recent-action" })],
    issueCommentEvidence: [],
    ...overrides,
  };
}

describe("prReviewContextForConversation", () => {
  it("returns null without a context or active conversation id", () => {
    expect(prReviewContextForConversation(null, "conversation-1")).toBeNull();
    expect(prReviewContextForConversation(reviewContext(), null)).toBeNull();
  });

  it("keeps review context when every conversation-owned record matches", () => {
    const context = reviewContext();

    expect(prReviewContextForConversation(context, "conversation-1")).toBe(
      context,
    );
  });

  it("drops review context when the workspace belongs to another conversation", () => {
    const context = reviewContext({
      workspace: conversationWorkspaceFixture({
        conversationId: "conversation-2",
        mode: "review_pr",
      }),
    });

    expect(prReviewContextForConversation(context, "conversation-1")).toBeNull();
  });

  it("drops review context when the monitor belongs to another conversation", () => {
    const context = reviewContext({
      monitor: monitor({ conversationId: "conversation-2" }),
    });

    expect(prReviewContextForConversation(context, "conversation-1")).toBeNull();
  });

  it("drops review context when the pending action belongs to another conversation", () => {
    const context = reviewContext({
      pendingAction: reviewAction({ conversationId: "conversation-2" }),
    });

    expect(prReviewContextForConversation(context, "conversation-1")).toBeNull();
  });

  it("drops review context when any recent action belongs to another conversation", () => {
    const context = reviewContext({
      recentActions: [
        reviewAction({ id: "matching-action" }),
        reviewAction({
          id: "stale-action",
          conversationId: "conversation-2",
        }),
      ],
    });

    expect(prReviewContextForConversation(context, "conversation-1")).toBeNull();
  });
});

describe("resolveWorkspaceReviewOwnerConversationId", () => {
  it("keeps the selected workspace as Review owner when it also has a parent conversation", () => {
    expect(
      resolveWorkspaceReviewOwnerConversationId({
        activeConversationContextType: "project",
        activeConversationId: "selected-workspace-conversation",
        activeConversationParentId: "parent-conversation",
        activeConversationMode: "edit",
        activeWorkspaceConversationId: "selected-workspace-conversation",
      }),
    ).toBe("selected-workspace-conversation");
  });

  it("uses the parent owner for a project child conversation without its own workspace", () => {
    expect(
      resolveWorkspaceReviewOwnerConversationId({
        activeConversationContextType: "project",
        activeConversationId: "review-child-conversation",
        activeConversationParentId: "parent-workspace-conversation",
        activeConversationMode: null,
        activeWorkspaceConversationId: null,
      }),
    ).toBe("parent-workspace-conversation");
  });

  it("rejects PLAN workspaces without falling back to their parent", () => {
    expect(
      resolveWorkspaceReviewOwnerConversationId({
        activeConversationContextType: "project",
        activeConversationId: "workspace-conversation",
        activeConversationParentId: "parent-workspace-conversation",
        activeConversationMode: "plan",
        activeWorkspaceConversationId: "workspace-conversation",
      }),
    ).toBeNull();
  });

  it("rejects Review PR workspaces without falling back to their parent", () => {
    expect(
      resolveWorkspaceReviewOwnerConversationId({
        activeConversationContextType: "project",
        activeConversationId: "review-pr-conversation",
        activeConversationParentId: "parent-workspace-conversation",
        activeConversationMode: "review_pr",
        activeWorkspaceConversationId: "review-pr-conversation",
      }),
    ).toBeNull();
  });

  it("returns null for non-project conversations", () => {
    expect(
      resolveWorkspaceReviewOwnerConversationId({
        activeConversationContextType: "ideation",
        activeConversationId: "ideation-conversation",
        activeConversationParentId: null,
        activeConversationMode: null,
        activeWorkspaceConversationId: null,
      }),
    ).toBeNull();
  });
});

describe("workspace Review refresh ownership", () => {
  it("keeps broad workspace invalidation away from Review context", async () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");

    await invalidateWorkspaceQueries(queryClient, "conversation-1");

    expect(invalidate).not.toHaveBeenCalledWith({
      queryKey: agentWorkspaceKeys.workspaceReview("conversation-1"),
    });
  });

  it("coalesces raced refreshes and runs one trailing full-target request", async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const requests: Array<{
      refreshTarget: boolean;
      resolve: (value: AgentWorkspaceReviewContext) => void;
    }> = [];
    const fetchContext = vi.fn(
      (
        _conversationId: string,
        options: { refreshTarget?: boolean },
      ) =>
        new Promise<AgentWorkspaceReviewContext>((resolve) => {
          requests.push({
            refreshTarget: options.refreshTarget ?? false,
            resolve,
          });
        }),
    );

    const first = refreshWorkspaceReviewContext(
      queryClient,
      "conversation-1",
      "status",
      fetchContext,
    );
    await vi.waitFor(() => expect(requests).toHaveLength(1));
    const racedStatus = refreshWorkspaceReviewContext(
      queryClient,
      "conversation-1",
      "status",
      fetchContext,
    );
    const racedTarget = refreshWorkspaceReviewContext(
      queryClient,
      "conversation-1",
      "full_target",
      fetchContext,
    );

    expect(requests).toHaveLength(1);
    requests[0]!.resolve({} as AgentWorkspaceReviewContext);
    await vi.waitFor(() => expect(requests).toHaveLength(2));
    expect(requests[1]!.refreshTarget).toBe(true);
    requests[1]!.resolve({} as AgentWorkspaceReviewContext);

    await Promise.all([first, racedStatus, racedTarget]);
    expect(fetchContext).toHaveBeenCalledTimes(2);
  });

  it("does not replace an active interval fetch when a Review signal refresh arrives", async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    queryClient.setQueryData(
      agentWorkspaceKeys.workspaceReview("conversation-1"),
      {} as AgentWorkspaceReviewContext,
    );
    const refetchQueries = vi.spyOn(queryClient, "refetchQueries");
    let resolveIntervalFetch: (value: AgentWorkspaceReviewContext) => void;
    let intervalSignal: AbortSignal | undefined;
    const intervalFetch = queryClient.fetchQuery({
      queryKey: agentWorkspaceKeys.workspaceReview("conversation-1"),
      queryFn: ({ signal }) => {
        intervalSignal = signal;
        return new Promise<AgentWorkspaceReviewContext>((resolve) => {
          resolveIntervalFetch = resolve;
        });
      },
      staleTime: 0,
    });
    const signalRefreshFetch = vi.fn();

    await vi.waitFor(() => expect(intervalSignal).toBeDefined());
    const signalRefresh = refreshWorkspaceReviewContext(
      queryClient,
      "conversation-1",
      "status",
      signalRefreshFetch,
    );

    await Promise.resolve();
    expect(intervalSignal?.aborted).toBe(false);
    expect(signalRefreshFetch).not.toHaveBeenCalled();
    expect(refetchQueries).toHaveBeenLastCalledWith(
      expect.objectContaining({
        exact: true,
        queryKey: agentWorkspaceKeys.workspaceReview("conversation-1"),
      }),
      { cancelRefetch: false },
    );

    resolveIntervalFetch!({} as AgentWorkspaceReviewContext);
    await Promise.all([intervalFetch, signalRefresh]);
  });
});
