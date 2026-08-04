import type {
  AgentConversationWorkspace,
  AgentConversationWorkspaceFreshness,
  AgentConversationWorkspacePublicationEvent,
  AgentWorkspaceMaintenanceOperation,
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
  action: "none" | "publish" | "retry";
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

export function getAgentWorkspaceMaintenanceOperation(
  workspace: AgentConversationWorkspace | null | undefined,
): AgentWorkspaceMaintenanceOperation | null {
  return workspace?.maintenanceOperation ?? null;
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
      operation.holdReason == null,
  );
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
  const summary = operation.blocker ?? operation.summary ?? "RalphX is continuing this workspace operation.";
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
        action: "publish",
        automaticContinuation: null,
      };
    case "blocked":
      return {
        title: "Repair blocked",
        summary,
        tone: "error",
        busy: false,
        action: "retry",
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
