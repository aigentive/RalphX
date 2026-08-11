import { describe, expect, it } from "vitest";

import type {
  AgentConversationWorkspace,
  AgentConversationWorkspaceFreshness,
  AgentConversationWorkspacePublicationEvent,
} from "@/api/chat";
import {
  canInspectAgentWorkspaceBaseFreshness,
  canInspectAgentWorkspacePublishDiffs,
  classifyAgentWorkspacePublishTerminalEvent,
  getPostBaselinePublicationEvents,
  getAgentWorkspaceEffectiveBaseLabel,
  getAgentWorkspaceDescriptionFailurePresentation,
  getAgentWorkspaceHoldPresentation,
  getAgentWorkspacePublishReceiptPresentation,
  getAgentWorkspaceMaintenancePresentation,
  getAgentWorkspacePrAutofixFingerprintSpendPresentation,
  isAgentWorkspaceMaintenanceActive,
  blocksAgentWorkspaceGitInspection,
  canResumeAgentWorkspacePublish,
  getAgentWorkspacePrConflictSummary,
  getAgentWorkspaceReviewActionBlocker,
  isAgentWorkspaceAutoMergeDeferred,
  isAgentWorkspaceAutoMergeRequestPending,
  isAgentWorkspacePublishActive,
  isAgentWorkspacePublishCurrent,
  shouldAutoRefreshCleanAgentWorkspaceFromBase,
  shouldShowAgentWorkspacePublishSurface,
} from "./agentWorkspacePublishState";

describe("getAgentWorkspaceDescriptionFailurePresentation", () => {
  it("distinguishes an unopened PR from an existing linked target", () => {
    expect(getAgentWorkspaceDescriptionFailurePresentation(null).summary).toContain(
      "no pull request was opened",
    );

    const linked = getAgentWorkspaceDescriptionFailurePresentation("PR #888");
    expect(linked.summary).toContain("prior metadata outcome for PR #888");
    expect(linked.summary).toContain("check the linked PR before writing");
    expect(linked.summary).not.toContain("no pull request was opened");
    expect(linked.summary).not.toContain("branch was unchanged");
  });

  it("names the specific metadata outcome when a receipt state is known", () => {
    expect(
      getAgentWorkspaceDescriptionFailurePresentation("PR #888", "not_applied").summary,
    ).toContain("did not apply");
    expect(
      getAgentWorkspaceDescriptionFailurePresentation("PR #888", "conflicted").summary,
    ).toContain("did not overwrite the newer remote version");
  });
});

describe("getAgentWorkspacePublishReceiptPresentation", () => {
  it.each([
    ["prepared", "not_attempted", /Updating PR metadata/i],
    ["reconciling", "unknown", /may have applied the description/i],
    ["settled", "not_applied", /did not apply the requested description/i],
    ["settled", "conflicted", /did not overwrite the newer remote version/i],
  ] as const)("renders truthful %s/%s evidence", (phase, state, summary) => {
    expect(
      getAgentWorkspacePublishReceiptPresentation(
        workspace({
          publicationMetadataPhase: phase,
          publicationMetadataState: state,
          publicationPrNumber: 888,
        }),
      )?.summary,
    ).toMatch(summary);
  });
});

describe("getAgentWorkspaceReviewActionBlocker", () => {
  it("blocks Review actions while an authoritative repair is active", () => {
    expect(
      getAgentWorkspaceReviewActionBlocker(
        workspace({
          publicationPushStatus: "needs_agent",
          prSupervisionStatus: "fixing",
        }),
      ),
    ).toEqual({
      kind: "repair",
      message: "Finish or abort the current repair, then retry Review.",
    });
  });

  it("blocks Review actions for an active maintenance operation before refreshed legacy fields", () => {
    expect(
      getAgentWorkspaceReviewActionBlocker(
        workspace({
          maintenanceOperation: {
            operationId: "maintenance-1",
            generation: 2,
            source: "base_update",
            stage: "repairing",
            status: "active",
            summary: "Resolving the base conflict",
            blocker: null,
            automaticContinuation: true,
            startedAt: "2026-07-25T10:00:00Z",
            updatedAt: "2026-07-25T10:01:00Z",
          },
          publicationPushStatus: "refreshed",
          prSupervisionStatus: "monitoring",
        }),
      ),
    ).toEqual({
      kind: "repair",
      message: "Finish or abort the current repair, then retry Review.",
    });
  });

  it("blocks Review actions for a recovered unresolved conflict", () => {
    expect(
      getAgentWorkspaceReviewActionBlocker(
        workspace({
          publicationPushStatus: "failed",
          prSupervisionStatus: "blocked",
          prSupervisionSummary: "Merge conflict remains in src/main.rs",
        }),
      ),
    ).toEqual({
      kind: "conflict",
      message: "Resolve conflicts before retrying Review.",
    });
  });

  it("allows Review after repair and conflicts are settled", () => {
    expect(
      getAgentWorkspaceReviewActionBlocker(
        workspace({
          publicationPushStatus: "refreshed",
          prSupervisionStatus: "monitoring",
        }),
      ),
    ).toBeNull();
  });
});

