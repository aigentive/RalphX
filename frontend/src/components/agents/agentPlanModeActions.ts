import type { PlanComplexityAssessment } from "@/types/artifact";

export const PLAN_TO_PROPOSALS_REQUEST =
  "Create implementation task proposals for the approved plan bundle. Read both the Plan Overview and Implementation Blueprint, run cross_project_guide if needed, define the complete proposal set with exact blueprint lineage, and analyze dependencies. Stop after proposal and dependency updates; do not finalize, apply, accept, or schedule tasks.";

export const PLAN_IMPLEMENT_DIRECTLY_REQUEST =
  "Implement the approved plan directly. Use both the linked Plan Overview and Implementation Blueprint as implementation guidance and edit the workspace branch.";

const PLAN_RECOMMENDATION_PENDING_WINDOW_MS = 2 * 60 * 1000;

export function isPlanRecommendationCheckPending({
  assessment,
  isFetching,
  approvedAt,
  nowMs = Date.now(),
}: {
  assessment: PlanComplexityAssessment | null | undefined;
  isFetching: boolean;
  approvedAt: string | null | undefined;
  nowMs?: number;
}): boolean {
  if (assessment) {
    return false;
  }
  if (!approvedAt) {
    return isFetching;
  }

  const approvedAtMs = Date.parse(approvedAt);
  if (!Number.isFinite(approvedAtMs)) {
    return false;
  }

  const elapsedMs = nowMs - approvedAtMs;
  return elapsedMs >= 0 && elapsedMs <= PLAN_RECOMMENDATION_PENDING_WINDOW_MS;
}

export function buildPlanActionHint({
  assessment,
  isAssessing,
  canChoose,
}: {
  assessment: PlanComplexityAssessment | null | undefined;
  isAssessing: boolean;
  canChoose: boolean;
}): string | null {
  if (!canChoose) {
    return null;
  }

  if (assessment) {
    const recommendation =
      assessment.recommendedAction === "create_proposals"
        ? "Create Proposals"
        : "Implement Directly";
    return `Recommended: ${recommendation}. ${assessment.reasonSummary} Both paths remain available.`;
  }

  if (isAssessing) {
    return "Checking recommended next action. Actions are disabled until the recommendation is ready.";
  }

  return "Use direct implementation for small, linear plans; use proposals when the work needs tracked tasks or review checkpoints.";
}
