import { describe, expect, it } from "vitest";

import type {
  AgentWorkspacePrReviewAction,
  AgentWorkspacePrReviewContext,
} from "@/api/chat";

import {
  getAgentWorkspacePrReviewPresentation,
  shouldPollForPrReviewContext,
} from "./agentWorkspacePrReviewPresentation";
import { conversationWorkspaceFixture } from "./agentsTestFixtures";

const now = "2026-07-20T12:00:00.000Z";

function action(
  overrides: Partial<AgentWorkspacePrReviewAction> = {},
): AgentWorkspacePrReviewAction {
  return {
    id: "action-1",
    conversationId: "conversation-1",
    prNumber: 411,
    headSha: "reviewed-head-a",
    proposedAction: "request_changes",
    summary: "Blocking regression",
    reviewBody: "Please fix it.",
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

function context(
  overrides: Partial<AgentWorkspacePrReviewContext> = {},
): AgentWorkspacePrReviewContext {
  return {
    success: true,
    workspace: conversationWorkspaceFixture({
      conversationId: "conversation-1",
      mode: "review_pr",
      publicationPrNumber: 411,
    }),
    events: [],
    prNumber: 411,
    prUrl: "https://github.com/example/repo/pull/411",
    currentHeadSha: "reviewed-head-a",
    pendingActionHeadStatus: "current",
    health: null,
    reviewFeedback: null,
    monitor: null,
    pendingAction: action(),
    recentActions: [],
    issueCommentEvidence: [],
    ...overrides,
  };
}

describe("getAgentWorkspacePrReviewPresentation", () => {
  it("enables submit only for a verified current action with its Review artifact", () => {
    const current = context({
      monitor: {
        conversationId: "conversation-1",
        projectId: "project-1",
        prNumber: 411,
        status: "awaiting_user",
        monitorEnabled: true,
        autoApproveEnabled: false,
        firstReviewCompleted: true,
        firstActionResolved: false,
        lastSeenHeadSha: "reviewed-head-a",
        lastReviewedHeadSha: "reviewed-head-a",
        lastReviewRunId: "run-1",
        lastReviewOutcome: "request_changes",
        lastSubmittedReviewId: null,
        reviewArtifactId: "artifact-1",
        reviewArtifactHeadSha: "reviewed-head-a",
        reviewArtifactVersion: 1,
        reviewArtifactUpdatedAt: now,
        lastError: null,
        createdAt: now,
        updatedAt: now,
      },
    });

    expect(getAgentWorkspacePrReviewPresentation(current)).toMatchObject({
      headStatus: "current",
      canSubmit: true,
      submitBlockedMessage: null,
      consistencyMessage: null,
    });
    expect(shouldPollForPrReviewContext(current)).toBe(true);
  });

  it("keeps polling a current action until its Review artifact is restored", () => {
    const currentWithoutArtifact = context({ monitor: null });

    expect(
      getAgentWorkspacePrReviewPresentation(currentWithoutArtifact),
    ).toMatchObject({
      headStatus: "current",
      canSubmit: false,
      submitBlockedMessage: "Write the Review for this PR head before submitting.",
    });
    expect(shouldPollForPrReviewContext(currentWithoutArtifact)).toBe(true);
  });

  it.each([
    [
      "stale",
      "PR head changed; a fresh review is required.",
      "Verified current head current-",
    ],
    [
      "unverified",
      "The remote PR head cannot currently be verified.",
      null,
    ],
  ] as const)(
    "keeps a %s action visible but non-submittable and polling",
    (headStatus, blockedMessage, currentHeadCopy) => {
      const value = context({
        currentHeadSha: "current-head-b",
        pendingActionHeadStatus: headStatus,
      });
      const presentation = getAgentWorkspacePrReviewPresentation(value);

      expect(presentation.pendingAction?.id).toBe("action-1");
      expect(presentation.canSubmit).toBe(false);
      expect(presentation.submitBlockedMessage).toBe(blockedMessage);
      if (currentHeadCopy) {
        expect(presentation.headDetail).toContain(currentHeadCopy);
      } else {
        expect(presentation.headDetail).not.toContain("current-head-b");
      }
      expect(shouldPollForPrReviewContext(value)).toBe(true);
    },
  );

  it("identifies awaiting-user monitor/action inconsistency and keeps polling", () => {
    const value = context({
      pendingAction: null,
      pendingActionHeadStatus: null,
      monitor: {
        conversationId: "conversation-1",
        projectId: "project-1",
        prNumber: 411,
        status: "awaiting_user",
        monitorEnabled: true,
        autoApproveEnabled: false,
        firstReviewCompleted: true,
        firstActionResolved: false,
        lastSeenHeadSha: "reviewed-head-a",
        lastReviewedHeadSha: "reviewed-head-a",
        lastReviewRunId: "run-1",
        lastReviewOutcome: "request_changes",
        lastSubmittedReviewId: null,
        reviewArtifactId: "artifact-1",
        reviewArtifactHeadSha: "reviewed-head-a",
        reviewArtifactVersion: 1,
        reviewArtifactUpdatedAt: now,
        lastError: null,
        createdAt: now,
        updatedAt: now,
      },
    });

    expect(getAgentWorkspacePrReviewPresentation(value).consistencyMessage).toBe(
      "The reviewer proposal is temporarily unavailable. RalphX will keep trying to restore it.",
    );
    expect(shouldPollForPrReviewContext(value)).toBe(true);
  });

  it.each(["merged", "closed"] as const)(
    "treats terminal workspace status %s as authoritative over a stale pending action",
    (publicationPrStatus) => {
      const value = context({
        workspace: conversationWorkspaceFixture({
          conversationId: "conversation-1",
          mode: "review_pr",
          publicationPrNumber: 411,
          publicationPrStatus,
        }),
      });

      expect(getAgentWorkspacePrReviewPresentation(value)).toMatchObject({
        pendingAction: null,
        canSubmit: false,
        consistencyMessage: null,
        isTerminal: true,
      });
      expect(shouldPollForPrReviewContext(value)).toBe(false);
    },
  );

  it("treats a terminal monitor as authoritative over stale actionability", () => {
    const value = context({
      monitor: {
        conversationId: "conversation-1",
        projectId: "project-1",
        prNumber: 411,
        status: "terminal",
        monitorEnabled: false,
        autoApproveEnabled: false,
        firstReviewCompleted: true,
        firstActionResolved: false,
        lastSeenHeadSha: "reviewed-head-a",
        lastReviewedHeadSha: "reviewed-head-a",
        lastReviewRunId: "run-1",
        lastReviewOutcome: "merged",
        lastSubmittedReviewId: null,
        reviewArtifactId: "artifact-1",
        reviewArtifactHeadSha: "reviewed-head-a",
        reviewArtifactVersion: 1,
        reviewArtifactUpdatedAt: now,
        lastError: null,
        createdAt: now,
        updatedAt: now,
      },
    });

    expect(getAgentWorkspacePrReviewPresentation(value)).toMatchObject({
      pendingAction: null,
      canSubmit: false,
      isTerminal: true,
    });
    expect(shouldPollForPrReviewContext(value)).toBe(false);
  });
});
