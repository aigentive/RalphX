// Ideation system types and Zod schemas
// Types for IdeationSession, TaskProposal, ChatMessage, DependencyGraph

import { z } from "zod";

// ============================================================================
// Verification
// ============================================================================


export const VERIFICATION_STATUS_VALUES = [
  "unverified",
  "reviewing",
  "verified",
  "needs_revision",
  "skipped",
  "imported_verified",
] as const;

export const VerificationStatusSchema = z.enum(VERIFICATION_STATUS_VALUES);
export type VerificationStatus = z.infer<typeof VerificationStatusSchema>;

// ============================================================================
// Ideation Session
// ============================================================================

/**
 * Status values for ideation sessions
 */
export const IDEATION_SESSION_STATUS_VALUES = [
  "active",
  "archived",
  "accepted",
] as const;

export const IdeationSessionStatusSchema = z.enum(IDEATION_SESSION_STATUS_VALUES);
export type IdeationSessionStatus = z.infer<typeof IdeationSessionStatusSchema>;

/**
 * Ideation session schema matching Rust backend serialization
 */
export const IdeationSessionSchema = z.object({
  id: z.string().min(1),
  projectId: z.string().min(1),
  title: z.string().nullable(),
  titleSource: z.enum(["auto", "user"]).nullable().optional(),
  status: IdeationSessionStatusSchema,
  planArtifactId: z.string().nullable(),
  inheritedPlanArtifactId: z.string().nullable().optional(),
  seedTaskId: z.string().nullish(),
  parentSessionId: z.string().nullable(),
  createdAt: z.string().datetime(),
  updatedAt: z.string().datetime(),
  archivedAt: z.string().datetime().nullable(),
  convertedAt: z.string().datetime().nullable(),
  verificationStatus: VerificationStatusSchema.optional().default("unverified"),
  verificationInProgress: z.boolean().optional().default(false),
  gapScore: z.number().int().nullable().optional(),
  verificationUpdateSeq: z.number().int().optional(),
  planUpdateSeq: z.number().int().optional(),
  sourceProjectId: z.string().nullable().optional(),
  sourceSessionId: z.string().nullable().optional(),
  sourceTaskId: z.string().nullable().optional(),
  sourceContextType: z.string().nullable().optional(),
  sourceContextId: z.string().nullable().optional(),
  spawnReason: z.string().nullable().optional(),
  blockerFingerprint: z.string().nullable().optional(),
  sessionPurpose: z.enum(["general", "verification"]).default("general"),
  sessionFlow: z.enum(["ideation", "planning"]).default("ideation"),
  acceptanceStatus: z.enum(["pending", "accepted", "rejected"]).nullable().optional(),
  analysisBaseRefKind: z.enum(["project_default", "current_branch", "local_branch", "pull_request"]).nullable().optional(),
  analysisBaseRef: z.string().nullable().optional(),
  analysisBaseDisplayName: z.string().nullable().optional(),
  analysisWorkspaceKind: z.enum(["project_root", "ideation_worktree"]).optional(),
  analysisWorkspacePath: z.string().nullable().optional(),
  analysisBaseCommit: z.string().nullable().optional(),
  analysisBaseLockedAt: z.string().nullable().optional(),
  lastEffectiveModel: z.string().nullable().optional(),
});

export type IdeationSession = z.infer<typeof IdeationSessionSchema>;

// ============================================================================
// Priority
// ============================================================================

/**
 * Priority values in descending order of importance
 */
export const PRIORITY_VALUES = ["critical", "high", "medium", "low"] as const;

export const PrioritySchema = z.enum(PRIORITY_VALUES);
export type Priority = z.infer<typeof PrioritySchema>;

// ============================================================================
// Complexity
// ============================================================================

/**
 * Complexity values in ascending order
 */
export const COMPLEXITY_VALUES = [
  "trivial",
  "simple",
  "moderate",
  "complex",
  "very_complex",
] as const;

export const ComplexitySchema = z.enum(COMPLEXITY_VALUES);
export type Complexity = z.infer<typeof ComplexitySchema>;

// ============================================================================
// Proposal Status
// ============================================================================

/**
 * Status values for task proposals
 */
export const PROPOSAL_STATUS_VALUES = [
  "pending",
  "accepted",
  "rejected",
  "modified",
] as const;

export const ProposalStatusSchema = z.enum(PROPOSAL_STATUS_VALUES);
export type ProposalStatus = z.infer<typeof ProposalStatusSchema>;

// ============================================================================
// Task Proposal
// ============================================================================

/**
 * Task proposal schema matching Rust backend serialization
 */
