import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ComponentProps } from "react";
import { describe, expect, it, vi } from "vitest";

import type {
  AgentWorkspaceReviewContext,
  AgentWorkspaceReviewMonitor,
  AgentWorkspaceReviewTarget,
  StartAgentWorkspaceReviewResult,
} from "@/api/chat";
import type { Artifact } from "@/types/artifact";

import {
  conversationWorkspaceFixture,
  renderWithAgentProviders,
} from "./agentsTestFixtures";
import { AgentReviewPanel } from "./AgentReviewPanel";

vi.mock("@/components/Ideation/PlanDisplay", () => ({
  PlanDisplay: ({ artifactLabel }: { artifactLabel: string }) => (
    <div data-testid="mock-plan-display">{artifactLabel}</div>
  ),
}));

const disabledReason =
  "Review is available after the current agent run finishes.";

const reviewTarget: AgentWorkspaceReviewTarget = {
  scope: "workspace_delta",
  baseRef: "main",
  baseSha: "base-sha",
  headRef: "HEAD",
  headSha: "head-sha",
  diffFingerprint: "diff-fingerprint",
  sourcePullRequestNumber: null,
};

function reviewMonitor(
  overrides: Partial<AgentWorkspaceReviewMonitor> = {},
): AgentWorkspaceReviewMonitor {
  return {
    conversationId: "conversation-1",
    projectId: "project-1",
    status: "ready",
    reviewOutcome: "none",
    reviewGateStatus: "required",
    currentTargetScope: "workspace_delta",
    reviewedTargetScope: "workspace_delta",
    reviewConversationId: "review-conversation-1",
    reviewArtifactId: "review-artifact-1",
    reviewArtifactVersion: 1,
    reviewArtifactUpdatedAt: "2026-07-10T00:00:00.000Z",
    reviewedHeadSha: "previous-head-sha",
    reviewedDiffFingerprint: "previous-diff-fingerprint",
    selectedSourceBaseRef: null,
    selectedSourceBaseSha: null,
    selectedSourceHeadRef: null,
    selectedSourceHeadSha: null,
    selectedSourcePullRequestNumber: null,
    workspaceBaseRef: "main",
    workspaceBaseSha: "base-sha",
    workspaceHeadRef: "HEAD",
    workspaceHeadSha: "head-sha",
    currentDiffFingerprint: reviewTarget.diffFingerprint,
    previousVersionId: null,
    reviewBlockingSummary: null,
    reviewBlockingFingerprint: null,
    reviewFixerRunId: null,
    reviewFixerConversationId: null,
    reviewFixerStatus: null,
    lastRunId: null,
    lastError: null,
    createdAt: "2026-07-10T00:00:00.000Z",
    updatedAt: "2026-07-10T00:00:00.000Z",
    ...overrides,
  };
}

function reviewContext(
  overrides: Partial<AgentWorkspaceReviewContext> = {},
): AgentWorkspaceReviewContext {
  return {
    success: true,
    workspace: conversationWorkspaceFixture(),
    events: [],
    target: reviewTarget,
    monitor: reviewMonitor(),
    isCurrent: false,
    isOutdated: true,
    shouldShowTab: true,
    ...overrides,
  };
}

function reviewStartResult(
  overrides: Partial<StartAgentWorkspaceReviewResult> = {},
): StartAgentWorkspaceReviewResult {
  return {
    success: true,
    target: reviewTarget,
    monitor: reviewMonitor(),
    isCurrent: false,
    isOutdated: true,
    shouldShowTab: true,
    started: false,
    skippedReason: null,
    wasQueued: false,
    ...overrides,
  };
}

function reviewArtifact(): Artifact {
  return {
    id: "review-artifact-1",
    type: "review_feedback",
    name: "Workspace Review",
    content: { type: "inline", text: "Review body" },
    metadata: {
      createdAt: "2026-07-10T00:00:00.000Z",
      createdBy: "reviewer",
      version: 1,
    },
    derivedFrom: [],
  };
}

function renderPanel(
  props: Partial<ComponentProps<typeof AgentReviewPanel>> = {},
) {
  return renderWithAgentProviders(
    <AgentReviewPanel
      reviewArtifact={reviewArtifact()}
      reviewContext={reviewContext()}
      reviewStartResult={null}
      reviewStartError={null}
      isReviewLoading={false}
      isReviewActionPending={false}
      isWorkspaceRuntimeGenerating={false}
      onStartReview={vi.fn()}
      onFixIssues={vi.fn()}
      {...props}
    />,
  );
}

describe("AgentReviewPanel", () => {
  it("keeps runtime-blocked Review reasons in the disabled action tooltip only", async () => {
    const user = userEvent.setup();

    renderPanel({ isWorkspaceRuntimeGenerating: true });

    const action = screen.getByRole("button", { name: "Update review" });
    expect(action).toBeDisabled();
    expect(action).not.toHaveAttribute("aria-describedby");
    expect(
      screen.queryByTestId("agents-review-action-disabled-reason"),
    ).not.toBeInTheDocument();
    expect(screen.queryByText(disabledReason)).not.toBeInTheDocument();

    await user.hover(action.parentElement ?? action);

    expect(await screen.findAllByText(disabledReason)).not.toHaveLength(0);
  });

  it("keeps the outdated Review warning when the action is not runtime-blocked", () => {
    renderPanel();

    expect(screen.getByRole("button", { name: "Update review" })).toBeEnabled();
    expect(screen.getByText("Review is outdated")).toBeInTheDocument();
    expect(
      screen.getByText(/Outdated for current changes\./),
    ).toBeInTheDocument();
  });

  it("does not duplicate conversation-active skipped text beside the disabled action", () => {
    renderPanel({
      isWorkspaceRuntimeGenerating: true,
      reviewStartResult: reviewStartResult({
        skippedReason: "conversation_active",
      }),
    });

    expect(
      screen.queryByText("Review will be available after the current agent run."),
    ).not.toBeInTheDocument();
  });

  it("renders the Review PR Auto Approve switch with an accessible explanation", async () => {
    const user = userEvent.setup();
    const onAutoApproveChange = vi.fn();

    renderPanel({
      isReviewPrWorkspace: true,
      autoApproveEnabled: false,
      onAutoApproveChange,
    });

    const toggle = screen.getByRole("switch", { name: "Auto Approve" });
    expect(toggle).toHaveAttribute("data-state", "unchecked");

    await user.click(toggle);
    expect(onAutoApproveChange).toHaveBeenCalledWith(true);

    await user.hover(
      screen.getByRole("button", { name: "About Auto Approve" }),
    );
    expect(
      await screen.findByRole("tooltip", {
        name: /After you decide the first review/i,
      }),
    ).toBeInTheDocument();
  });

  it("keeps Auto Approve out of non-Review PR Review tabs", () => {
    renderPanel();

    expect(
      screen.queryByTestId("agents-review-pr-auto-approve"),
    ).not.toBeInTheDocument();
  });
});
