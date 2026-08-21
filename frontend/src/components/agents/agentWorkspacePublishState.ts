import type {
  AgentConversationWorkspace,
  AgentConversationWorkspaceFreshness,
  AgentConversationWorkspacePublicationEvent,
  AgentWorkspaceMaintenanceOperation,
  AgentWorkspaceMaintenanceOperationHoldReason,
  AgentWorkspacePrAutofixFingerprintSpend,
  AgentWorkspaceReviewGateStatus,
} from "@/api/chat";

const PUBLISH_EVENT_START_SKEW_MS = 5_000;
const AGENT_WORKSPACE_ACTIVE_PUBLISH_STATUSES = new Set([
  "checking",
  "committing",
  "refreshing",
  "describing",
  "pushing",
  "redrive_pending",
  "redrive_delivering",
]);

export type AgentWorkspacePublishTerminalEvent = {
  event: AgentConversationWorkspacePublicationEvent;
  kind: "failure" | "needs_agent" | "no_changes" | "success";
};

export type AgentWorkspacePublishReceiptPresentation = {
  summary: string;
  title: string;
  tone: "error" | "neutral";
};

const ACTIVE_METADATA_RECEIPT_PHASES = new Set([
  "prepared",
  "mutating",
  "reconciling",
]);

export function hasPublishedWorkspacePr(
  workspace: AgentConversationWorkspace | null
): boolean {
  return Boolean(workspace?.publicationPrNumber ?? workspace?.publicationPrUrl);
}

export function getAgentWorkspaceDescriptionFailurePresentation(
  targetPullRequestLabel: string | null,
  receiptState: AgentConversationWorkspace["publicationMetadataState"] = null,
): { title: string; summary: string } {
  if (receiptState === "not_attempted") {
    return {
      title: "PR metadata not updated",
      summary:
        "The PR changed before the metadata write, so RalphX left the newer remote description untouched.",
    };
  }
  if (receiptState === "not_applied") {
    return {
      title: "PR metadata not updated",
      summary:
        "GitHub did not apply the requested description update. Retry after reviewing the latest event.",
    };
  }
  if (receiptState === "conflicted") {
    return {
      title: "PR metadata changed",
      summary:
        "The PR changed during the metadata update. RalphX did not overwrite the newer remote version.",
    };
  }
  if (targetPullRequestLabel) {
    return {
      title: "Publishing failed",
      summary: `RalphX could not confirm the prior metadata outcome for ${targetPullRequestLabel}. The next retry will check the linked PR before writing.`,
    };
  }
  return {
    title: "Publishing failed",
    summary:
      "RalphX could not draft a PR description, so no pull request was opened. Review the latest publish event before retrying Commit & Publish.",
  };
}

export function getAgentWorkspacePublishReceiptPresentation(
  workspace: AgentConversationWorkspace | null | undefined,
): AgentWorkspacePublishReceiptPresentation | null {
  const phase = workspace?.publicationMetadataPhase;
  const state = workspace?.publicationMetadataState;
  if (phase === "prepared" || phase === "mutating") {
    return {
      title: "Updating PR metadata",
      summary: "Updating PR metadata…",
      tone: "neutral",
    };
  }
  if (phase === "reconciling") {
    return {
      title: "Verifying PR metadata",
      summary:
        "GitHub may have applied the description. RalphX is verifying the linked PR before another write.",
      tone: "neutral",
    };
  }
  if (phase === "settled" && state && state !== "applied" && state !== "reconciled") {
    const target = workspace?.publicationPrNumber
      ? `PR #${workspace.publicationPrNumber}`
      : workspace?.publicationPrUrl
        ? "the linked pull request"
        : null;
    return {
      ...getAgentWorkspaceDescriptionFailurePresentation(target, state),
      tone: "error",
    };
  }
  return null;
}

function normalizePublicationStatus(status: string | null | undefined): string | null {
  const normalized = status?.trim().toLowerCase();
  return normalized || null;
}

export function getAgentWorkspaceTerminalPublicationStatus(
  workspace: AgentConversationWorkspace | null
): "merged" | "closed" | null {
  const status = normalizePublicationStatus(workspace?.publicationPrStatus);
  if (status === "merged") {
    return "merged";
  }
  if (status === "closed") {
    return "closed";
  }
  return null;
}

export function getAgentWorkspaceTerminalPublicationLabel(
  workspace: AgentConversationWorkspace | null
): string | null {
  const status = getAgentWorkspaceTerminalPublicationStatus(workspace);
  if (status === "merged") {
    return "Merged";
  }
  if (status === "closed") {
    return "Closed";
  }
  return null;
}

