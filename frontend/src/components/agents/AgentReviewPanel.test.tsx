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
import { chatApi } from "@/api/chat";
import type { Artifact } from "@/types/artifact";

import {
  conversationWorkspaceFixture,
  renderWithAgentProviders,
} from "./agentsTestFixtures";
import { AgentReviewPanel } from "./AgentReviewPanel";

vi.mock("@/components/Ideation/PlanDisplay", () => ({
  PlanDisplay: ({
    artifactLabel,
    plan,
  }: {
    artifactLabel: string;
    plan: Artifact;
  }) => (
    <div data-testid="mock-plan-display">
      {artifactLabel}: {plan.name}
    </div>
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
    reviewRequestedChangesArtifactId: "review-requested-changes-1",
    reviewRequestedChangesArtifactVersion: 1,
    reviewRequestedChangesArtifactUpdatedAt: "2026-07-10T00:00:00.000Z",
    reviewGateBypassedAt: null,
    reviewGateBypassedTargetScope: null,
    reviewGateBypassedDiffFingerprint: null,
    reviewGateBypassedArtifactId: null,
    reviewGateBypassedArtifactVersion: null,
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
    reviewRequestedChangesPreviousVersionId: null,
    reviewBlockingSummary: null,
    reviewBlockingFingerprint: null,
    reviewFixerRunId: null,
    reviewFixerConversationId: null,
    reviewFixerStatus: null,
    reviewFixerCycleCount: 0,
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
  const reviewArtifactIsCurrent =
    overrides.reviewArtifactIsCurrent ?? overrides.isCurrent ?? false;
  const reviewArtifactIsOutdated =
    overrides.reviewArtifactIsOutdated ?? overrides.isOutdated ?? true;
  return {
    success: true,
    workspace: conversationWorkspaceFixture(),
    events: [],
    target: reviewTarget,
    monitor: reviewMonitor(),
    reviewArtifactIsCurrent,
    reviewArtifactIsOutdated,
    canMutateReviewState: false,
    reviewRuntimeState: "missing_runtime_identity",
    isCurrent: false,
    isOutdated: true,
    shouldShowTab: true,
    ...overrides,
  };
}

it("distinguishes a blocking review authorized by a human bypass", () => {
  renderPanel({
    reviewContext: reviewContext({
      isCurrent: true,
      isOutdated: false,
      monitor: reviewMonitor({
        reviewOutcome: "blocking",
        reviewGateStatus: "passed",
        reviewBlockingSummary: "One unresolved blocker remains.",
        reviewGateBypassedAt: "2026-07-10T00:05:00.000Z",
        reviewGateBypassedTargetScope: "workspace_delta",
        reviewGateBypassedDiffFingerprint: reviewTarget.diffFingerprint,
        reviewGateBypassedArtifactId: "review-artifact-1",
        reviewGateBypassedArtifactVersion: 1,
      }),
    }),
  });

  expect(screen.getByText("Review approved anyway")).toBeInTheDocument();
  expect(screen.getByText("One unresolved blocker remains.")).toBeInTheDocument();
  expect(screen.queryByText("Review passed")).not.toBeInTheDocument();
});

function reviewStartResult(
  overrides: Partial<StartAgentWorkspaceReviewResult> = {},
): StartAgentWorkspaceReviewResult {
  const reviewArtifactIsCurrent =
    overrides.reviewArtifactIsCurrent ?? overrides.isCurrent ?? false;
  const reviewArtifactIsOutdated =
    overrides.reviewArtifactIsOutdated ?? overrides.isOutdated ?? true;
  return {
    success: true,
    target: reviewTarget,
    monitor: reviewMonitor(),
    reviewArtifactIsCurrent,
    reviewArtifactIsOutdated,
    canMutateReviewState: false,
    reviewRuntimeState: "missing_runtime_identity",
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

function requestedChangesArtifact(): Artifact {
  return {
    ...reviewArtifact(),
    id: "review-requested-changes-1",
    name: "Workspace Review — Requested Changes",
    content: { type: "inline", text: "Detailed repair blueprint" },
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
  it("keeps the Review status shell visible while documents load", () => {
    renderPanel({
      reviewArtifact: null,
      reviewRequestedChangesArtifact: null,
      isReviewLoading: true,
    });

    expect(screen.getByText("Review is outdated")).toBeInTheDocument();
    expect(screen.getByText("Loading review...")).toBeInTheDocument();
  });

  it("switches between Overview and Requested Changes artifacts", async () => {
    const user = userEvent.setup();
    renderPanel({
      reviewRequestedChangesArtifact: requestedChangesArtifact(),
    });

    expect(screen.getByTestId("mock-plan-display")).toHaveTextContent(
      "Review: Workspace Review",
    );
    await user.click(
      screen.getByRole("tab", { name: "Requested Changes" }),
    );
    expect(screen.getByTestId("mock-plan-display")).toHaveTextContent(
      "Review: Workspace Review — Requested Changes",
    );
  });

  it("keeps Requested Changes visible with an upgrade state for legacy reviews", async () => {
    const user = userEvent.setup();
    renderPanel({
      reviewRequestedChangesArtifact: null,
      reviewContext: reviewContext({
        monitor: reviewMonitor({
          reviewRequestedChangesArtifactId: null,
          reviewRequestedChangesArtifactVersion: null,
          reviewRequestedChangesArtifactUpdatedAt: null,
        }),
      }),
    });

    await user.click(
      screen.getByRole("tab", { name: "Requested Changes" }),
    );
    expect(
      screen.getByText("Requested Changes not available"),
    ).toBeInTheDocument();
  });

  it("removes the outer artifact padding when embedded in Commit & Publish", () => {
    renderPanel({ embedded: true, reviewArtifact: null });

    const panel = screen.getByTestId("agents-review-panel");
    expect(panel).toHaveAttribute("data-embedded", "true");
    expect(panel).not.toHaveClass("px-4", "pb-4", "pt-4");
  });

  it("opens the Workspace Review transcript without hiding its metadata", async () => {
    const user = userEvent.setup();
    const onViewTranscript = vi.fn();

    renderPanel({ onViewTranscript });

    expect(screen.getByText("Workspace changes")).toBeInTheDocument();
    expect(
      screen.getByText(new Date("2026-07-10T00:00:00.000Z").toLocaleString()),
    ).toBeInTheDocument();

    await user.click(
      screen.getByRole("button", { name: "View transcript" }),
    );

    expect(onViewTranscript).toHaveBeenCalledOnce();
  });

  it("hides the transcript action without a callback", () => {
    renderPanel();

    expect(
      screen.queryByRole("button", { name: "View transcript" }),
    ).not.toBeInTheDocument();
  });

  it("hides the Workspace Review transcript action in Review PR", () => {
    renderPanel({ onViewTranscript: vi.fn(), isReviewPrWorkspace: true });

    expect(
      screen.queryByRole("button", { name: "View transcript" }),
    ).not.toBeInTheDocument();
  });

  it("offers Approve anyway behind confirmation while Fix Issues stays primary", async () => {
    const user = userEvent.setup();
    const onApproveAnyway = vi.fn().mockResolvedValue(undefined);
    renderPanel({
      onApproveAnyway,
      reviewContext: reviewContext({
        isCurrent: true,
        isOutdated: false,
        monitor: reviewMonitor({
          reviewOutcome: "blocking",
          reviewGateStatus: "blocking",
          reviewBlockingSummary: "One unresolved blocker remains.",
        }),
      }),
    });

    expect(screen.getByRole("button", { name: "Fix Issues" })).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "Review actions" }));
    await user.click(screen.getByText("Approve anyway"));

    expect(
      screen.getByText("Approve this blocking Review anyway?"),
    ).toBeInTheDocument();
    expect(onApproveAnyway).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Approve anyway" }));
    expect(onApproveAnyway).toHaveBeenCalledOnce();
  });

  it("keeps the blocking reason visible when automatic fixing reaches its cycle cap", () => {
    renderPanel({
      reviewContext: reviewContext({
        isCurrent: true,
        isOutdated: false,
        monitor: reviewMonitor({
          reviewOutcome: "blocking",
          reviewGateStatus: "blocking",
          reviewBlockingSummary: "One unresolved blocker remains.",
          reviewFixerStatus: "cycle_capped",
          reviewFixerCycleCount: 3,
        }),
      }),
    });

    expect(
      screen.getByText("Automatic fix cycle limit reached"),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "One unresolved blocker remains. This workspace has recorded 3 fixer cycles. Automatic fixing is paused; Fix Issues manually to continue.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Fix Issues" })).toBeEnabled();
    expect(
      screen.getAllByText("Automatic fix cycle limit reached"),
    ).toHaveLength(1);
    expect(
      screen.getByText("Turn Auto Review & Fix off, then on to re-arm the loop with a fresh cycle budget."),
    ).toBeInTheDocument();
  });

  it("shows the Workspace Review-only automation row and writes its explicit override", async () => {
    const user = userEvent.setup();
    const workspace = conversationWorkspaceFixture({
      reviewAutomationOverride: true,
    });
    const update = vi
      .spyOn(chatApi, "setAgentConversationWorkspaceReviewAutomation")
      .mockResolvedValue({ ...workspace, reviewAutomationOverride: false });
    renderPanel({
      reviewContext: reviewContext({
        workspace,
        monitor: reviewMonitor({
          reviewFixerStatus: "running",
          reviewFixerCycleCount: 2,
        }),
      }),
    });

    expect(
      screen.getByTestId("agents-review-auto-review-fix"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("switch", { name: "Auto Review & Fix" }),
    ).toBeChecked();
    expect(
      screen.getByText("Auto Review & Fix · cycle 2 — fixing…"),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("switch", { name: "Auto Review & Fix" }));

    expect(update).toHaveBeenCalledWith(workspace.conversationId, {
      enabled: false,
    });
  });

  it("keeps Auto Review & Fix out of Review PR mode", () => {
    renderPanel({
      isReviewPrWorkspace: true,
      reviewContext: reviewContext({
        workspace: conversationWorkspaceFixture({ reviewAutomationOverride: true }),
      }),
    });

    expect(
      screen.queryByTestId("agents-review-auto-review-fix"),
    ).not.toBeInTheDocument();
  });

  it("shows only the cap detail when its blocking summary is absent", () => {
    renderPanel({
      reviewContext: reviewContext({
        isCurrent: true,
        isOutdated: false,
        monitor: reviewMonitor({
          reviewOutcome: "blocking",
          reviewGateStatus: "blocking",
          reviewBlockingSummary: null,
          reviewFixerStatus: "cycle_capped",
          reviewFixerCycleCount: 3,
        }),
      }),
    });

    expect(
      screen.getByText(
        "This workspace has recorded 3 fixer cycles. Automatic fixing is paused; Fix Issues manually to continue.",
      ),
    ).toBeInTheDocument();
  });

  it("explains that automatic fixing is disabled when the cap is zero", () => {
    renderPanel({
      reviewContext: reviewContext({
        isCurrent: true,
        isOutdated: false,
        monitor: reviewMonitor({
          reviewOutcome: "blocking",
          reviewGateStatus: "blocking",
          reviewFixerStatus: "cycle_capped",
          reviewFixerCycleCount: 0,
        }),
      }),
    });

    expect(
      screen.getByText(
        "Automatic fixes are disabled by the cycle limit. Fix Issues manually to continue.",
      ),
    ).toBeInTheDocument();
  });

  it("cancels cleanly and prevents duplicate approval while confirmation is pending", async () => {
    const user = userEvent.setup();
    let finishApproval: (() => void) | undefined;
    const onApproveAnyway = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          finishApproval = resolve;
        }),
    );
    renderPanel({
      onApproveAnyway,
      reviewContext: reviewContext({
        isCurrent: true,
        isOutdated: false,
        monitor: reviewMonitor({
          reviewOutcome: "blocking",
          reviewGateStatus: "blocking",
        }),
      }),
    });

    await user.click(screen.getByRole("button", { name: "Review actions" }));
    await user.click(screen.getByTestId("agents-review-approve-anyway"));
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onApproveAnyway).not.toHaveBeenCalled();

    await user.click(screen.getByTestId("agents-review-approve-anyway"));
    const confirmButton = screen.getByRole("button", {
      name: "Approve anyway",
    });
    await user.click(confirmButton);

    expect(onApproveAnyway).toHaveBeenCalledOnce();
    expect(screen.getByRole("button", { name: "Approving..." })).toBeDisabled();
    finishApproval?.();
  });

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

  it("keeps Review inspectable but disables retry during conflict repair", () => {
    renderPanel({
      reviewActionBlocker: {
        kind: "repair",
        message: "Finish or abort the current repair, then retry Review.",
      },
    });

    expect(screen.getByTestId("mock-plan-display")).toBeInTheDocument();
    expect(
      screen.getByText("Finish or abort the current repair, then retry Review."),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Update review" })).toBeDisabled();
  });

  it("labels retained output from an interrupted Review as unfinalized", () => {
    renderPanel({
      reviewContext: reviewContext({
        monitor: reviewMonitor({
          status: "blocked",
          reviewOutcome: "run_failed",
          reviewGateStatus: "failed",
          lastError: "Provider exited after saving output",
        }),
      }),
    });

    expect(
      screen.getByText("Review failed; output was saved but not finalized."),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Retry review" })).toBeEnabled();
  });

  it("keeps the outdated Review warning when the action is not runtime-blocked", () => {
    renderPanel();

    expect(screen.getByRole("button", { name: "Update review" })).toBeEnabled();
    expect(screen.getByText("Review is outdated")).toBeInTheDocument();
    expect(
      screen.getByText(/Previous Review covers earlier changes\./),
    ).toBeInTheDocument();
  });

  it.each([
    [
      "paused_for_review",
      "GitHub auto-merge is paused until this Review is resolved.",
    ],
    [
      "awaiting_publish",
      "GitHub auto-merge will resume after these reviewed changes are published.",
    ],
    [
      "restore_failed",
      "GitHub auto-merge is still paused and restoration will retry.",
    ],
    ["pausing", "Updating GitHub auto-merge…"],
    ["restoring", "Updating GitHub auto-merge…"],
  ] as const)(
    "shows the %s auto-merge guard detail",
    (autoMergeGuardStatus, expectedDetail) => {
      renderPanel({
        reviewContext: reviewContext({
          monitor: reviewMonitor({
            autoMergeGuardStatus,
            autoMergeGuardLastError: null,
          }),
        }),
      });

      expect(screen.getByText(expectedDetail)).toBeInTheDocument();
    },
  );

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

  it("ignores stale Workspace Review start results in Review PR Review tabs", () => {
    renderPanel({
      isReviewPrWorkspace: true,
      reviewContext: null,
      reviewStartResult: reviewStartResult({
        isCurrent: false,
        isOutdated: true,
        shouldShowTab: true,
      }),
    });

    expect(
      screen.getByTestId("agents-review-pr-auto-approve"),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Update review" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Run review" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Fix Issues" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Review actions" }),
    ).not.toBeInTheDocument();
  });
});
