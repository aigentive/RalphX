import type {
  AgentWorkspaceReviewContext,
  StartAgentWorkspaceReviewResult,
} from "@/api/chat";

export type WorkspaceReviewAuthorizationContext = Pick<
  AgentWorkspaceReviewContext | StartAgentWorkspaceReviewResult,
  | "target"
  | "monitor"
  | "reviewArtifactIsCurrent"
  | "reviewArtifactIsOutdated"
>;

export function isWorkspaceReviewApprovedAnyway(
  context: WorkspaceReviewAuthorizationContext | null,
): boolean {
  const target = context?.target;
  const monitor = context?.monitor;
  return Boolean(
    target &&
      monitor &&
      context.reviewArtifactIsCurrent &&
      !context.reviewArtifactIsOutdated &&
      monitor.status === "ready" &&
      monitor.reviewOutcome === "blocking" &&
      monitor.reviewGateStatus === "passed" &&
      monitor.reviewGateBypassedAt &&
      monitor.reviewGateBypassedTargetScope === target.scope &&
      monitor.reviewGateBypassedDiffFingerprint === target.diffFingerprint &&
      monitor.reviewGateBypassedArtifactId === monitor.reviewArtifactId &&
      monitor.reviewGateBypassedArtifactVersion === monitor.reviewArtifactVersion,
  );
}

export function hasWorkspaceReviewPublishAuthorization(
  context: WorkspaceReviewAuthorizationContext | null,
): boolean {
  if (isWorkspaceReviewApprovedAnyway(context)) return true;
  return Boolean(
    context?.reviewArtifactIsCurrent &&
      !context.reviewArtifactIsOutdated &&
      context.monitor.status === "ready" &&
      context.monitor.reviewGateStatus === "passed" &&
      context.monitor.reviewOutcome !== "blocking" &&
      context.monitor.reviewOutcome !== "run_failed",
  );
}

export function isWorkspaceReviewBlockingPublish(
  context: WorkspaceReviewAuthorizationContext | null,
): boolean {
  return Boolean(
    context &&
      !isWorkspaceReviewApprovedAnyway(context) &&
      (context.monitor.status === "blocked" ||
        context.monitor.reviewGateStatus === "blocking" ||
        context.monitor.reviewGateStatus === "failed" ||
        context.monitor.reviewOutcome === "blocking" ||
        context.monitor.reviewOutcome === "run_failed"),
  );
}