export type AgentWorkspaceMaintenancePresentation = {
  action: "hold" | "none" | "publish" | "retry";
  automaticContinuation: string | null;
  busy: boolean;
  summary: string;
  title: string;
  tone: "error" | "neutral" | "warning";
};

export type AgentWorkspacePrAutofixFingerprintSpendPresentation = {
  summary: string;
  exhausted: boolean;
};

export type AgentWorkspaceHoldPrimaryActionKind =
  | "recheck"
  | "rerunChecks"
  | "retryRepair"
  | "retryPublication";

export type AgentWorkspaceHoldActionKind = AgentWorkspaceHoldPrimaryActionKind | "stop";

export type AgentWorkspaceHoldPresentation = {
  pill: string;
  courtChip: { label: string; autoResumes: boolean };
  headline: string;
  paragraph: string;
  technicalDetails: string | null;
  primary: { kind: AgentWorkspaceHoldPrimaryActionKind; label: string; caption: string };
  secondary: { kind: AgentWorkspaceHoldActionKind; label: string; tooltip: string } | null;
  more: Array<{ kind: AgentWorkspaceHoldActionKind; label: string; caption: string }>;
  releaseConditions: string[];
};

const AGENT_WORKSPACE_HOLD_PILL = "Auto-repair paused";

type HoldCopyPrimaryCaption = (
  spend: AgentWorkspacePrAutofixFingerprintSpendPresentation | null,
) => string;

type HoldCopy = {
  courtChip: { label: string; autoResumes: boolean };
  headline: string;
  paragraph: string;
  primary: {
    kind: AgentWorkspaceHoldPrimaryActionKind;
    label: string;
    caption: HoldCopyPrimaryCaption;
  };
  secondary: { kind: AgentWorkspaceHoldActionKind; label: string; tooltip: string } | null;
  more: Array<{ kind: AgentWorkspaceHoldActionKind; label: string; caption: string }>;
  releaseConditions: string[];
};

const STOP_AUTO_REPAIR_MORE_ITEM = {
  kind: "stop" as const,
  label: "Stop auto-repair",
  caption: "Stops RalphX from retrying this failure automatically.",
};

