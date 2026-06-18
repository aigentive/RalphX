import type { AgentArtifactTab } from "@/stores/agentSessionStore";

export type IdeationArtifactTab = Exclude<AgentArtifactTab, "publish" | "jira">;

export interface IdeationArtifactAvailability {
  hasAttachedIdeationSession: boolean;
  hasPlanArtifact: boolean;
  hasExecutionTasks: boolean;
}

export function getVisibleIdeationArtifactTabs({
  hasAttachedIdeationSession,
  hasPlanArtifact,
  hasExecutionTasks,
}: IdeationArtifactAvailability): IdeationArtifactTab[] {
  if (!hasAttachedIdeationSession || !hasPlanArtifact) {
    return [];
  }

  return hasExecutionTasks
    ? ["plan", "verification", "proposal", "tasks"]
    : ["plan", "verification", "proposal"];
}