describe("maintenance operation presentation", () => {
  const maintenanceOperation = {
    operationId: "maintenance-1",
    generation: 2,
    source: "base_update" as const,
    stage: "repairing" as const,
    status: "active" as const,
    recoveryAction: "none" as const,
    summary: "Resolving the base conflict",
    blocker: null,
    holdReason: null,
    automaticContinuation: true,
    startedAt: "2026-07-25T10:00:00Z",
    updatedAt: "2026-07-25T10:01:00Z",
  };

  it("prefers the active durable operation over legacy publish state", () => {
    const current = workspace({
      maintenanceOperation,
      publicationPushStatus: "refreshed",
    });

    expect(isAgentWorkspaceMaintenanceActive(current)).toBe(true);
    expect(blocksAgentWorkspaceGitInspection(current)).toBe(true);
    expect(getAgentWorkspaceMaintenancePresentation(current)).toMatchObject({
      title: "Repairing workspace",
      action: "none",
      automaticContinuation: "Will continue automatically.",
      busy: true,
    });
  });

  it("keeps Reviewing inspectable while automation remains active", () => {
    const current = workspace({
      maintenanceOperation: { ...maintenanceOperation, stage: "reviewing" },
    });

    expect(isAgentWorkspaceMaintenanceActive(current)).toBe(true);
    expect(blocksAgentWorkspaceGitInspection(current)).toBe(false);
    expect(getAgentWorkspaceMaintenancePresentation(current)?.title).toBe(
      "Workspace Review in progress",
    );
  });

  it.each([
    ["updating_base", "Updating base"],
    ["validating", "Validating repair"],
    ["publishing", "Publishing workspace"],
  ] as const)("presents the active %s stage", (stage, title) => {
    const current = workspace({
      maintenanceOperation: { ...maintenanceOperation, stage },
    });

    expect(getAgentWorkspaceMaintenancePresentation(current)).toMatchObject({
      title,
      action: "none",
      busy: true,
    });
  });

  it("provides one explicit ready or blocked recovery action", () => {
    const ready = workspace({
      maintenanceOperation: {
        ...maintenanceOperation,
        stage: "ready",
        status: "ready",
        recoveryAction: "resume_publish",
        automaticContinuation: false,
      },
    });
    const blocked = workspace({
      maintenanceOperation: {
        ...maintenanceOperation,
        stage: "blocked",
        status: "blocked",
        recoveryAction: "retry_repair",
        blocker: "Resolve the protected branch policy.",
      },
    });

    expect(canResumeAgentWorkspacePublish(ready)).toBe(true);
    expect(getAgentWorkspaceMaintenancePresentation(ready)).toMatchObject({
      title: "Base updated — ready to publish",
      action: "publish",
    });
    expect(getAgentWorkspaceMaintenancePresentation(blocked)).toMatchObject({
      title: "Repair blocked",
      summary: "Resolve the protected branch policy.",
      action: "retry",
    });
  });

  it("does not invent a recovery action for a non-retryable blocked operation", () => {
    const blocked = workspace({
      maintenanceOperation: {
        ...maintenanceOperation,
        stage: "blocked",
        status: "blocked",
        blocker: "Wait for backend recovery.",
      },
    });

    expect(canResumeAgentWorkspacePublish(blocked)).toBe(false);
    expect(getAgentWorkspaceMaintenancePresentation(blocked)).toMatchObject({
      title: "Repair blocked",
      action: "none",
      busy: false,
    });
  });

  it("presents a health-held repair as backend-owned waiting, not ready to publish", () => {
    const held = workspace({
      maintenanceOperation: {
        ...maintenanceOperation,
        source: "pr_autofix",
        stage: "held",
        status: "held",
        holdReason: "pr_autofix_unchanged_health",
        automaticContinuation: false,
      },
    });

    expect(canResumeAgentWorkspacePublish(held)).toBe(false);
    expect(getAgentWorkspaceMaintenancePresentation(held)).toMatchObject({
      title: "Repair paused — waiting for new CI evidence",
      action: "hold",
      busy: false,
      automaticContinuation: null,
    });
  });

  it("presents a reserved CI rerun as held without enabling publish", () => {
    const held = workspace({
      maintenanceOperation: {
        ...maintenanceOperation,
        source: "pr_autofix",
        stage: "held",
        status: "held",
        holdReason: "pr_autofix_ci_rerun_pending",
        automaticContinuation: false,
      },
    });

    expect(canResumeAgentWorkspacePublish(held)).toBe(false);
    expect(getAgentWorkspaceMaintenancePresentation(held)).toMatchObject({
      title: "Repair paused — waiting for CI rerun",
      action: "hold",
      busy: false,
    });
  });

  it("presents a stale base as a non-resumable update failure before CI evidence holds", () => {
    const held = workspace({
      maintenanceOperation: {
        ...maintenanceOperation,
        stage: "ready",
        status: "ready",
        holdReason: "base_stale",
        summary: "The workspace is still behind its base branch.",
        automaticContinuation: false,
      },
    });

    expect(canResumeAgentWorkspacePublish(held)).toBe(false);
    expect(getAgentWorkspaceMaintenancePresentation(held)).toEqual({
      title: "Behind base — update did not take",
      summary: "The workspace is still behind its base branch.",
      tone: "warning",
      busy: false,
      action: "none",
      automaticContinuation: null,
    });
  });

  it("presents a reserved unpublished-head re-drive as automatic publishing", () => {
    const redriving = workspace({
      maintenanceOperation: {
        ...maintenanceOperation,
        source: "pr_autofix",
        stage: "publishing",
        status: "active",
        holdReason: null,
        automaticContinuation: true,
      },
    });

    expect(canResumeAgentWorkspacePublish(redriving)).toBe(false);
    expect(getAgentWorkspaceMaintenancePresentation(redriving)).toMatchObject({
      title: "Publishing workspace",
      action: "none",
      busy: true,
      automaticContinuation: "Will continue automatically.",
    });
  });

  it("explains that a pre-existing base failure cannot be fixed on the PR branch", () => {
    const held = workspace({
      maintenanceOperation: {
        ...maintenanceOperation,
        source: "pr_autofix",
        stage: "held",
        status: "held",
        holdReason: "pr_autofix_pre_existing_on_base",
        automaticContinuation: false,
      },
    });

    expect(getAgentWorkspaceMaintenancePresentation(held)).toMatchObject({
      title: "Repair paused — failure exists on the base branch",
      action: "hold",
      busy: false,
    });
    expect(getAgentWorkspaceMaintenancePresentation(held)?.summary).toContain(
      "fixing it on this PR branch would not help",
    );
  });

  it("presents a publication-effect hold as its own reason, not the CI-rerun copy", () => {
    const held = workspace({
      maintenanceOperation: {
        ...maintenanceOperation,
        source: "publish",
        stage: "held",
        status: "held",
        holdReason: "publication_effect_attention",
        automaticContinuation: false,
      },
    });

    expect(canResumeAgentWorkspacePublish(held)).toBe(false);
    const presentation = getAgentWorkspaceMaintenancePresentation(held);
    expect(presentation).toMatchObject({
      action: "hold",
      busy: false,
      automaticContinuation: null,
    });
    expect(presentation?.title).not.toBe("Repair paused — waiting for new CI evidence");
    expect(presentation?.title).not.toMatch(/CI/i);
  });

  it("does not let stale maintenance data mask a terminal pull request", () => {
    const current = workspace({
      maintenanceOperation,
      publicationPrStatus: "merged",
    });

    expect(getAgentWorkspaceMaintenancePresentation(current)).toBeNull();
    expect(isAgentWorkspaceMaintenanceActive(current)).toBe(false);
  });

  it("renders the agent's whatHappened/whatIDid narrative verbatim over the legacy summary", () => {
    const current = workspace({
      maintenanceOperation: {
        ...maintenanceOperation,
        summary: "Resolving the base conflict",
        whatHappened: "The install step failed with a 404.",
        whatIDid: "Retried twice, then reported the blocker.",
      },
    });

    expect(getAgentWorkspaceMaintenancePresentation(current)?.summary).toBe(
      "The install step failed with a 404. Retried twice, then reported the blocker.",
    );
  });

  it("renders whatHappened alone when whatIDid is absent", () => {
    const current = workspace({
      maintenanceOperation: {
        ...maintenanceOperation,
        whatHappened: "The install step failed with a 404.",
        whatIDid: null,
      },
    });

    expect(getAgentWorkspaceMaintenancePresentation(current)?.summary).toBe(
      "The install step failed with a 404.",
    );
  });

  it("renders whatIDid alone when whatHappened is absent", () => {
    const current = workspace({
      maintenanceOperation: {
        ...maintenanceOperation,
        whatHappened: null,
        whatIDid: "Retried twice, then reported the blocker.",
      },
    });

    expect(getAgentWorkspaceMaintenancePresentation(current)?.summary).toBe(
      "Retried twice, then reported the blocker.",
    );
  });

  it("falls back to the legacy operation.summary when no agent narrative is present", () => {
    const current = workspace({
      maintenanceOperation: {
        ...maintenanceOperation,
        summary: "Resolving the base conflict",
        whatHappened: null,
        whatIDid: null,
      },
    });

    expect(getAgentWorkspaceMaintenancePresentation(current)?.summary).toBe(
      "Resolving the base conflict",
    );
  });

  it("falls back to the hold-reason template paragraph for a poller-created hold with no narrative", () => {
    const held = workspace({
      maintenanceOperation: {
        ...maintenanceOperation,
        source: "pr_autofix",
        stage: "held",
        status: "held",
        holdReason: "pr_autofix_unchanged_health",
        summary: "RalphX is waiting for a new CI result.",
        blocker: null,
        whatHappened: null,
        whatIDid: null,
        automaticContinuation: false,
      },
    });

    expect(getAgentWorkspaceMaintenancePresentation(held)?.summary).toBe(
      getAgentWorkspaceHoldPresentation(held)?.paragraph,
    );
    expect(getAgentWorkspaceMaintenancePresentation(held)?.summary).not.toBe(
      "RalphX is waiting for a new CI result.",
    );
  });

  it("keeps a machine-authored blocker visibly separate from the agent's own narrative", () => {
    const blocked = workspace({
      maintenanceOperation: {
        ...maintenanceOperation,
        stage: "blocked",
        status: "blocked",
        blocker: "Pull-request continuation could not complete: GitHub rejected the push.",
        whatHappened: "The credential rotation needs manual approval.",
        whatIDid: "Escalated instead of guessing at the credential.",
      },
    });

    const { summary } = getAgentWorkspaceMaintenancePresentation(blocked) ?? {};
    expect(summary).toContain("The credential rotation needs manual approval.");
    expect(summary).toContain("Escalated instead of guessing at the credential.");
    expect(summary).toContain(
      "Pull-request continuation could not complete: GitHub rejected the push.",
    );
    // The blocker text must be introduced by RalphX, not appended as if the agent said it.
    const narrativeEnd = summary?.indexOf(
      "Escalated instead of guessing at the credential.",
    );
    const blockerStart = summary?.indexOf(
      "Pull-request continuation could not complete",
    );
    expect(narrativeEnd).toBeGreaterThan(-1);
    expect(blockerStart).toBeGreaterThan(narrativeEnd ?? -1);
    expect(
      summary?.slice((narrativeEnd ?? 0) + "Escalated instead of guessing at the credential.".length, blockerStart),
    ).toMatch(/RalphX/);
  });
});