const HOLD_COPY: Record<AgentWorkspaceMaintenanceOperationHoldReason, HoldCopy> = {
  pr_autofix_unchanged_health: {
    courtChip: { label: "Waiting on you", autoResumes: false },
    headline: "Repair paused — waiting for new CI evidence",
    paragraph:
      "The fixer ran but changed nothing, and GitHub still reports the same failing check. RalphX won't spend another generation until this PR's health changes.",
    primary: {
      kind: "retryRepair",
      label: "Retry repair anyway",
      caption: (spend) =>
        spend
          ? `Spends another generation despite the repeat failure. ${spend.summary}${spend.exhausted ? " — budget exhausted" : ""}.`
          : "Spends another generation despite the repeat failure.",
    },
    secondary: {
      kind: "recheck",
      label: "Re-check PR health",
      tooltip: "Checks GitHub for a new result before spending another generation.",
    },
    more: [STOP_AUTO_REPAIR_MORE_ITEM],
    releaseConditions: [
      "GitHub reports a different failing check or a passing result for this PR.",
    ],
  },
  pr_autofix_pre_existing_on_base: {
    courtChip: { label: "Waiting on main", autoResumes: true },
    headline: "Repair paused — failure exists on the base branch",
    paragraph:
      "This failure already exists on the base branch, so fixing it on this PR branch would not help.",
    primary: {
      kind: "recheck",
      label: "Re-check PR health",
      caption: () => "Checks whether the base branch's failure has changed.",
    },
    secondary: null,
    more: [
      {
        kind: "stop",
        label: "Stop auto-repair",
        caption: "Stops RalphX from retrying a failure the base branch also has.",
      },
    ],
    releaseConditions: [
      "The base branch's health changes to something other than this same failure.",
    ],
  },
  pr_autofix_ci_rerun_pending: {
    courtChip: { label: "Waiting on a re-run", autoResumes: true },
    headline: "Repair paused — waiting for CI rerun",
    paragraph:
      "RalphX asked GitHub to re-run the failed jobs and is waiting for the result.",
    primary: {
      kind: "recheck",
      label: "Re-check PR health",
      caption: () => "Checks GitHub for the rerun's result.",
    },
    secondary: null,
    more: [
      {
        kind: "stop",
        label: "Stop auto-repair",
        caption: "Stops RalphX from waiting on this rerun.",
      },
    ],
    releaseConditions: ["GitHub reports a result for the re-run jobs."],
  },
  base_stale: {
    courtChip: { label: "Waiting on you", autoResumes: false },
    headline: "Behind base — update did not take",
    paragraph:
      "The branch is still behind its targeted base commit after RalphX attempted the update.",
    primary: {
      kind: "recheck",
      label: "Re-check PR health",
      caption: () => "Checks whether the base branch update has landed.",
    },
    secondary: null,
    more: [],
    releaseConditions: ["The workspace successfully updates to its targeted base commit."],
  },
  health_evidence: {
    courtChip: { label: "Waiting on GitHub — resumes on its own", autoResumes: true },
    headline: "Holding — waiting for new CI evidence",
    paragraph:
      "RalphX is waiting for GitHub to report new PR health evidence before retrying.",
    primary: {
      kind: "recheck",
      label: "Re-check PR health",
      caption: () => "Checks GitHub for a newer health signal.",
    },
    secondary: null,
    more: [],
    releaseConditions: ["GitHub reports new PR health evidence."],
  },
  publish_redrive: {
    courtChip: { label: "Waiting on GitHub — resumes on its own", autoResumes: true },
    headline: "Pushing rebased branch…",
    paragraph: "RalphX is resuming publication for the rebased branch.",
    primary: {
      kind: "retryPublication",
      label: "Retry publication",
      caption: () => "RalphX is already pushing the rebased branch — no action needed.",
    },
    secondary: null,
    more: [],
    releaseConditions: ["The rebased branch finishes pushing to GitHub."],
  },
  publication_effect_attention: {
    courtChip: { label: "Waiting on you", autoResumes: false },
    headline: "Repair paused — publish not confirmed",
    paragraph:
      "RalphX can't confirm whether an earlier publish step reached GitHub. It stopped rather than risk pushing twice. Retry publication to have it check again and continue.",
    primary: {
      kind: "retryPublication",
      label: "Retry publication",
      caption: () => "Pushes the repair again and confirms it reaches GitHub.",
    },
    secondary: null,
    more: [],
    releaseConditions: ["RalphX confirms the publish step's state on GitHub."],
  },
  pr_autofix_base_parity_transient: {
    courtChip: { label: "Waiting on a re-run", autoResumes: true },
    headline: "Repair paused — checks were cancelled or timed out",
    paragraph:
      "GitHub cancelled or timed out the checks, and the same failure is present on the base branch. Re-running the checks can clear this.",
    primary: {
      kind: "rerunChecks",
      label: "Re-run failed checks",
      caption: (spend) =>
        spend
          ? `Asks GitHub to run the cancelled or timed-out jobs again — no agent runs, no commit is made. ${spend.summary}.`
          : "Asks GitHub to run the cancelled or timed-out jobs again — no agent runs, no commit is made.",
    },
    secondary: {
      kind: "retryRepair",
      label: "Retry repair anyway",
      tooltip: "Spends a generation on the base-branch failure instead of re-running checks.",
    },
    more: [
      {
        kind: "stop",
        label: "Stop auto-repair",
        caption: "Stops RalphX from waiting on this checks state.",
      },
    ],
    releaseConditions: [
      "The re-run checks report a different result, or GitHub reports different PR health.",
    ],
  },
};

export function getAgentWorkspacePrAutofixFingerprintSpendPresentation(
  workspace: AgentConversationWorkspace | null | undefined,
): AgentWorkspacePrAutofixFingerprintSpendPresentation | null {
  const spend = workspace?.prAutofixFingerprintSpend;
  if (!spend) {
    return null;
  }
  return {
    summary: `${spend.generations} generations · ${spend.minutes} min on this failure`,
    exhausted: spend.isExhausted,
  };
}

/**
 * Gates the Automation tab's budget card on a reportable spend value rather than on
 * presence: the backend returns a zeroed spend for any workspace that merely has a
 * fingerprint, so a null check would leave a permanent "0 min · 0 generations" card.
 */
export function hasReportableAutofixSpend(
  spend: AgentWorkspacePrAutofixFingerprintSpend | null | undefined,
): boolean {
  return Boolean(
    spend && (spend.generations > 0 || spend.minutes > 0 || spend.isExhausted),
  );
}

export function getAgentWorkspaceMaintenanceOperation(
  workspace: AgentConversationWorkspace | null | undefined,
): AgentWorkspaceMaintenanceOperation | null {
  return workspace?.maintenanceOperation ?? null;
}

