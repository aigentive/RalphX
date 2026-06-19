import type { AgentArtifactTab } from "@/stores/agentSessionStore";

export type IdeationArtifactTab = Exclude<AgentArtifactTab, "publish" | "jira" | "linear">;

export interface IdeationArtifactAvailability {
  hasAttachedIdeationSession: boolean;
  hasPlanArtifact: boolean;
  hasVerificationArtifacts?: boolean;
  hasExecutionTasks: boolean;
}

export function getVisibleIdeationArtifactTabs({
  hasAttachedIdeationSession,
  hasPlanArtifact,
  hasVerificationArtifacts = false,
  hasExecutionTasks,
}: IdeationArtifactAvailability): IdeationArtifactTab[] {
  if (!hasAttachedIdeationSession || !hasPlanArtifact) {
    return [];
  }

  return [
    "plan",
    ...(hasVerificationArtifacts ? ["verification" as const] : []),
    "proposal",
    ...(hasExecutionTasks ? ["tasks" as const] : []),
  ];
}