export const TaskProposalSchema = z.object({
  id: z.string().min(1),
  sessionId: z.string().min(1),
  title: z.string().min(1),
  description: z.string().nullable(),
  category: z.string().min(1),
  steps: z.array(z.string()),
  acceptanceCriteria: z.array(z.string()),
  suggestedPriority: PrioritySchema,
  priorityScore: z.number().int().min(0).max(100),
  priorityReason: z.string().nullable(),
  estimatedComplexity: ComplexitySchema,
  userPriority: PrioritySchema.nullable(),
  userModified: z.boolean(),
  status: ProposalStatusSchema,
  createdTaskId: z.string().nullable(),
  planArtifactId: z.string().nullable(),
  planVersionAtCreation: z.number().int().nullable(),
  sortOrder: z.number().int(),
  createdAt: z.string().datetime(),
  updatedAt: z.string().datetime(),
});

export type TaskProposal = z.infer<typeof TaskProposalSchema>;

// ============================================================================
// Message Role and Chat Message
// ============================================================================

/**
 * Message role values
 */
export const MESSAGE_ROLE_VALUES = ["user", "orchestrator", "system"] as const;

export const MessageRoleSchema = z.enum(MESSAGE_ROLE_VALUES);
export type MessageRole = z.infer<typeof MessageRoleSchema>;

/**
 * Chat message schema matching Rust backend serialization
 */
export const ChatMessageSchema = z.object({
  id: z.string().min(1),
  sessionId: z.string().nullable(),
  projectId: z.string().nullable(),
  taskId: z.string().nullable(),
  role: MessageRoleSchema,
  content: z.string().min(1),
  metadata: z.string().nullable(),
  parentMessageId: z.string().nullable(),
  conversationId: z.string().nullable(),
  toolCalls: z.string().nullable(), // JSON string of tool calls
  contentBlocks: z.string().nullish(), // JSON string of interleaved text/tool_use blocks (optional for backwards compat)
  createdAt: z.string().datetime(),
});

export type ChatMessage = z.infer<typeof ChatMessageSchema>;

// ============================================================================
// Dependency Graph
// ============================================================================

/**
 * Node in the dependency graph
 */
export const DependencyGraphNodeSchema = z.object({
  proposalId: z.string().min(1),
  title: z.string(),
  inDegree: z.number().int().min(0),
  outDegree: z.number().int().min(0),
});

export type DependencyGraphNode = z.infer<typeof DependencyGraphNodeSchema>;

/**
 * Edge in the dependency graph (from depends on to)
 */
export const DependencyGraphEdgeSchema = z.object({
  from: z.string().min(1),
  to: z.string().min(1),
  reason: z.string().optional(),
});

export type DependencyGraphEdge = z.infer<typeof DependencyGraphEdgeSchema>;

/**
 * Complete dependency graph structure
 */
export const DependencyGraphSchema = z.object({
  nodes: z.array(DependencyGraphNodeSchema),
  edges: z.array(DependencyGraphEdgeSchema),
  criticalPath: z.array(z.string()),
  hasCycles: z.boolean(),
  cycles: z.array(z.array(z.string())).nullable(),
});

export type DependencyGraph = z.infer<typeof DependencyGraphSchema>;

// ============================================================================
// Priority Assessment
// ============================================================================

/**
 * Result of priority assessment for a proposal
 */
export const PriorityAssessmentSchema = z.object({
  proposalId: z.string().min(1),
  priority: PrioritySchema,
  score: z.number().int().min(0).max(100),
  reason: z.string(),
});

export type PriorityAssessment = z.infer<typeof PriorityAssessmentSchema>;

// ============================================================================
// Apply Proposals
// ============================================================================

/**
 * Input for applying proposals to Kanban board
 */
export const ApplyProposalsInputSchema = z.object({
  sessionId: z.string().min(1),
  proposalIds: z.array(z.string().min(1)).min(1),
  targetColumn: z.string().min(1),
  baseBranchOverride: z.string().optional(),
});

export type ApplyProposalsInput = z.infer<typeof ApplyProposalsInputSchema>;

/**
 * Result of applying proposals
 */
export const ApplyProposalsResultSchema = z.object({
  createdTaskIds: z.array(z.string()),
  dependenciesCreated: z.number().int().min(0),
  warnings: z.array(z.string()),
  sessionConverted: z.boolean(),
});

export type ApplyProposalsResult = z.infer<typeof ApplyProposalsResultSchema>;

// ============================================================================
// Session Linking
// ============================================================================

/**
 * Enum for session relationship types
 */
export const SESSION_RELATIONSHIP_VALUES = ["follow_on", "alternative", "dependency"] as const;

export const SessionRelationshipSchema = z.enum(SESSION_RELATIONSHIP_VALUES);
export type SessionRelationship = z.infer<typeof SessionRelationshipSchema>;

