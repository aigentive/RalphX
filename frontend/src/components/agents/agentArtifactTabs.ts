import type { AgentArtifactTab } from "@/stores/agentSessionStore";

export type IdeationArtifactTab = Exclude<
  AgentArtifactTab,
  "publish" | "jira" | "linear" | "granola"
>;

export interface IdeationArtifactAvailability {
  hasAttachedIdeationSession: boolean;
  hasPlanArtifact: boolean;
  canStartPlan: boolean;
  hasProposals: boolean;
  hasVerificationEvidence: boolean;
  hasExecutionTasks: boolean;
  artifactMode: string | null | undefined;
}

export function getVisibleIdeationArtifactTabs({
  hasAttachedIdeationSession,
  hasPlanArtifact,
  canStartPlan,
  hasProposals,
  hasVerificationEvidence,
  hasExecutionTasks,
  artifactMode,
}: IdeationArtifactAvailability): IdeationArtifactTab[] {
  if (!hasPlanArtifact && !canStartPlan) {
    return [];
  }

  const canShowDataDrivenTabs = hasAttachedIdeationSession && hasPlanArtifact;
  const canShowProposalTab = artifactMode === "plan" || artifactMode === "ideation";

  return [
    "plan",
    ...(canShowDataDrivenTabs && hasVerificationEvidence ? ["verification" as const] : []),
    ...(canShowDataDrivenTabs && hasProposals && canShowProposalTab ? ["proposal" as const] : []),
    ...(canShowDataDrivenTabs && hasExecutionTasks ? ["tasks" as const] : []),
  ];
}
