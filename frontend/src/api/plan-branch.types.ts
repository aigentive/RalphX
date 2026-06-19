// TypeScript types for plan-branch API (camelCase)

export type PlanBranchStatus = "active" | "merged" | "abandoned";

export interface PlanBranch {
  id: string;
  planArtifactId: string;
  sessionId: string;
  projectId: string;
  branchName: string;
  sourceBranch: string;
  status: PlanBranchStatus;
  mergeTaskId: string | null;
  createdAt: string;
  mergedAt: string | null;
  prNumber: number | null;
  prUrl: string | null;
  prDraft: boolean | null;
  prPushStatus: "pending" | "pushed" | "failed" | null;
  prStatus: "Draft" | "Open" | "Merged" | "Closed" | null;
  prPollingActive: boolean;
  prEligible: boolean;
  mergeCommitSha?: string | null;
  baseBranchOverride: string | null;
  executionPlanId: string | null;
}

export interface EnableFeatureBranchInput {
  planArtifactId: string;
  sessionId: string;
  projectId: string;
  baseBranchOverride?: string;
}