describe("PR autofix fingerprint spend presentation", () => {
  it("reports effort for the current failure and marks an exhausted budget", () => {
    expect(
      getAgentWorkspacePrAutofixFingerprintSpendPresentation(
        workspace({
          prAutofixFingerprintSpend: {
            generations: 3,
            minutes: 92,
            budgetMinutes: 45,
            isExhausted: true,
          },
        }),
      ),
    ).toEqual({
      summary: "3 generations · 92 min on this failure",
      exhausted: true,
    });
  });

  it("omits the indicator when there is no tracked failure fingerprint", () => {
    expect(getAgentWorkspacePrAutofixFingerprintSpendPresentation(workspace())).toBeNull();
  });
});

describe("getAgentWorkspaceHoldPresentation", () => {
  const maintenanceOperation = {
    operationId: "maintenance-1",
    generation: 2,
    source: "publish" as const,
    stage: "held" as const,
    status: "held" as const,
    summary: null,
    blocker: null,
    automaticContinuation: false,
    startedAt: "2026-07-25T10:00:00Z",
    updatedAt: "2026-07-25T10:01:00Z",
  };

  it("returns null outside the held stage", () => {
    expect(
      getAgentWorkspaceHoldPresentation(
        workspace({
          maintenanceOperation: {
            ...maintenanceOperation,
            stage: "ready",
            status: "ready",
            holdReason: "pr_autofix_unchanged_health",
          },
        }),
      ),
    ).toBeNull();
  });

  it("uses one fixed pill across every hold reason", () => {
    expect(
      getAgentWorkspaceHoldPresentation(
        workspace({
          maintenanceOperation: {
            ...maintenanceOperation,
            holdReason: "pr_autofix_unchanged_health",
          },
        }),
      )?.pill,
    ).toBe("Auto-repair paused");
  });

  it("explains a publication-effect hold in plain product language with no jargon", () => {
    const presentation = getAgentWorkspaceHoldPresentation(
      workspace({
        maintenanceOperation: {
          ...maintenanceOperation,
          holdReason: "publication_effect_attention",
        },
      }),
    );

    expect(presentation).not.toBeNull();
    expect(presentation?.headline).toBe("Repair paused — publish not confirmed");
    expect(presentation?.primary).toMatchObject({ kind: "retryPublication" });
    expect(presentation?.primary.label.toLowerCase()).toContain("retry publication");
    expect(presentation?.paragraph.toLowerCase()).not.toContain("effect fence");
    expect(presentation?.paragraph.toLowerCase()).not.toContain("cas");
    expect(presentation?.paragraph.toLowerCase()).not.toContain(" ci ");
  });

  it("keeps the hold-reason template paragraph regardless of the durable operation summary", () => {
    const presentation = getAgentWorkspaceHoldPresentation(
      workspace({
        maintenanceOperation: {
          ...maintenanceOperation,
          holdReason: "publication_effect_attention",
          summary: "RalphX pushed commit abc123 but GitHub returned a 502.",
        },
      }),
    );

    expect(presentation?.paragraph).toBe(
      "RalphX pushed a repair but could not confirm it reached GitHub. Retry publication to make RalphX try again.",
    );
  });

  it("renders the agent's narrative verbatim in place of the template paragraph", () => {
    const presentation = getAgentWorkspaceHoldPresentation(
      workspace({
        maintenanceOperation: {
          ...maintenanceOperation,
          holdReason: "pr_autofix_base_parity_transient",
          summary: "RalphX is waiting on a re-run.",
          whatHappened: "GitHub cancelled the test job before it started.",
          whatIDid: "Left the branch untouched so a re-run can pick it up.",
        },
      }),
    );

    expect(presentation?.paragraph).toBe(
      "GitHub cancelled the test job before it started. Left the branch untouched so a re-run can pick it up.",
    );
  });

  it("keeps the template paragraph for a poller-created hold that has no narrative", () => {
    const presentation = getAgentWorkspaceHoldPresentation(
      workspace({
        maintenanceOperation: {
          ...maintenanceOperation,
          holdReason: "pr_autofix_base_parity_transient",
          whatHappened: null,
          whatIDid: null,
        },
      }),
    );

    expect(presentation?.paragraph).toBe(
      "GitHub cancelled or timed out the checks, and the same failure is present on the base branch. Re-running the checks can clear this.",
    );
  });

  it("keeps the existing CI-flavoured headline for other hold reasons", () => {
    const presentation = getAgentWorkspaceHoldPresentation(
      workspace({
        maintenanceOperation: {
          ...maintenanceOperation,
          holdReason: "pr_autofix_unchanged_health",
        },
      }),
    );

    expect(presentation?.headline).toBe("Repair paused — waiting for new CI evidence");
    expect(presentation?.primary).toMatchObject({ kind: "retryRepair" });
  });

  it("names the repair cost in the retry caption when spend is tracked", () => {
    const presentation = getAgentWorkspaceHoldPresentation(
      workspace({
        maintenanceOperation: {
          ...maintenanceOperation,
          holdReason: "pr_autofix_unchanged_health",
        },
        prAutofixFingerprintSpend: {
          generations: 2,
          minutes: 41,
          budgetMinutes: 45,
          isExhausted: false,
        },
      }),
    );

    expect(presentation?.primary.caption).toContain("2 generations · 41 min on this failure");
  });

  it("explains a base-parity-transient hold in plain product language", () => {
    const presentation = getAgentWorkspaceHoldPresentation(
      workspace({
        maintenanceOperation: {
          ...maintenanceOperation,
          holdReason: "pr_autofix_base_parity_transient",
        },
      }),
    );

    expect(presentation).not.toBeNull();
    expect(presentation?.paragraph).toMatch(/cancel|timed out/i);
    expect(presentation?.paragraph).toMatch(/base branch/i);
    expect(presentation?.paragraph).toMatch(/re-run/i);
    expect(presentation?.primary).toMatchObject({ kind: "rerunChecks" });
  });

  it("names the remaining rerun budget in the rerun-checks caption when spend is tracked", () => {
    const presentation = getAgentWorkspaceHoldPresentation(
      workspace({
        maintenanceOperation: {
          ...maintenanceOperation,
          holdReason: "pr_autofix_base_parity_transient",
        },
        prAutofixFingerprintSpend: {
          generations: 1,
          minutes: 12,
          budgetMinutes: 45,
          isExhausted: false,
        },
      }),
    );

    expect(presentation?.primary.caption).toContain("1 generations · 12 min on this failure");
  });

  it("gives the pre-existing-on-base hold a Waiting on main court chip and a recheck primary", () => {
    const presentation = getAgentWorkspaceHoldPresentation(
      workspace({
        maintenanceOperation: {
          ...maintenanceOperation,
          holdReason: "pr_autofix_pre_existing_on_base",
        },
      }),
    );

    expect(presentation?.courtChip).toMatchObject({ label: "Waiting on main" });
    expect(presentation?.primary).toMatchObject({ kind: "recheck" });
  });

  it.each([
    ["pr_autofix_unchanged_health", "retryRepair"],
    ["pr_autofix_pre_existing_on_base", "recheck"],
    ["pr_autofix_ci_rerun_pending", "recheck"],
    ["base_stale", "recheck"],
    ["health_evidence", "recheck"],
    ["publish_redrive", "retryPublication"],
    ["publication_effect_attention", "retryPublication"],
    ["pr_autofix_base_parity_transient", "rerunChecks"],
  ] as const)(
    "projects a full four-layer presentation for %s",
    (holdReason, expectedPrimaryKind) => {
      const presentation = getAgentWorkspaceHoldPresentation(
        workspace({
          maintenanceOperation: { ...maintenanceOperation, holdReason },
        }),
      );

      expect(presentation).not.toBeNull();
      expect(presentation?.pill).toBe("Auto-repair paused");
      expect(presentation?.headline.length).toBeGreaterThan(0);
      expect(presentation?.paragraph.length).toBeGreaterThan(0);
      expect(presentation?.courtChip.label.length).toBeGreaterThan(0);
      expect(presentation?.primary).toMatchObject({ kind: expectedPrimaryKind });
      expect(presentation?.primary.label.length).toBeGreaterThan(0);
      expect(presentation?.primary.caption.length).toBeGreaterThan(0);
      expect(presentation?.releaseConditions.length).toBeGreaterThan(0);
      expect(Array.isArray(presentation?.more)).toBe(true);
    },
  );
});

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
    createdAt: "2026-04-23T09:00:01Z",
    ...overrides,
  };
}

