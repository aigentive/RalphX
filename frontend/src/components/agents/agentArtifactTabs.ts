import type { AgentArtifactTab } from "@/stores/agentSessionStore";

export type IdeationArtifactTab = Exclude<AgentArtifactTab, "publish" | "jira">;

export interface IdeationArtifactAvailability {
  hasAttachedIdeationSession: boolean;
  hasPlanArtifact: boolean;
  hasVerificationEvidence: boolean;
  hasExecutionTasks: boolean;
}

export function getVisibleIdeationArtifactTabs({
  hasAttachedIdeationSession,
  hasPlanArtifact,
  hasVerificationEvidence,
  hasExecutionTasks,
}: IdeationArtifactAvailability): IdeationArtifactTab[] {
  if (!hasAttachedIdeationSession || !hasPlanArtifact) {
    return [];
  }

  return [
    "plan",
    ...(hasVerificationEvidence ? ["verification" as const] : []),
    "proposal",
    ...(hasExecutionTasks ? ["tasks" as const] : []),
  ];
}