/**
 * Session link schema representing relationship between parent and child sessions
 */
export const SessionLinkSchema = z.object({
  id: z.string().min(1),
  parentSessionId: z.string().min(1),
  childSessionId: z.string().min(1),
  relationship: SessionRelationshipSchema,
  notes: z.string().nullable(),
  createdAt: z.string().datetime(),
});

export type SessionLink = z.infer<typeof SessionLinkSchema>;

/**
 * Parent session context returned when querying parent context from child session
 */
export const ParentSessionContextSchema = z.object({
  parentSession: z.object({
    id: z.string().min(1),
    title: z.string().nullable(),
    status: IdeationSessionStatusSchema,
  }),
  planContent: z.string().nullable(),
  proposals: z.array(
    z.object({
      id: z.string().min(1),
      title: z.string().min(1),
      category: z.string().min(1),
      priority: PrioritySchema.nullable(),
      status: ProposalStatusSchema,
      acceptanceCriteria: z.array(z.string()),
    })
  ),
});

export type ParentSessionContext = z.infer<typeof ParentSessionContextSchema>;

// ============================================================================
// Input Schemas (for API calls)
// ============================================================================

/**
 * Input for creating a new ideation session
 */
export const CreateSessionInputSchema = z.object({
  projectId: z.string().min(1, "Project ID is required"),
  title: z.string().optional(),
  seedTaskId: z.string().optional(),
});

export type CreateSessionInput = z.infer<typeof CreateSessionInputSchema>;

/**
 * Input for creating a new task proposal
 */
export const CreateProposalInputSchema = z.object({
  sessionId: z.string().min(1, "Session ID is required"),
  title: z.string().min(1, "Title is required"),
  description: z.string().optional(),
  category: z.string().min(1, "Category is required"),
  steps: z.array(z.string()).optional(),
  acceptanceCriteria: z.array(z.string()).optional(),
  priority: z.string().optional(),
  complexity: z.string().optional(),
});

export type CreateProposalInput = z.infer<typeof CreateProposalInputSchema>;

/**
 * Input for updating a task proposal
 */
export const UpdateProposalInputSchema = z.object({
  title: z.string().min(1).optional(),
  description: z.string().optional(),
  category: z.string().min(1).optional(),
  steps: z.array(z.string()).optional(),
  acceptanceCriteria: z.array(z.string()).optional(),
  userPriority: z.string().optional(),
  complexity: z.string().optional(),
});

export type UpdateProposalInput = z.infer<typeof UpdateProposalInputSchema>;

/**
 * Input for sending a chat message
 */
export const SendChatMessageInputSchema = z.object({
  sessionId: z.string().optional(),
  projectId: z.string().optional(),
  taskId: z.string().optional(),
  role: MessageRoleSchema,
  content: z.string().min(1, "Message content is required"),
  metadata: z.string().optional(),
  parentMessageId: z.string().optional(),
});

export type SendChatMessageInput = z.infer<typeof SendChatMessageInputSchema>;

// ============================================================================
// Session with Data (composite response)
// ============================================================================

/**
 * Session with proposals and messages
 */
export const SessionWithDataSchema = z.object({
  session: IdeationSessionSchema,
  proposals: z.array(TaskProposalSchema),
  messages: z.array(ChatMessageSchema),
});

export type SessionWithData = z.infer<typeof SessionWithDataSchema>;

// ============================================================================
// List schemas
// ============================================================================

export const IdeationSessionListSchema = z.array(IdeationSessionSchema);
export type IdeationSessionList = z.infer<typeof IdeationSessionListSchema>;

export const TaskProposalListSchema = z.array(TaskProposalSchema);
export type TaskProposalList = z.infer<typeof TaskProposalListSchema>;

export const ChatMessageListSchema = z.array(ChatMessageSchema);
export type ChatMessageList = z.infer<typeof ChatMessageListSchema>;

// ============================================================================
// Paginated Session Group Types (server-side grouping and pagination)
// ============================================================================

export type SessionGroupKey = "drafts" | "in_progress" | "accepted" | "done" | "archived";

export interface SessionGroupCounts {
  drafts: number;
  inProgress: number;
  accepted: number;
  done: number;
  archived: number;
}

export interface SessionProgress {
  idle: number;
  active: number;
  done: number;
  total: number;
}

export interface IdeationSessionWithProgress extends IdeationSession {
  progress: SessionProgress | null;
  parentSessionTitle: string | null;
  verificationChildCount: number;
  hasPendingPrompt: boolean;
}

export interface SessionListResponse {
  sessions: IdeationSessionWithProgress[];
  total: number;
  hasMore: boolean;
  offset: number;
}