function workspace(
  overrides: Partial<AgentConversationWorkspace> = {},
): AgentConversationWorkspace {
  return {
    conversationId: "conversation-1",
    projectId: "project-1",
    mode: "edit",
    branchMode: "isolated",
    baseRefKind: "project_default",
    baseRef: "main",
    baseDisplayName: "Project default (main)",
    baseCommit: null,
    branchName: "ralphx/ralphx/agent-abcdef12",
    worktreePath: "/tmp/ralphx/conversation-1",
    linkedIdeationSessionId: null,
    linkedPlanBranchId: null,
    publicationPrNumber: null,
    publicationPrUrl: null,
    publicationPrStatus: null,
    publicationPushStatus: null,
    autoPublishEnabled: true,
    autoPublishPausedPrAutofixEnabled: null,
    autoPublishPausedPrAutoMergeDesired: null,
    status: "active",
    createdAt: "2026-04-23T09:00:00Z",
    updatedAt: "2026-04-23T09:00:00Z",
    ...overrides,
  };
}

function freshness(
  overrides: Partial<AgentConversationWorkspaceFreshness> = {},
): AgentConversationWorkspaceFreshness {
  return {
    conversationId: "conversation-1",
    freshnessScope: "full",
    baseRef: "release/1.2",
    baseDisplayName: "release/1.2",
    targetRef: "origin/release/1.2",
    capturedBaseCommit: "old-base-sha",
    targetBaseCommit: "new-base-sha",
    isBaseAhead: true,
    hasUncommittedChanges: false,
    unpublishedCommitCount: 0,
    remoteRefreshed: true,
    worktreeStatusChecked: true,
    baseStatus: "valid",
    effectiveBaseRef: "release/1.2",
    effectiveBaseDisplayName: "release/1.2",
    baseBlockReason: null,
    ...overrides,
  };
}