export function getAgentWorkspaceHoldPresentation(
  workspace: AgentConversationWorkspace | null | undefined,
): AgentWorkspaceHoldPresentation | null {
  const operation = getAgentWorkspaceMaintenanceOperation(workspace);
  if (operation?.stage !== "held") {
    return null;
  }
  const copy: HoldCopy = operation.holdReason
    ? HOLD_COPY[operation.holdReason]
    : {
        courtChip: { label: "Waiting on GitHub — resumes on its own", autoResumes: true },
        headline: "Repair paused",
        paragraph: "RalphX paused this repair until new PR health evidence is available.",
        primary: {
          kind: "recheck",
          label: "Re-check PR health",
          caption: () => "Checks GitHub for new results on this PR.",
        },
        secondary: null,
        more: [],
        releaseConditions: ["New PR health evidence from GitHub."],
      };
  const spend = getAgentWorkspacePrAutofixFingerprintSpendPresentation(workspace);
  return {
    pill: AGENT_WORKSPACE_HOLD_PILL,
    courtChip: copy.courtChip,
    headline: copy.headline,
    // An agent's own structured account wins, because it describes this specific repair. Anything
    // else takes the curated per-hold-reason copy.
    //
    // The raw `operation.summary` is deliberately NOT in this chain. It is machine-written backend
    // text, and using it as the paragraph is what put "RalphX retained the effect fence and did not
    // reacquire or release Git authority: Conflict: …" in front of users while this curated copy
    // sat unreachable. It is still available, one click away, as `technicalDetails`.
    paragraph: composeAgentWorkspaceOperationNarrative(operation) ?? copy.paragraph,
    technicalDetails: nonBlank(operation.summary),
    primary: {
      kind: copy.primary.kind,
      label: copy.primary.label,
      caption: copy.primary.caption(spend),
    },
    secondary: copy.secondary,
    more: copy.more,
    releaseConditions: copy.releaseConditions,
  };
}

export function isAgentWorkspaceMaintenanceActive(
  workspace: AgentConversationWorkspace | null | undefined,
): boolean {
  return Boolean(
    !getAgentWorkspaceTerminalPublicationStatus(workspace ?? null) &&
      getAgentWorkspaceMaintenanceOperation(workspace)?.status === "active",
  );
}

export function blocksAgentWorkspaceGitInspection(
  workspace: AgentConversationWorkspace | null | undefined,
): boolean {
  if (!isAgentWorkspaceMaintenanceActive(workspace)) {
    return false;
  }
  const stage = getAgentWorkspaceMaintenanceOperation(workspace)?.stage;
  return (
    stage === "updating_base" ||
    stage === "repairing" ||
    stage === "validating" ||
    stage === "publishing"
  );
}

export function canResumeAgentWorkspacePublish(
  workspace: AgentConversationWorkspace | null | undefined,
): boolean {
  const operation = getAgentWorkspaceMaintenanceOperation(workspace);
  return Boolean(
    !getAgentWorkspaceTerminalPublicationStatus(workspace ?? null) &&
      operation?.status === "ready" &&
      operation.stage === "ready" &&
      operation.recoveryAction === "resume_publish" &&
      operation.holdReason == null,
  );
}

export type AgentWorkspaceMaintenancePublishGateInput = {
  hasPublishHandler: boolean;
  isManagedByTaskPipeline: boolean;
  effectivePublishing: boolean;
  isAutomationPreferenceSaving: boolean;
  baseBlocked: boolean;
  reviewBlocksPublish: boolean;
  reviewIsRunning: boolean;
  reviewGateStatus: AgentWorkspaceReviewGateStatus | null;
  reviewGateSummary: string | null;
  hasPrConflict: boolean;
  hasTerminalPublication: boolean;
  workspaceMissing: boolean;
};

export type AgentWorkspaceMaintenancePublishGate = {
  disabled: boolean;
  /** Review-state override label; null → caller uses its branch default label. */
  label: string | null;
  /** User-facing reason for the disabled state; null when enabled. */
  blockedReason: string | null;
};

/**
 * Single verdict for the maintenance banner's Resume publish / Retry repair action.
 *
 * The banner buttons must use this for BOTH their `disabled` prop and their click
 * guard, so an enabled-looking button can never silently refuse the click.
 *
 * Deliberately NOT inputs, because the backend resume path is designed for them:
 * - `hasNoDetectedChanges` — zero local delta is the expected post-repair state.
 * - `isPublishCurrent` — the parked durable attempt must still settle, or the
 *   banner strands with no way forward.
 * - `repositoryInspectionFailed` — the backend resume re-validates and fails safely.
 * - `isRepairPending` — structurally false whenever a maintenance operation exists.
 */
