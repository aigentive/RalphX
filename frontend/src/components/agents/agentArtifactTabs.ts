import type { AgentArtifactTab } from "@/stores/agentSessionStore";

export type IdeationArtifactTab = Exclude<
  AgentArtifactTab,
  "publish" | "jira" | "linear" | "granola"
>;

export interface IdeationArtifactAvailability {
  hasAttachedIdeationSession: boolean;
  hasPlanArtifact: boolean;
  isPlanCapable?: boolean;
  hasProposals: boolean;
  hasVerificationEvidence: boolean;
  hasExecutionTasks: boolean;
  artifactMode: string | null | undefined;
}

export function getVisibleIdeationArtifactTabs({
  hasAttachedIdeationSession,
  hasPlanArtifact,
  isPlanCapable = false,
  hasProposals,
  hasVerificationEvidence,
  hasExecutionTasks,
  artifactMode,
}: IdeationArtifactAvailability): IdeationArtifactTab[] {
  const hasCurrentPlan = hasAttachedIdeationSession && hasPlanArtifact;
  const canShowPlanTab = isPlanCapable || hasCurrentPlan;

  if (!canShowPlanTab) {
    return [];
  }

  const canShowProposalTab = artifactMode === "plan" || artifactMode === "ideation";

  return [
    "plan",
    ...(hasCurrentPlan && hasVerificationEvidence ? ["verification" as const] : []),
    ...(hasCurrentPlan && hasProposals && canShowProposalTab
      ? ["proposal" as const]
      : []),
    ...(hasCurrentPlan && hasExecutionTasks ? ["tasks" as const] : []),
  ];
}