const base = {
  autoMergeDesired: true,
  autoMergeCurrent: false as boolean | null,
  hasPublishedPr: true,
  publicationPushStatus: "pushed",
  terminalPublicationStatus: null as string | null,
};

describe("isAgentWorkspacePublishActive", () => {
  it.each([
    "checking",
    "committing",
    "refreshing",
    "describing",
    "pushing",
    "redrive_pending",
    "redrive_delivering",
  ])(
    "treats %s as an active publish status",
    (publicationPushStatus) => {
      expect(
        isAgentWorkspacePublishActive(workspace({ publicationPushStatus })),
      ).toBe(true);
    },
  );

  it("normalizes casing and whitespace for active publish statuses", () => {
    expect(
      isAgentWorkspacePublishActive(
        workspace({ publicationPushStatus: "  PuShInG  " }),
      ),
    ).toBe(true);
  });

  it.each([
    null,
    "pending",
    "pushed",
    "published",
    "refreshed",
    "failed",
    "description_failed",
    "needs_agent",
    "future_status",
  ])("does not treat %s as active publishing", (publicationPushStatus) => {
    expect(
      isAgentWorkspacePublishActive(workspace({ publicationPushStatus })),
    ).toBe(false);
  });

  it.each(["merged", "closed"])(
    "keeps terminal %s pull requests out of the active publish lock",
    (publicationPrStatus) => {
      expect(
        isAgentWorkspacePublishActive(
          workspace({ publicationPrStatus, publicationPushStatus: "pushing" }),
        ),
      ).toBe(false);
    },
  );

  it("handles a missing workspace", () => {
    expect(isAgentWorkspacePublishActive(null)).toBe(false);
  });
});

