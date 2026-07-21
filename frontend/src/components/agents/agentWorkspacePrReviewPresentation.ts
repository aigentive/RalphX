import type {
  AgentWorkspacePrReviewAction,
  AgentWorkspacePrReviewActionHeadStatus,
  AgentWorkspacePrReviewActionKind,
  AgentWorkspacePrReviewContext,
} from "@/api/chat";

export interface AgentWorkspacePrReviewActionCopy {
  title: string;
  primaryLabel: string;
  submittedToast: string;
}

export interface AgentWorkspacePrReviewPresentation {
  isTerminal: boolean;
  pendingAction: AgentWorkspacePrReviewAction | null;
  headStatus: AgentWorkspacePrReviewActionHeadStatus | null;
  headDetail: string;
  canSubmit: boolean;
  submitBlockedMessage: string | null;
  consistencyMessage: string | null;
}

export function shortPrReviewSha(value: string | null | undefined): string {
  return value ? value.slice(0, 8) : "unknown";
}

export function prReviewActionCopy(
  actionKind: AgentWorkspacePrReviewActionKind,
): AgentWorkspacePrReviewActionCopy {
  switch (actionKind) {
    case "approve":
      return {
        title: "Approve PR proposed",
        primaryLabel: "Approve PR",
        submittedToast: "PR approved",
      };
    case "comment":
      return {
        title: "Review comment proposed",
        primaryLabel: "Submit Comment",
        submittedToast: "Review comment submitted",
      };
    case "request_changes":
    default:
      return {
        title: "Request changes proposed",
        primaryLabel: "Request Changes",
        submittedToast: "Changes requested",
      };
  }
}

export function prReviewStatusLabel(status: string): string {
  return status
    .split("_")
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

export function lastResolvedPrReviewAction(
  actions: AgentWorkspacePrReviewAction[],
): AgentWorkspacePrReviewAction | null {
  return (
    actions.find((action) =>
      ["approved", "submitted", "skipped", "failed", "superseded"].includes(
        action.status,
      ),
    ) ?? null
  );
}

export function getAgentWorkspacePrReviewPresentation(
  context: AgentWorkspacePrReviewContext,
): AgentWorkspacePrReviewPresentation {
  const publicationStatus = context.workspace.publicationPrStatus
    ?.trim()
    .toLowerCase();
  const isTerminal =
    publicationStatus === "merged" ||
    publicationStatus === "closed" ||
    context.monitor?.status === "terminal";
  const pendingAction =
    !isTerminal && context.pendingAction?.status === "pending"
      ? context.pendingAction
      : null;
  const headStatus = pendingAction
    ? (context.pendingActionHeadStatus ?? "unverified")
    : null;
  const hasReviewArtifact = Boolean(
    pendingAction &&
      context.monitor?.reviewArtifactId &&
      context.monitor.reviewArtifactHeadSha === pendingAction.headSha,
  );

  let submitBlockedMessage: string | null = null;
  if (pendingAction) {
    if (headStatus === "stale") {
      submitBlockedMessage = "PR head changed; a fresh review is required.";
    } else if (headStatus === "unverified") {
      submitBlockedMessage = "The remote PR head cannot currently be verified.";
    } else if (!hasReviewArtifact) {
      submitBlockedMessage = "Write the Review for this PR head before submitting.";
    }
  }

  const headDetail = pendingAction
    ? headStatus === "stale"
      ? `Reviewed head ${shortPrReviewSha(pendingAction.headSha)} · Verified current head ${shortPrReviewSha(context.currentHeadSha)}`
      : `Reviewed head ${shortPrReviewSha(pendingAction.headSha)}`
    : `Head ${shortPrReviewSha(context.currentHeadSha)}`;

  return {
    isTerminal,
    pendingAction,
    headStatus,
    headDetail,
    canSubmit: Boolean(
      pendingAction && headStatus === "current" && hasReviewArtifact,
    ),
    submitBlockedMessage,
    consistencyMessage:
      !isTerminal &&
      !pendingAction &&
      context.monitor?.status === "awaiting_user"
        ? "The reviewer proposal is temporarily unavailable. RalphX will keep trying to restore it."
        : null,
  };
}

export function shouldPollForPrReviewContext(
  context: AgentWorkspacePrReviewContext | null | undefined,
): boolean {
  if (!context) {
    return true;
  }
  return !getAgentWorkspacePrReviewPresentation(context).isTerminal;
}