export function getAgentWorkspaceMaintenancePublishGate(
  input: AgentWorkspaceMaintenancePublishGateInput,
): AgentWorkspaceMaintenancePublishGate {
  const label = input.reviewBlocksPublish
    ? input.reviewIsRunning
      ? "Reviewing"
      : input.reviewGateStatus === "required"
        ? "Review required"
        : input.reviewGateStatus === "blocking"
          ? "Review blocking"
          : input.reviewGateStatus === "failed"
            ? "Review failed"
            : null
    : null;

  const blockedReason = (() => {
    if (input.reviewBlocksPublish) {
      return (
        input.reviewGateSummary ??
        "Workspace Review must settle before publishing."
      );
    }
    if (!input.hasPublishHandler) {
      return "Publishing is unavailable for this workspace.";
    }
    if (input.isManagedByTaskPipeline) {
      return "Publishing is managed by this ideation plan's task pipeline.";
    }
    if (input.workspaceMissing) {
      return "The workspace files are missing.";
    }
    if (input.hasTerminalPublication) {
      return "The linked pull request is already merged or closed.";
    }
    if (input.baseBlocked) {
      return "Publishing is blocked until the workspace base branch is resolved.";
    }
    if (input.hasPrConflict) {
      return "Resolve the pull request conflicts before publishing.";
    }
    if (input.effectivePublishing) {
      return "A workspace operation is already in progress.";
    }
    if (input.isAutomationPreferenceSaving) {
      return "Saving automation preferences. Try again in a moment.";
    }
    return null;
  })();

  return {
    disabled: blockedReason !== null,
    label,
    blockedReason,
  };
}

/** A blank string carries no information; treat it the same as absent. */
function nonBlank(value: string | null | undefined): string | null {
  return value && value.trim().length > 0 ? value : null;
}

/**
 * Composes the agent's own whatHappened/whatIDid narrative verbatim. Recovery/poller/review-gate
 * sites preserve this narrative even when they separately author a machine blocker, so the
 * blocker (when present) is appended as a clearly RalphX-attributed sentence rather than folded
 * into the agent's account.
 */
function composeAgentWorkspaceOperationNarrative(
  operation: AgentWorkspaceMaintenanceOperation,
): string | null {
  const parts = [operation.whatHappened, operation.whatIDid].filter(
    (part): part is string => typeof part === "string" && part.length > 0,
  );
  if (parts.length === 0) {
    return null;
  }
  const narrative = parts.join(" ");
  return operation.blocker
    ? `${narrative} Separately, RalphX reports: ${operation.blocker}`
    : narrative;
}