describe("getPostBaselinePublicationEvents", () => {
  const startedAtMs = new Date("2026-04-23T09:00:00Z").getTime();
  const events = [
    publicationEvent({ id: "old", createdAt: "2026-04-23T08:59:59Z" }),
    publicationEvent({ id: "checking" }),
    publicationEvent({
      id: "published",
      step: "published",
      status: "succeeded",
      createdAt: "2026-04-23T09:00:02Z",
    }),
  ];

  it("returns only the ordered suffix after an exact event baseline", () => {
    expect(
      getPostBaselinePublicationEvents(events, "old", startedAtMs)?.map(
        (event) => event.id,
      ),
    ).toEqual(["checking", "published"]);
  });

  it("accepts all valid later events after an authoritatively empty baseline", () => {
    expect(
      getPostBaselinePublicationEvents(events.slice(1), null, startedAtMs)?.map(
        (event) => event.id,
      ),
    ).toEqual(["checking", "published"]);
  });

  it("fails closed when the non-empty baseline disappears", () => {
    expect(
      getPostBaselinePublicationEvents(events.slice(1), "old", startedAtMs),
    ).toBeNull();
  });

  it("excludes malformed and implausibly old timestamps from terminal authority", () => {
    const suffix = getPostBaselinePublicationEvents(
      [
        publicationEvent({ id: "baseline" }),
        publicationEvent({
          id: "malformed",
          step: "published",
          status: "succeeded",
          createdAt: "not-a-date",
        }),
        publicationEvent({
          id: "stale",
          step: "published",
          status: "succeeded",
          createdAt: "2026-04-23T08:00:00Z",
        }),
      ],
      "baseline",
      startedAtMs,
    );

    expect(suffix).toEqual([]);
  });
});

describe("classifyAgentWorkspacePublishTerminalEvent", () => {
  const currentWorkspace = workspace({
    publicationPrNumber: 78,
    publicationPushStatus: "pushed",
  });
  const currentFreshness = freshness({
    isBaseAhead: false,
    hasUncommittedChanges: false,
    unpublishedCommitCount: 0,
  });

  it("authorizes published success only with current workspace and full freshness", () => {
    const published = publicationEvent({
      step: "published",
      status: "succeeded",
    });

    expect(
      classifyAgentWorkspacePublishTerminalEvent(
        [published],
        currentWorkspace,
        currentFreshness,
      ),
    ).toEqual({ event: published, kind: "success" });
    expect(
      classifyAgentWorkspacePublishTerminalEvent(
        [published],
        workspace({ publicationPushStatus: "pushed" }),
        currentFreshness,
      ),
    ).toBeNull();
    expect(
      classifyAgentWorkspacePublishTerminalEvent(
        [published],
        currentWorkspace,
        undefined,
      ),
    ).toBeNull();
  });

  it.each([
    ["needs_agent", "failed", "agent_fixable", "needs_agent"],
    ["failed", "failed", "operational", "failure"],
    ["description_failed", "failed", "operational", "failure"],
    ["no_changes", "skipped", null, "no_changes"],
  ] as const)(
    "classifies %s/%s as %s",
    (step, status, classification, expectedKind) => {
      const event = publicationEvent({ step, status, classification });
      expect(
        classifyAgentWorkspacePublishTerminalEvent(
          [event],
          workspace({ publicationPushStatus: step }),
          undefined,
        ),
      ).toEqual({ event, kind: expectedKind });
    },
  );

  it("keeps a terminal failure visible when later repair events are appended", () => {
    const failure = publicationEvent({
      id: "failure",
      step: "needs_agent",
      status: "failed",
      classification: "agent_fixable",
    });
    expect(
      classifyAgentWorkspacePublishTerminalEvent(
        [
          publicationEvent({ step: "checking", status: "started" }),
          failure,
          publicationEvent({
            id: "repair",
            step: "repair_sent",
            status: "succeeded",
          }),
        ],
        workspace({ publicationPushStatus: "needs_agent" }),
        undefined,
      ),
    ).toEqual({ event: failure, kind: "needs_agent" });
  });

  it("ignores progress, repair-only, unknown, and mismatched needs-agent evidence", () => {
    expect(
      classifyAgentWorkspacePublishTerminalEvent(
        [
          publicationEvent({ step: "checking", status: "started" }),
          publicationEvent({ step: "repair_sent", status: "succeeded" }),
          publicationEvent({ step: "future_step", status: "succeeded" }),
          publicationEvent({ step: "needs_agent", status: "succeeded" }),
        ],
        workspace(),
        undefined,
      ),
    ).toBeNull();
  });
});

describe("getAgentWorkspaceEffectiveBaseLabel", () => {
  it("uses the actual base ref for linked workspaces when the stored display name is the source branch", () => {
    expect(
      getAgentWorkspaceEffectiveBaseLabel(
        workspace({
          branchMode: "linked",
          baseRef: "master",
          baseDisplayName: "feature/diverged-agent-work",
          branchName: "feature/diverged-agent-work",
        }),
        undefined,
      ),
    ).toBe("master");
  });

  it("retains the descriptive base label for isolated workspaces", () => {
    expect(
      getAgentWorkspaceEffectiveBaseLabel(
        workspace({
          baseRef: "main",
          baseDisplayName: "Project default (main)",
        }),
        undefined,
      ),
    ).toBe("Project default (main)");
  });
});

describe("isAgentWorkspacePublishCurrent", () => {
  const currentFreshness = () =>
    freshness({
      isBaseAhead: false,
      hasUncommittedChanges: false,
      unpublishedCommitCount: 0,
    });

  it("treats pushed published workspaces with no remaining changes as current", () => {
    expect(
      isAgentWorkspacePublishCurrent(
        workspace({
          publicationPrNumber: 78,
          publicationPushStatus: "pushed",
        }),
        currentFreshness(),
      ),
    ).toBe(true);
  });

  it("treats refreshed published workspaces with no remaining changes as current", () => {
    expect(
      isAgentWorkspacePublishCurrent(
        workspace({
          publicationPrNumber: 78,
          publicationPushStatus: "refreshed",
        }),
        currentFreshness(),
      ),
    ).toBe(true);
  });

  it("rejects pending publication statuses even when freshness is clean", () => {
    expect(
      isAgentWorkspacePublishCurrent(
        workspace({
          publicationPrNumber: 78,
          publicationPushStatus: "checking",
        }),
        currentFreshness(),
      ),
    ).toBe(false);
  });
});

