import type { AgentArtifactTab } from "@/stores/agentSessionStore";

export type IdeationArtifactTab = Exclude<AgentArtifactTab, "publish" | "jira" | "linear">;

export interface IdeationArtifactAvailability {
  hasAttachedIdeationSession: boolean;
  hasPlanArtifact: boolean;
  hasProposals: boolean;
  hasVerificationEvidence: boolean;
  hasExecutionTasks: boolean;
  artifactMode: string | null | undefined;
}

export function getVisibleIdeationArtifactTabs({
  hasAttachedIdeationSession,
  hasPlanArtifact,
  hasProposals,
  hasVerificationEvidence,
  hasExecutionTasks,
  artifactMode,
}: IdeationArtifactAvailability): IdeationArtifactTab[] {
  if (!hasAttachedIdeationSession || !hasPlanArtifact) {
    return [];
  }

  const canShowProposalTab = artifactMode === "plan" || artifactMode === "ideation";

  return [
    "plan",
    ...(hasVerificationEvidence ? ["verification" as const] : []),
    ...(hasProposals && canShowProposalTab ? ["proposal" as const] : []),
    ...(hasExecutionTasks ? ["tasks" as const] : []),
  ];
}
