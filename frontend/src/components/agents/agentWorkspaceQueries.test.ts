import { describe, expect, it } from "vitest";

import type {
  AgentWorkspacePrReviewAction,
  AgentWorkspacePrReviewContext,
  AgentWorkspacePrReviewMonitor,
} from "@/api/chat";

import { conversationWorkspaceFixture } from "./agentsTestFixtures";
import { prReviewContextForConversation } from "./agentWorkspaceQueries";

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
    firstReviewCompleted: false,
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