describe("isAgentWorkspaceAutoMergeRequestPending", () => {
  it("returns true when supervision status is null (active publish in progress)", () => {
    expect(
      isAgentWorkspaceAutoMergeRequestPending({
        ...base,
        prSupervisionStatus: null,
      }),
    ).toBe(true);
  });

  it("returns false when supervision status is waiting (deferred/failed)", () => {
    expect(
      isAgentWorkspaceAutoMergeRequestPending({
        ...base,
        prSupervisionStatus: "waiting",
      }),
    ).toBe(false);
  });

  it("returns false when autoMergeCurrent is true", () => {
    expect(
      isAgentWorkspaceAutoMergeRequestPending({
        ...base,
        autoMergeCurrent: true,
        prSupervisionStatus: null,
      }),
    ).toBe(false);
  });

  it("returns false when autoMergeDesired is false", () => {
    expect(
      isAgentWorkspaceAutoMergeRequestPending({
        ...base,
        autoMergeDesired: false,
        prSupervisionStatus: null,
      }),
    ).toBe(false);
  });

  it("returns false for terminal publication status", () => {
    expect(
      isAgentWorkspaceAutoMergeRequestPending({
        ...base,
        prSupervisionStatus: null,
        terminalPublicationStatus: "merged",
      }),
    ).toBe(false);
  });

  it("returns false when supervision is monitoring", () => {
    expect(
      isAgentWorkspaceAutoMergeRequestPending({
        ...base,
        prSupervisionStatus: "monitoring",
      }),
    ).toBe(false);
  });
});

describe("isAgentWorkspaceAutoMergeDeferred", () => {
  it("returns true when supervision status is waiting", () => {
    expect(
      isAgentWorkspaceAutoMergeDeferred({
        ...base,
        prSupervisionStatus: "waiting",
      }),
    ).toBe(true);
  });

  it("returns false when supervision status is null", () => {
    expect(
      isAgentWorkspaceAutoMergeDeferred({
        ...base,
        prSupervisionStatus: null,
      }),
    ).toBe(false);
  });

  it("returns false when autoMergeCurrent is true", () => {
    expect(
      isAgentWorkspaceAutoMergeDeferred({
        ...base,
        autoMergeCurrent: true,
        prSupervisionStatus: "waiting",
      }),
    ).toBe(false);
  });

  it("returns false when autoMergeDesired is false", () => {
    expect(
      isAgentWorkspaceAutoMergeDeferred({
        ...base,
        autoMergeDesired: false,
        prSupervisionStatus: "waiting",
      }),
    ).toBe(false);
  });

  it("returns false when supervision is monitoring", () => {
    expect(
      isAgentWorkspaceAutoMergeDeferred({
        ...base,
        prSupervisionStatus: "monitoring",
      }),
    ).toBe(false);
  });
});

describe("shouldShowAgentWorkspacePublishSurface", () => {
  it("shows the publish surface for edit workspaces linked to a planning session", () => {
    expect(
      shouldShowAgentWorkspacePublishSurface(
        workspace({ linkedIdeationSessionId: "planning-session-1" }),
      ),
    ).toBe(true);
  });

  it("shows the publish surface for edit workspaces linked to a plan branch", () => {
    expect(
      shouldShowAgentWorkspacePublishSurface(
        workspace({ linkedPlanBranchId: "plan-branch-1" }),
      ),
    ).toBe(true);
  });

  it("shows the publish surface for plan workspaces before publication", () => {
    expect(
      shouldShowAgentWorkspacePublishSurface(
        workspace({
          mode: "plan",
          linkedIdeationSessionId: "planning-session-1",
        }),
      ),
    ).toBe(true);
  });

  it("shows the publish surface for published plan workspaces", () => {
    expect(
      shouldShowAgentWorkspacePublishSurface(
        workspace({
          mode: "plan",
          publicationPrNumber: 648,
          publicationPushStatus: "pushed",
        }),
      ),
    ).toBe(true);
  });

  it("keeps the publish surface reachable for missing plan workspaces", () => {
    expect(
      shouldShowAgentWorkspacePublishSurface(
        workspace({
          mode: "plan",
          linkedIdeationSessionId: "planning-session-1",
          status: "missing",
        }),
      ),
    ).toBe(true);
  });

  it("keeps non-publish workspace modes out of the publish surface", () => {
    expect(shouldShowAgentWorkspacePublishSurface(workspace({ mode: "chat" }))).toBe(
      false,
    );
    expect(
      shouldShowAgentWorkspacePublishSurface(workspace({ mode: "review_pr" })),
    ).toBe(false);
  });

  it("shows the existing publish surface for automation setup workspaces", () => {
    expect(
      shouldShowAgentWorkspacePublishSurface(workspace({ mode: "automation" })),
    ).toBe(true);
  });
});