export function getAgentWorkspaceMaintenancePresentation(
  workspace: AgentConversationWorkspace | null | undefined,
): AgentWorkspaceMaintenancePresentation | null {
  if (getAgentWorkspaceTerminalPublicationStatus(workspace ?? null)) {
    return null;
  }
  const operation = getAgentWorkspaceMaintenanceOperation(workspace);
  if (!operation) {
    return null;
  }

  const automaticContinuation =
    operation.status === "active" && operation.automaticContinuation
      ? "Will continue automatically."
      : null;
  const narrative = composeAgentWorkspaceOperationNarrative(operation);
  const legacySummary =
    operation.blocker ?? operation.summary ?? "RalphX is continuing this workspace operation.";
  const summary = narrative ?? legacySummary;
  switch (operation.stage) {
    case "updating_base":
      return {
        title: "Updating base",
        summary,
        tone: "neutral",
        busy: true,
        action: "none",
        automaticContinuation,
      };
    case "repairing":
      return {
        title: "Repairing workspace",
        summary,
        tone: "warning",
        busy: true,
        action: "none",
        automaticContinuation,
      };
    case "validating":
      return {
        title: "Validating repair",
        summary,
        tone: "neutral",
        busy: true,
        action: "none",
        automaticContinuation,
      };
    case "reviewing":
      return {
        title: "Workspace Review in progress",
        summary,
        tone: "neutral",
        busy: true,
        action: "none",
        automaticContinuation,
      };
    case "publishing":
      return {
        title: "Publishing workspace",
        summary,
        tone: "neutral",
        busy: true,
        action: "none",
        automaticContinuation,
      };
    case "held": {
      const hold = getAgentWorkspaceHoldPresentation(workspace);
      // For the maintenance banner summary: prefer narrative, then the raw operation
      // summary (diagnostic detail), then the curated per-hold-reason template as
      // a last resort. The hold card paragraph has a different chain (narrative →
      // curated copy, with the raw summary surfaced as technicalDetails instead).
      const heldSummary =
        narrative ?? nonBlank(operation.summary) ?? hold?.paragraph ?? summary;
      return {
        title: hold?.headline ?? "Repair paused",
        summary: heldSummary,
        tone: "warning",
        busy: false,
        action: "hold",
        automaticContinuation: null,
      };
    }
    case "ready":
      if (operation.holdReason === "publish_redrive") {
        return {
          title: "Pushing rebased branch…",
          summary,
          tone: "neutral",
          busy: true,
          action: "none",
          automaticContinuation: "RalphX is resuming publication automatically.",
        };
      }
      if (operation.holdReason === "base_stale") {
        return {
          title: "Behind base — update did not take",
          summary,
          tone: "warning",
          busy: false,
          action: "none",
          automaticContinuation: null,
        };
      }
      if (operation.holdReason === "health_evidence") {
        return {
          title: "Holding — waiting for new CI evidence",
          summary,
          tone: "warning",
          busy: false,
          action: "none",
          automaticContinuation:
            "RalphX will continue when the PR evidence changes.",
        };
      }
      return {
        title: "Base updated — ready to publish",
        summary,
        tone: "warning",
        busy: false,
        action: operation.recoveryAction === "resume_publish" ? "publish" : "none",
        automaticContinuation: null,
      };
    case "blocked":
      return {
        title: "Repair blocked",
        summary,
        tone: "error",
        busy: false,
        action: operation.recoveryAction === "retry_repair" ? "retry" : "none",
        automaticContinuation: null,
      };
  }
}

export function isAgentWorkspacePublishActive(
  workspace: AgentConversationWorkspace | null | undefined,
): boolean {
  if (!workspace || getAgentWorkspaceTerminalPublicationStatus(workspace)) {
    return false;
  }
  if (
    workspace.publicationMetadataPhase !== null &&
    ACTIVE_METADATA_RECEIPT_PHASES.has(workspace.publicationMetadataPhase)
  ) {
    return true;
  }
  const pushStatus = normalizePublicationStatus(workspace.publicationPushStatus);
  return (
    pushStatus !== null && AGENT_WORKSPACE_ACTIVE_PUBLISH_STATUSES.has(pushStatus)
  );
}

export function isPipelineOwnedAgentWorkspace(
  workspace: AgentConversationWorkspace | null | undefined
): boolean {
  return Boolean(workspace?.linkedPlanBranchId);
}

function isAgentWorkspacePublishSurfaceMode(
  workspace: AgentConversationWorkspace,
): boolean {
  if (
    workspace.mode === "edit" ||
    workspace.mode === "plan" ||
    workspace.mode === "automation"
  ) {
    return true;
  }

  if (workspace.mode === "ideation") {
    return isPipelineOwnedAgentWorkspace(workspace);
  }

  return false;
}

export function getAgentWorkspacePrConflictSummary(
  workspace: AgentConversationWorkspace | null | undefined,
): string | null {
  const status = workspace?.prSupervisionStatus?.trim().toLowerCase();
  const summary = workspace?.prSupervisionSummary?.trim() ?? "";
  if (status !== "blocked" || !summary) {
    return null;
  }

  const normalized = summary.toLowerCase();
  if (
    normalized.includes("merge conflict") ||
    normalized.includes("reported as conflicting") ||
    normalized.includes("mergeability blocker")
  ) {
    return summary;
  }
  return null;
}

export type AgentWorkspaceReviewActionBlocker = {
  kind: "repair" | "conflict";
  message: string;
};

export function getAgentWorkspaceReviewActionBlocker(
  workspace: AgentConversationWorkspace | null | undefined,
): AgentWorkspaceReviewActionBlocker | null {
  if (!workspace || getAgentWorkspaceTerminalPublicationStatus(workspace)) {
    return null;
  }
  if (isAgentWorkspaceMaintenanceActive(workspace)) {
    return {
      kind: "repair",
      message: "Finish or abort the current repair, then retry Review.",
    };
  }
  const pushStatus = normalizePublicationStatus(workspace.publicationPushStatus);
  const supervisionStatus = normalizePublicationStatus(workspace.prSupervisionStatus);
  if (pushStatus === "needs_agent" || supervisionStatus === "fixing") {
    return {
      kind: "repair",
      message: "Finish or abort the current repair, then retry Review.",
    };
  }
  if (getAgentWorkspacePrConflictSummary(workspace)) {
    return {
      kind: "conflict",
      message: "Resolve conflicts before retrying Review.",
    };
  }
  return null;
}

