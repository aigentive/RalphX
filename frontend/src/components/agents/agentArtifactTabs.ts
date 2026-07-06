import type { AgentArtifactTab } from "@/stores/agentSessionStore";

export type IdeationArtifactTab = Exclude<
  AgentArtifactTab,
  "publish" | "jira" | "linear" | "granola"
>;

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
  const shouldShowProposals =
    hasProposals && (artifactMode === "plan" || artifactMode === "ideation");

  return [
    "plan",
    ...(shouldShowProposals ? ["proposal" as const] : []),
    ...(hasVerificationEvidence ? ["verification" as const] : []),
    ...(hasExecutionTasks ? ["tasks" as const] : []),
  ];
}