describe("canInspectAgentWorkspacePublishDiffs", () => {
  it("allows active edit workspaces", () => {
    expect(canInspectAgentWorkspacePublishDiffs(workspace())).toBe(true);
  });

  it("allows linked ideation plan workspaces", () => {
    expect(
      canInspectAgentWorkspacePublishDiffs(
        workspace({
          mode: "ideation",
          linkedPlanBranchId: "plan-branch-1",
        }),
      ),
    ).toBe(true);
  });

  it("allows active plan workspaces", () => {
    expect(
      canInspectAgentWorkspacePublishDiffs(
        workspace({
          mode: "plan",
          linkedIdeationSessionId: "planning-session-1",
        }),
      ),
    ).toBe(true);
  });

  it("rejects ideation workspaces without a linked plan branch", () => {
    expect(
      canInspectAgentWorkspacePublishDiffs(workspace({ mode: "ideation" })),
    ).toBe(false);
  });

  it("rejects missing workspaces by default", () => {
    expect(
      canInspectAgentWorkspacePublishDiffs(workspace({ status: "missing" })),
    ).toBe(false);
  });

  it("rejects missing plan workspaces for live diff inspection", () => {
    expect(
      canInspectAgentWorkspacePublishDiffs(
        workspace({
          mode: "plan",
          linkedIdeationSessionId: "planning-session-1",
          status: "missing",
        }),
      ),
    ).toBe(false);
  });

  it("can preserve terminal published edit workspace inspection", () => {
    expect(
      canInspectAgentWorkspacePublishDiffs(
        workspace({
          status: "missing",
          publicationPrNumber: 42,
          publicationPrStatus: "merged",
        }),
        { includeTerminalPublished: true },
      ),
    ).toBe(true);
  });
});

describe("canInspectAgentWorkspaceBaseFreshness", () => {
  it("allows edit workspaces", () => {
    expect(canInspectAgentWorkspaceBaseFreshness(workspace())).toBe(true);
  });

  it("allows linked ideation plan workspaces before a PR exists", () => {
    expect(
      canInspectAgentWorkspaceBaseFreshness(
        workspace({
          mode: "ideation",
          linkedPlanBranchId: "plan-branch-1",
        }),
      ),
    ).toBe(true);
  });

  it("keeps plan publish, diff, and base freshness surfaces inspectable", () => {
    const planWorkspace = workspace({
      mode: "plan",
      linkedIdeationSessionId: "planning-session-1",
      publicationPrNumber: 42,
      publicationPrStatus: "open",
    });

    expect(canInspectAgentWorkspaceBaseFreshness(planWorkspace)).toBe(true);
    expect(shouldShowAgentWorkspacePublishSurface(planWorkspace)).toBe(true);
    expect(canInspectAgentWorkspacePublishDiffs(planWorkspace)).toBe(true);
  });

  it("preserves published PR freshness inspection", () => {
    expect(
      canInspectAgentWorkspaceBaseFreshness(
        workspace({
          mode: "ideation",
          publicationPrNumber: 42,
        }),
      ),
    ).toBe(true);
  });

  it("rejects ideation workspaces without a linked plan branch or PR", () => {
    expect(
      canInspectAgentWorkspaceBaseFreshness(workspace({ mode: "ideation" })),
    ).toBe(false);
  });

  it("keeps missing plan workspaces eligible for base freshness inspection", () => {
    expect(
      canInspectAgentWorkspaceBaseFreshness(
        workspace({
          mode: "plan",
          linkedIdeationSessionId: "planning-session-1",
          status: "missing",
        }),
      ),
    ).toBe(true);
  });
});

describe("shouldAutoRefreshCleanAgentWorkspaceFromBase", () => {
  it("allows clean edit workspaces behind their configured base", () => {
    expect(
      shouldAutoRefreshCleanAgentWorkspaceFromBase(
        workspace({ baseRef: "release/1.2", baseDisplayName: "release/1.2" }),
        freshness(),
      ),
    ).toBe(true);
  });

  it("rejects workspaces with local changes or publishable commits", () => {
    expect(
      shouldAutoRefreshCleanAgentWorkspaceFromBase(
        workspace(),
        freshness({ hasUncommittedChanges: true }),
      ),
    ).toBe(false);
    expect(
      shouldAutoRefreshCleanAgentWorkspaceFromBase(
        workspace(),
        freshness({ unpublishedCommitCount: 1 }),
      ),
    ).toBe(false);
  });

  it("requires a full remote-refreshed freshness check", () => {
    expect(
      shouldAutoRefreshCleanAgentWorkspaceFromBase(
        workspace(),
        freshness({ freshnessScope: "local" }),
      ),
    ).toBe(false);
    expect(
      shouldAutoRefreshCleanAgentWorkspaceFromBase(
        workspace(),
        freshness({ remoteRefreshed: false }),
      ),
    ).toBe(false);
    expect(
      shouldAutoRefreshCleanAgentWorkspaceFromBase(
        workspace(),
        freshness({ worktreeStatusChecked: false }),
      ),
    ).toBe(false);
  });

  it("rejects blocked, missing, and non-edit workspaces", () => {
    expect(
      shouldAutoRefreshCleanAgentWorkspaceFromBase(
        workspace(),
        freshness({ baseStatus: "blocked" }),
      ),
    ).toBe(false);
    expect(
      shouldAutoRefreshCleanAgentWorkspaceFromBase(
        workspace({ status: "missing" }),
        freshness(),
      ),
    ).toBe(false);
    expect(
      shouldAutoRefreshCleanAgentWorkspaceFromBase(
        workspace({ mode: "ideation" }),
        freshness(),
      ),
    ).toBe(false);
  });
});

describe("getAgentWorkspacePrConflictSummary", () => {
  it("returns blocked merge-conflict summaries", () => {
    expect(
      getAgentWorkspacePrConflictSummary(
        workspace({
          prSupervisionStatus: "blocked",
          prSupervisionSummary:
            "PR #470 has merge conflicts. GitHub reports: PR is reported as conflicting.",
        }),
      ),
    ).toBe(
      "PR #470 has merge conflicts. GitHub reports: PR is reported as conflicting.",
    );
  });

  it("ignores generic blocked supervision summaries", () => {
    expect(
      getAgentWorkspacePrConflictSummary(
        workspace({
          prSupervisionStatus: "blocked",
          prSupervisionSummary: "Required checks are still pending.",
        }),
      ),
    ).toBeNull();
    expect(
      getAgentWorkspacePrConflictSummary(
        workspace({
          prSupervisionStatus: "monitoring",
          prSupervisionSummary: "PR #470 has merge conflicts.",
        }),
      ),
    ).toBeNull();
  });
});
