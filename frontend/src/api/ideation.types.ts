// Frontend types for ideation API responses (camelCase)

import type {
  IdeationSessionStatus,
  VerificationStatus,
} from "../types/ideation";

export interface IdeationSessionResponse {
  id: string;
  projectId: string;
  title: string | null;
  titleSource: "auto" | "user" | null;
  status: IdeationSessionStatus;
  planArtifactId: string | null;
  seedTaskId: string | null;
  parentSessionId: string | null;
  createdAt: string;
  updatedAt: string;
  archivedAt: string | null;
  convertedAt: string | null;
  verificationStatus: VerificationStatus;
  verificationInProgress: boolean;
  gapScore: number | null;
  sourceProjectId?: string | null;
  sourceSessionId?: string | null;
  sourceTaskId?: string | null;
  sourceContextType?: string | null;
  sourceContextId?: string | null;
  spawnReason?: string | null;
  blockerFingerprint?: string | null;
  inheritedPlanArtifactId?: string | null;
  sessionPurpose: "general" | "verification";
  sessionFlow: "ideation" | "planning";
  acceptanceStatus: "pending" | "accepted" | "rejected" | null;
  analysisBaseRefKind?: "project_default" | "current_branch" | "local_branch" | "pull_request" | null;
  analysisBaseRef?: string | null;
  analysisBaseDisplayName?: string | null;
  analysisWorkspaceKind?: "project_root" | "ideation_worktree";
  analysisWorkspacePath?: string | null;
  analysisBaseCommit?: string | null;
  analysisBaseLockedAt?: string | null;
  lastEffectiveModel?: string | null;
}

export interface LatestChildSessionIdResponse {
  sessionId: string;
  purpose: "general" | "verification" | null;
  latestChildSessionId: string | null;
}

export type IdeationAnalysisBaseRefKind =
  | "project_default"
  | "current_branch"
  | "local_branch"
  | "pull_request";

export interface IdeationAnalysisBaseSelection {
  kind: IdeationAnalysisBaseRefKind;
  ref: string;
  displayName: string;
}

export interface VerificationStatusResponse {
  sessionId: string;
  status: "unverified" | "queued" | "verifying" | "verified" | "failed" | "cancelled";
  inProgress: boolean;
  planArtifactId: string | null;
  verifiedPlanArtifactId: string | null;
  agentRunId: string | null;
  startedAt: string | null;
  completedAt: string | null;
  error: string | null;
}

export interface TaskProposalResponse {
  id: string;
  sessionId: string;
  title: string;
  description: string | null;
  category: string;
  steps: string[];
  acceptanceCriteria: string[];
  suggestedPriority: string;
  priorityScore: number;
  priorityReason: string | null;
  estimatedComplexity: string;
  userPriority: string | null;
  userModified: boolean;
  status: string;
  createdTaskId: string | null;
  planArtifactId: string | null;
  planVersionAtCreation: number | null;
  blueprintArtifactId: string | null;
  blueprintVersionAtCreation: number | null;
  sortOrder: number;
  createdAt: string;
  updatedAt: string;
}

export interface ChatMessageResponse {
  id: string;
  sessionId: string | null;
  projectId: string | null;
  taskId: string | null;
  role: string;
  content: string;
  metadata: string | null;
  parentMessageId: string | null;
  toolCalls: string | null;
  createdAt: string;
}

export interface SessionWithDataResponse {
  session: IdeationSessionResponse;
  proposals: TaskProposalResponse[];
  messages: ChatMessageResponse[];
}

export interface PriorityAssessmentResponse {
  proposalId: string;
  priority: string;
  score: number;
  reason: string;
}

export interface DependencyGraphNodeResponse {
  proposalId: string;
  title: string;
  inDegree: number;
  outDegree: number;
}

export interface DependencyGraphEdgeResponse {
  from: string;
  to: string;
  reason: string | null;
}

export interface DependencyAnalysisSummary {
  totalProposals: number;
  rootCount: number;
  leafCount: number;
  maxDepth: number;
}

export interface DependencyGraphResponse {
  nodes: DependencyGraphNodeResponse[];
  edges: DependencyGraphEdgeResponse[];
  criticalPath: string[];
  hasCycles: boolean;
  cycles: string[][] | null;
  message?: string | null;
  summary?: DependencyAnalysisSummary | null;
}

export interface ApplyProposalsResultResponse {
  createdTaskIds: string[];
  dependenciesCreated: number;
  tasksCreated?: number;
  warnings: string[];
  sessionConverted: boolean;
  executionPlanId: string | null;
  message?: string | null;
}

export interface RestartImplementationResultResponse {
  sessionId: string;
  oldExecutionPlanId: string;
  executionPlanId: string;
  archivedTaskCount: number;
  createdTaskIds: string[];
}

// Input types for API calls

export interface CreateProposalInput {
  sessionId: string;
  title: string;
  category: string;
  description?: string;
  steps?: string[];
  acceptanceCriteria?: string[];
  priority?: string;
  complexity?: string;
}

export interface UpdateProposalInput {
  title?: string;
  description?: string;
  category?: string;
  steps?: string[];
  acceptanceCriteria?: string[];
  userPriority?: string;
  complexity?: string;
}

export interface ApplyProposalsInput {
  sessionId: string;
  proposalIds: string[];
  targetColumn: string;
  baseBranchOverride?: string;
}

// Session linking response types

export interface CreateChildSessionResponse {
  sessionId: string;
  parentSessionId: string;
  title: string | null;
  status: string;
  createdAt: string;
  generation?: number;
  parentContext: ParentSessionContextResponse | undefined;
}

export interface ParentSessionContextResponse {
  parentSession: {
    id: string;
    title: string | null;
    status: string;
  };
  planContent: string | null;
  proposals: Array<{
    id: string;
    title: string;
    category: string;
    priority: string | null;
    status: string;
    acceptanceCriteria: string[];
  }>;
}

export interface CreateChildSessionInput {
  parentSessionId: string;
  title?: string;
  description?: string;
  inheritContext?: boolean;
}

export interface CrossProjectSessionInput {
  targetProjectPath: string;
  sourceSessionId: string;
  title?: string;
}
