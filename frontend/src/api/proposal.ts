// Tauri invoke wrappers for task proposals with type safety using Zod schemas
// This provides an alternative API interface focused on proposals

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";
import type {
  TaskProposalResponse,
  PriorityAssessmentResponse,
  DependencyGraphResponse,
  ApplyProposalsResultResponse,
} from "./ideation";

// ============================================================================
// Response Schemas (matching Rust backend serialization with snake_case)
// ============================================================================

const TaskProposalResponseSchema = z.object({
  id: z.string(),
  session_id: z.string(),
  title: z.string(),
  description: z.string().nullable(),
  category: z.string(),
  steps: z.array(z.string()),
  acceptance_criteria: z.array(z.string()),
  suggested_priority: z.string(),
  priority_score: z.number(),
  priority_reason: z.string().nullable(),
  estimated_complexity: z.string(),
  user_priority: z.string().nullable(),
  user_modified: z.boolean(),
  status: z.string(),
  created_task_id: z.string().nullable(),
  plan_artifact_id: z.string().nullable(),
  plan_version_at_creation: z.number().nullable(),
  sort_order: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

const PriorityAssessmentResponseSchema = z.object({
  proposal_id: z.string(),
  priority: z.string(),
  score: z.number(),
  reason: z.string(),
});

const DependencyGraphNodeResponseSchema = z.object({
  proposal_id: z.string(),
  title: z.string(),
  in_degree: z.number(),
  out_degree: z.number(),
});

const DependencyGraphEdgeResponseSchema = z.object({
  from: z.string(),
  to: z.string(),
  reason: z.string().nullable(),
});

const DependencyGraphResponseSchema = z.object({
  nodes: z.array(DependencyGraphNodeResponseSchema),
  edges: z.array(DependencyGraphEdgeResponseSchema),
  critical_path: z.array(z.string()),
  has_cycles: z.boolean(),
  cycles: z.array(z.array(z.string())).nullable(),
  message: z.string().nullable().optional(),
  summary: z.object({
    total_proposals: z.number(),
    root_count: z.number(),
    leaf_count: z.number(),
    max_depth: z.number(),
  }).nullable().optional(),
});

const ApplyProposalsResultResponseSchema = z.object({
  created_task_ids: z.array(z.string()),
  dependencies_created: z.number(),
  tasks_created: z.number().optional(),
  warnings: z.array(z.string()),
  session_converted: z.boolean(),
  execution_plan_id: z.string().nullable().optional(),
  message: z.string().nullable().optional(),
});

// ============================================================================
// Input Types
// ============================================================================

/**
 * Data for creating a new task proposal
 */
export interface CreateProposalData {
  title: string;
  category: string;
  description?: string;
  steps?: string[];
  acceptanceCriteria?: string[];
  priority?: string;
  complexity?: string;
}

/**
 * Partial data for updating a task proposal
 */
export interface UpdateProposalChanges {
  title?: string;
  description?: string;
  category?: string;
  steps?: string[];
  acceptanceCriteria?: string[];
  userPriority?: string;
  complexity?: string;
}

/**
 * Options for applying proposals to Kanban
 */
export interface ApplyToKanbanOptions {
  sessionId: string;
  proposalIds: string[];
  targetColumn: "draft" | "backlog" | "todo";
}

// ============================================================================
// Transform Functions (snake_case -> camelCase)
// ============================================================================

type RawProposal = z.infer<typeof TaskProposalResponseSchema>;
type RawAssessment = z.infer<typeof PriorityAssessmentResponseSchema>;
type RawGraph = z.infer<typeof DependencyGraphResponseSchema>;
type RawApplyResult = z.infer<typeof ApplyProposalsResultResponseSchema>;

function transformProposal(raw: RawProposal): TaskProposalResponse {
  return {
    id: raw.id,
    sessionId: raw.session_id,
    title: raw.title,
    description: raw.description,
    category: raw.category,
    steps: raw.steps,
    acceptanceCriteria: raw.acceptance_criteria,
    suggestedPriority: raw.suggested_priority,
    priorityScore: raw.priority_score,
    priorityReason: raw.priority_reason,
    estimatedComplexity: raw.estimated_complexity,
    userPriority: raw.user_priority,
    userModified: raw.user_modified,
    status: raw.status,
    createdTaskId: raw.created_task_id,
    planArtifactId: raw.plan_artifact_id,
    planVersionAtCreation: raw.plan_version_at_creation,
    sortOrder: raw.sort_order,
    createdAt: raw.created_at,
    updatedAt: raw.updated_at,
  };
}

function transformPriorityAssessment(raw: RawAssessment): PriorityAssessmentResponse {
  return {
    proposalId: raw.proposal_id,
    priority: raw.priority,
    score: raw.score,
    reason: raw.reason,
  };
}

function transformDependencyGraph(raw: RawGraph): DependencyGraphResponse {
  return {
    nodes: raw.nodes.map((n) => ({
      proposalId: n.proposal_id,
      title: n.title,
      inDegree: n.in_degree,
      outDegree: n.out_degree,
    })),
    edges: raw.edges.map((e) => ({
      from: e.from,
      to: e.to,
      reason: e.reason,
    })),
    criticalPath: raw.critical_path,
    hasCycles: raw.has_cycles,
    cycles: raw.cycles,
    ...(raw.message !== undefined && { message: raw.message }),
    ...(raw.summary !== undefined && {
      summary: raw.summary
        ? {
            totalProposals: raw.summary.total_proposals,
            rootCount: raw.summary.root_count,
            leafCount: raw.summary.leaf_count,
            maxDepth: raw.summary.max_depth,
          }
        : null,
    }),
  };
}

function transformApplyResult(raw: RawApplyResult): ApplyProposalsResultResponse {
  return {
    createdTaskIds: raw.created_task_ids,
    dependenciesCreated: raw.dependencies_created,
    ...(raw.tasks_created !== undefined && { tasksCreated: raw.tasks_created }),
    warnings: raw.warnings,
    sessionConverted: raw.session_converted,
    executionPlanId: raw.execution_plan_id ?? null,
    ...(raw.message !== undefined && { message: raw.message }),
  };
}

// ============================================================================
// Typed Invoke Helper
// ============================================================================

async function typedInvoke<T>(
  cmd: string,
  args: Record<string, unknown>,
  schema: z.ZodType<T>
): Promise<T> {
  const result = await invoke(cmd, args);
  return schema.parse(result);
}

// ============================================================================
// Proposal API Functions
// ============================================================================

/**
 * Create a new task proposal in a session
 * @param sessionId The ideation session ID
 * @param data Proposal creation data
 * @returns The created proposal
 */
export async function createTaskProposal(
  sessionId: string,
  data: CreateProposalData
): Promise<TaskProposalResponse> {
  const raw = await typedInvoke(
    "create_task_proposal",
    {
      input: {
        session_id: sessionId,
        title: data.title,
        category: data.category,
        description: data.description,
        steps: data.steps,
        acceptance_criteria: data.acceptanceCriteria,
        priority: data.priority,
        complexity: data.complexity,
      },
    },
    TaskProposalResponseSchema
  );
  return transformProposal(raw);
}

/**
 * Update an existing task proposal
 * @param proposalId The proposal ID
 * @param changes Partial update data
 * @returns The updated proposal
 */
export async function updateTaskProposal(
  proposalId: string,
  changes: UpdateProposalChanges
): Promise<TaskProposalResponse> {
  const raw = await typedInvoke(
    "update_task_proposal",
    {
      id: proposalId,
      input: {
        title: changes.title,
        description: changes.description,
        category: changes.category,
        steps: changes.steps,
        acceptance_criteria: changes.acceptanceCriteria,
        user_priority: changes.userPriority,
        complexity: changes.complexity,
      },
    },
    TaskProposalResponseSchema
  );
  return transformProposal(raw);
}

/**
 * Delete a task proposal
 * @param proposalId The proposal ID
 */
export async function deleteTaskProposal(proposalId: string): Promise<void> {
  await invoke("delete_task_proposal", { id: proposalId });
}

/**
 * Reorder proposals within a session
 * @param sessionId The session ID
 * @param proposalIds Array of proposal IDs in desired order
 */
export async function reorderProposals(
  sessionId: string,
  proposalIds: string[]
): Promise<void> {
  await invoke("reorder_proposals", {
    sessionId,
    proposalIds,
  });
}

/**
 * Assess priority for a single proposal
 * @param proposalId The proposal ID
 * @returns Priority assessment result
 */
export async function assessProposalPriority(
  proposalId: string
): Promise<PriorityAssessmentResponse> {
  const raw = await typedInvoke(
    "assess_proposal_priority",
    { id: proposalId },
    PriorityAssessmentResponseSchema
  );
  return transformPriorityAssessment(raw);
}

/**
 * Assess priority for all proposals in a session
 * @param sessionId The session ID
 * @returns Array of priority assessments
 */
export async function assessAllPriorities(
  sessionId: string
): Promise<PriorityAssessmentResponse[]> {
  const raw = await typedInvoke(
    "assess_all_priorities",
    { sessionId },
    z.array(PriorityAssessmentResponseSchema)
  );
  return raw.map(transformPriorityAssessment);
}

/**
 * Add a dependency between proposals
 * @param proposalId The proposal that depends on another
 * @param dependsOnId The proposal that is depended on
 */
export async function addProposalDependency(
  proposalId: string,
  dependsOnId: string
): Promise<void> {
  await invoke("add_proposal_dependency", {
    proposalId,
    dependsOnId,
  });
}

/**
 * Remove a dependency between proposals
 * @param proposalId The proposal that depends on another
 * @param dependsOnId The proposal that is depended on
 */
export async function removeProposalDependency(
  proposalId: string,
  dependsOnId: string
): Promise<void> {
  await invoke("remove_proposal_dependency", {
    proposalId,
    dependsOnId,
  });
}

/**
 * Analyze dependencies and build graph for a session
 * @param sessionId The session ID
 * @returns Dependency graph with nodes, edges, and cycle info
 */
export async function analyzeDependencies(
  sessionId: string
): Promise<DependencyGraphResponse> {
  const raw = await typedInvoke(
    "analyze_dependencies",
    { sessionId },
    DependencyGraphResponseSchema
  );
  return transformDependencyGraph(raw);
}

/**
 * Apply proposals to the Kanban board as tasks
 * @param options Apply options including session, proposals, target column, and dependency preservation
 * @returns Result with created task IDs and warnings
 */
export async function applyProposalsToKanban(
  options: ApplyToKanbanOptions
): Promise<ApplyProposalsResultResponse> {
  const raw = await typedInvoke(
    "apply_proposals_to_kanban",
    {
      input: {
        session_id: options.sessionId,
        proposal_ids: options.proposalIds,
        target_column: options.targetColumn,
      },
    },
    ApplyProposalsResultResponseSchema
  );
  return transformApplyResult(raw);
}

// ============================================================================
// Namespace Export for Alternative Usage Pattern
// ============================================================================

/**
 * Proposal API as a namespace object (alternative to individual imports)
 */
export const proposalApi = {
  createTaskProposal,
  updateTaskProposal,
  deleteTaskProposal,
  reorderProposals,
  assessProposalPriority,
  assessAllPriorities,
  addProposalDependency,
  removeProposalDependency,
  analyzeDependencies,
  applyProposalsToKanban,
} as const;