export function isAgentWorkspaceAutoMergeRequestPending({
  autoMergeCurrent,
  autoMergeDesired,
  hasPublishedPr,
  prSupervisionStatus,
  publicationPushStatus,
  terminalPublicationStatus,
}: {
  autoMergeCurrent?: boolean | null;
  autoMergeDesired?: boolean;
  hasPublishedPr?: boolean;
  prSupervisionStatus?: string | null;
  publicationPushStatus?: string | null;
  terminalPublicationStatus?: string | null;
}): boolean {
  const normalizedSupervisionStatus =
    prSupervisionStatus?.trim().toLowerCase() || null;
  return Boolean(
    autoMergeDesired &&
      publicationPushStatus === "pushed" &&
      hasPublishedPr &&
      autoMergeCurrent !== true &&
      !terminalPublicationStatus &&
      normalizedSupervisionStatus === null,
  );
}

export function isAgentWorkspaceAutoMergeDeferred({
  autoMergeCurrent,
  autoMergeDesired,
  hasPublishedPr,
  prSupervisionStatus,
  publicationPushStatus,
  terminalPublicationStatus,
}: {
  autoMergeCurrent?: boolean | null;
  autoMergeDesired?: boolean;
  hasPublishedPr?: boolean;
  prSupervisionStatus?: string | null;
  publicationPushStatus?: string | null;
  terminalPublicationStatus?: string | null;
}): boolean {
  const normalizedSupervisionStatus =
    prSupervisionStatus?.trim().toLowerCase() || null;
  return Boolean(
    autoMergeDesired &&
      publicationPushStatus === "pushed" &&
      hasPublishedPr &&
      autoMergeCurrent !== true &&
      !terminalPublicationStatus &&
      normalizedSupervisionStatus === "waiting",
  );
}

export function shouldShowAgentWorkspacePublishSurface(
  workspace: AgentConversationWorkspace | null | undefined
): boolean {
  return Boolean(workspace && isAgentWorkspacePublishSurfaceMode(workspace));
}

export function canInspectAgentWorkspacePublishDiffs(
  workspace: AgentConversationWorkspace | null | undefined,
  options: { includeTerminalPublished?: boolean } = {},
): boolean {
  if (!workspace) {
    return false;
  }

  const isInspectableMode = isAgentWorkspacePublishSurfaceMode(workspace);

  if (isInspectableMode && workspace.status !== "missing") {
    return true;
  }

  return Boolean(
    options.includeTerminalPublished &&
      workspace.mode === "edit" &&
      hasPublishedWorkspacePr(workspace) &&
      getAgentWorkspaceTerminalPublicationStatus(workspace),
  );
}

export function canInspectAgentWorkspaceBaseFreshness(
  workspace: AgentConversationWorkspace | null | undefined,
): boolean {
  if (!workspace) {
    return false;
  }

  if (isAgentWorkspacePublishSurfaceMode(workspace)) {
    return true;
  }

  return hasPublishedWorkspacePr(workspace);
}

export function isAgentWorkspacePublishCurrent(
  workspace: AgentConversationWorkspace | null,
  freshness: AgentConversationWorkspaceFreshness | undefined
): boolean {
  const freshnessScope = freshness?.freshnessScope ?? "full";
  const remoteRefreshed = freshness?.remoteRefreshed ?? true;
  const worktreeStatusChecked = freshness?.worktreeStatusChecked ?? true;
  const pushStatus = normalizePublicationStatus(workspace?.publicationPushStatus);
  return (
    hasPublishedWorkspacePr(workspace) &&
    (pushStatus === "pushed" || pushStatus === "refreshed") &&
    freshness !== undefined &&
    freshnessScope === "full" &&
    remoteRefreshed &&
    worktreeStatusChecked &&
    freshness.baseStatus !== "blocked" &&
    !freshness.isBaseAhead &&
    !freshness.hasUncommittedChanges &&
    freshness.unpublishedCommitCount === 0
  );
}

