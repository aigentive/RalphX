import type { PlanComplexityAssessment } from "@/types/artifact";

export const PLAN_TO_PROPOSALS_REQUEST =
  "Proceed to proposals for the approved plan. Read the current plan, run cross_project_guide if needed, create implementation task proposals, and finalize proposals only after all proposal and dependency updates are complete.";

export const PLAN_IMPLEMENT_DIRECTLY_REQUEST =
  "Implement the approved plan directly. Use the linked plan as implementation guidance, edit the workspace branch, and do not create task proposals unless the user explicitly asks.";

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
    return "Assessing plan complexity. Both paths are available while the recommendation is prepared.";
  }

  return "Use direct implementation for small, linear plans; use proposals when the work needs tracked tasks or review checkpoints.";
}
