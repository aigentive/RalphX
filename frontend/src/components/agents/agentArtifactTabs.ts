import type { AgentArtifactTab } from "@/stores/agentSessionStore";

export type IdeationArtifactTab = Exclude<
  AgentArtifactTab,
  "publish" | "jira" | "linear" | "clickup" | "granola" | "team"
>;

export interface IdeationArtifactAvailability {
  hasAttachedIdeationSession: boolean;
  hasPlanArtifact: boolean;
  canStartPlan: boolean;
  hasVerificationEvidence: boolean;
  hasExecutionTasks: boolean;
}

export function getVisibleIdeationArtifactTabs({
  hasAttachedIdeationSession,
  hasPlanArtifact,
  canStartPlan,
  hasVerificationEvidence: _hasVerificationEvidence,
  hasExecutionTasks,
}: IdeationArtifactAvailability): IdeationArtifactTab[] {
  if (!hasPlanArtifact) {
    return canStartPlan ? ["plan"] : [];
  }

  const canShowDataDrivenTabs = hasAttachedIdeationSession;

  return [
    "plan",
    ...(canShowDataDrivenTabs && hasExecutionTasks ? ["tasks" as const] : []),
  ];
}