export function getPostBaselinePublicationEvents(
  events: AgentConversationWorkspacePublicationEvent[],
  lastEventId: string | null,
  startedAtMs: number,
): AgentConversationWorkspacePublicationEvent[] | null {
  let suffix = events;
  if (lastEventId !== null) {
    const baselineIndexes = events.flatMap((event, index) =>
      event.id === lastEventId ? [index] : [],
    );
    if (baselineIndexes.length !== 1) {
      return null;
    }
    suffix = events.slice((baselineIndexes[0] ?? 0) + 1);
  }

  const seenEventIds = new Set<string>();
  return suffix.filter((event) => {
    if (seenEventIds.has(event.id)) {
      return false;
    }
    seenEventIds.add(event.id);
    const createdAtMs = new Date(event.createdAt).getTime();
    return (
      Number.isFinite(createdAtMs) &&
      createdAtMs >= startedAtMs - PUBLISH_EVENT_START_SKEW_MS
    );
  });
}

export function classifyAgentWorkspacePublishTerminalEvent(
  events: AgentConversationWorkspacePublicationEvent[],
  workspace: AgentConversationWorkspace | null,
  freshness: AgentConversationWorkspaceFreshness | undefined,
): AgentWorkspacePublishTerminalEvent | null {
  const currentAttemptId = workspace?.publicationMetadataAttemptId;
  const authoritativeEvents = currentAttemptId
    ? events.filter(
        (event) =>
          event.attemptId === currentAttemptId || event.attemptId === null,
      )
    : events;
  const workspacePushStatus = normalizePublicationStatus(
    workspace?.publicationPushStatus,
  );
  for (const event of authoritativeEvents) {
    const step = event.step.trim().toLowerCase();
    const status = event.status.trim().toLowerCase();
    const classification = event.classification?.trim().toLowerCase() ?? null;
    if (
      (step === "published" && status === "succeeded") ||
      (step === "metadata_settled" &&
        status === "succeeded" &&
        (classification === "applied" || classification === "reconciled"))
    ) {
      if (isAgentWorkspacePublishCurrent(workspace, freshness)) {
        return { event, kind: "success" };
      }
      continue;
    }
    if (
      step === "metadata_settled" &&
      (status === "failed" || status === "skipped") &&
      (classification === "not_attempted" ||
        classification === "not_applied" ||
        classification === "conflicted")
    ) {
      return { event, kind: "failure" };
    }
    if (
      step === "needs_agent" &&
      status === "failed" &&
      (classification === "agent_fixable" || workspacePushStatus === "needs_agent")
    ) {
      return { event, kind: "needs_agent" };
    }
    if (
      (step === "failed" || step === "description_failed") &&
      status === "failed"
    ) {
      return { event, kind: "failure" };
    }
    if (step === "no_changes" && status === "skipped") {
      return { event, kind: "no_changes" };
    }
  }
  return null;
}

export function shouldAutoRefreshCleanAgentWorkspaceFromBase(
  workspace: AgentConversationWorkspace | null,
  freshness: AgentConversationWorkspaceFreshness | undefined
): boolean {
  const freshnessScope = freshness?.freshnessScope ?? "full";
  const remoteRefreshed = freshness?.remoteRefreshed ?? false;
  const worktreeStatusChecked = freshness?.worktreeStatusChecked ?? false;
  const baseStatus = freshness?.baseStatus ?? "valid";
  return (
    workspace?.mode === "edit" &&
    workspace.status !== "missing" &&
    freshness !== undefined &&
    freshnessScope === "full" &&
    remoteRefreshed &&
    worktreeStatusChecked &&
    baseStatus !== "blocked" &&
    freshness.isBaseAhead &&
    !freshness.hasUncommittedChanges &&
    freshness.unpublishedCommitCount === 0
  );
}

export function getAgentWorkspaceEffectiveBaseLabel(
  workspace: AgentConversationWorkspace | null,
  freshness: AgentConversationWorkspaceFreshness | undefined
): string {
  if (freshness?.baseStatus === "blocked") {
    return "Base unavailable";
  }
  if (workspace?.branchMode === "linked") {
    const linkedBaseRef =
      freshness?.effectiveBaseRef ??
      freshness?.baseRef ??
      workspace.baseRef;
    return linkedBaseRef.trim() ? linkedBaseRef : (workspace.baseDisplayName ?? "Base branch");
  }
  return (
    freshness?.effectiveBaseDisplayName ??
    freshness?.baseDisplayName ??
    freshness?.effectiveBaseRef ??
    freshness?.baseRef ??
    workspace?.baseDisplayName ??
    workspace?.baseRef ??
    "Base branch"
  );
}
