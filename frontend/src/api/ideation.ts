// Tauri invoke wrappers for ideation system with type safety using Zod schemas

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";
import { backendApiUrl } from "@/api/backend";
import { IdeationSettingsResponseSchema } from "../types/ideation-config";
import type { IdeationSettings } from "../types/ideation-config";
import {
  IdeationSessionResponseSchema,
  TaskProposalResponseSchema,
  SessionWithDataResponseSchema,
  PriorityAssessmentResponseSchema,
  DependencyGraphResponseSchema,
  ApplyProposalsResultResponseSchema,
  CreateChildSessionResponseSchema,
  LatestChildSessionIdResponseSchema,
  ParentSessionContextResponseSchema,
  VerificationResponseSchema,
  SessionListResponseSchema,
} from "./ideation.schemas";
import {
  transformSession,
  transformSessionList,
  transformProposal,
  transformSessionWithData,
  transformPriorityAssessment,
  transformDependencyGraph,
  transformApplyResult,
  transformIdeationSettings,
  transformCreateChildSession,
  transformParentSessionContext,
} from "./ideation.transforms";
export { toTaskProposal } from "./ideation.transforms";
import type {
  SessionGroupKey,
  SessionListResponse,
} from "../types/ideation";
import type {
  IdeationSessionResponse,
  TaskProposalResponse,
  SessionWithDataResponse,
  PriorityAssessmentResponse,
  DependencyGraphResponse,
  ApplyProposalsResultResponse,
  CreateProposalInput,
  UpdateProposalInput,
  ApplyProposalsInput,
  IdeationAnalysisBaseSelection,
  CreateChildSessionResponse,
  LatestChildSessionIdResponse,
  ParentSessionContextResponse,
  CreateChildSessionInput,
  VerificationStatusResponse,
} from "./ideation.types";

// Re-export types for convenience
export type {
  IdeationSessionResponse,
  TaskProposalResponse,
  ChatMessageResponse,
  SessionWithDataResponse,
  PriorityAssessmentResponse,
  DependencyGraphNodeResponse,
  DependencyGraphEdgeResponse,
  DependencyGraphResponse,
  ApplyProposalsResultResponse,
  CreateProposalInput,
  UpdateProposalInput,
  ApplyProposalsInput,
  IdeationAnalysisBaseSelection,
  CreateChildSessionResponse,
  LatestChildSessionIdResponse,
  ParentSessionContextResponse,
  CreateChildSessionInput,
  VerificationStatusResponse,
} from "./ideation.types";


// ============================================================================
// Helpers
// ============================================================================

function nullableBoolToInt(value: boolean | null | undefined): number | null {
  if (value === null || value === undefined) return null;
  return value ? 1 : 0;
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

function toVerificationStatusResponse(
  raw: z.infer<typeof VerificationResponseSchema>
): VerificationStatusResponse {
  return {
    sessionId: raw.session_id,
    status: raw.status as VerificationStatusResponse["status"],
    inProgress: raw.in_progress,
    ...(raw.verification_generation !== undefined && { generation: raw.verification_generation }),
    ...(raw.selected_generation !== undefined && { selectedGeneration: raw.selected_generation }),
    ...(raw.current_round !== undefined && { currentRound: raw.current_round }),
    ...(raw.max_rounds !== undefined && { maxRounds: raw.max_rounds }),
    ...(raw.gap_score !== undefined && { gapScore: raw.gap_score }),
    ...(raw.convergence_reason !== undefined && { convergenceReason: raw.convergence_reason }),
    ...(raw.best_round_index !== undefined && { bestRoundIndex: raw.best_round_index }),
    gaps: raw.current_gaps,
    rounds: raw.rounds,
    roundDetails: raw.round_details,
    runHistory: raw.run_history.map((entry) => ({
      ...entry,
      status: entry.status as VerificationStatusResponse["status"],
    })),
    ...(raw.plan_version !== undefined && { planVersion: raw.plan_version }),
    ...(raw.verification_child !== undefined && { verificationChild: raw.verification_child }),
  };
}

// ============================================================================
// API Object
// ============================================================================

/**
 * Ideation API wrappers for Tauri commands
 */
export const ideationApi = {
  /**
   * Session management operations
   */
  sessions: {
    /**
     * Create a new ideation session
     * @param projectId The project ID
     * @param title Optional session title
     * @param seedTaskId Optional draft task ID to seed the session
     * @param teamMode Optional team mode ('solo' | 'research' | 'debate')
     * @param teamConfig Optional team configuration
     * @returns The created session
     */
    create: async (
      projectId: string,
      title?: string,
      seedTaskId?: string,
      teamMode?: string,
      teamConfig?: { maxTeammates: number; modelCeiling: string; budgetLimit?: number | undefined; compositionMode: string },
      analysisBase?: IdeationAnalysisBaseSelection,
    ): Promise<IdeationSessionResponse> => {
      const raw = await typedInvoke(
        "create_ideation_session",
        {
          input: {
            project_id: projectId,
            title,
            seed_task_id: seedTaskId,
            ...(analysisBase !== undefined && {
              analysis_base_ref_kind: analysisBase.kind,
              analysis_base_ref: analysisBase.ref,
              analysis_base_display_name: analysisBase.displayName,
            }),
            ...(teamMode !== undefined && { team_mode: teamMode }),
            ...(teamConfig !== undefined && {
              team_config: {
                max_teammates: teamConfig.maxTeammates,
                model_ceiling: teamConfig.modelCeiling,
                ...(teamConfig.budgetLimit !== undefined && { budget_limit: teamConfig.budgetLimit }),
                composition_mode: teamConfig.compositionMode,
              },
            }),
          },
        },
        IdeationSessionResponseSchema
      );
      return transformSession(raw);
    },

    /**
     * Get a session by ID
     * @param sessionId The session ID
     * @returns The session or null if not found
     */
    get: async (sessionId: string): Promise<IdeationSessionResponse | null> => {
      const raw = await typedInvoke(
        "get_ideation_session",
        { id: sessionId },
        IdeationSessionResponseSchema.nullable()
      );
      return raw ? transformSession(raw) : null;
    },

    /**
     * Get a session with all proposals and messages
     * @param sessionId The session ID
     * @returns Session with data or null if not found
     */
    getWithData: async (sessionId: string): Promise<SessionWithDataResponse | null> => {
      const raw = await typedInvoke(
        "get_ideation_session_with_data",
        { id: sessionId },
        SessionWithDataResponseSchema.nullable()
      );
      return raw ? transformSessionWithData(raw) : null;
    },

    /**
     * List all sessions for a project
     * @param projectId The project ID
     * @returns Array of sessions
     */
    list: async (projectId: string): Promise<IdeationSessionResponse[]> => {
      const raw = await typedInvoke(
        "list_ideation_sessions",
        { projectId, purpose: "general" },  // camelCase - Tauri auto-converts to snake_case for Rust
        z.array(IdeationSessionResponseSchema)
      );
      return raw.map(transformSession);
    },

    listByGroup: async (
      projectId: string,
      group: SessionGroupKey,
      offset = 0,
      limit = 200,
      search?: string
    ): Promise<SessionListResponse> => {
      const raw = await typedInvoke(
        "list_sessions_by_group",
        {
          projectId,
          group,
          offset,
          limit,
          ...(search ? { search } : {}),
        },
        SessionListResponseSchema
      );
      return transformSessionList(raw);
    },

    /**
     * Archive a session
     * @param sessionId The session ID
     */
    archive: async (sessionId: string): Promise<void> => {
      await invoke("archive_ideation_session", { id: sessionId });
    },

    /**
     * Reopen an accepted/archived session back to Active status
     * Deletes all tasks, cleans up git resources, clears proposal task links
     * @param sessionId The session ID
     */
    reopen: async (sessionId: string): Promise<void> => {
      await invoke("reopen_ideation_session", { id: sessionId });
    },

    /**
     * Update a session's title
     * @param sessionId The session ID
     * @param title The new title (or null to clear)
     * @returns The updated session
     */
    updateTitle: async (sessionId: string, title: string | null): Promise<IdeationSessionResponse> => {
      const raw = await typedInvoke(
        "update_ideation_session_title",
        { id: sessionId, title },
        IdeationSessionResponseSchema
      );
      return transformSession(raw);
    },

    /**
     * Spawn a ralphx-utility-session-namer agent to auto-generate a title from the first message
     * Runs in background and returns immediately (fire-and-forget)
     * @param sessionId The session ID
     * @param firstMessage The user's first message in the session
     */
    spawnSessionNamer: async (sessionId: string, firstMessage: string): Promise<void> => {
      await invoke("spawn_session_namer", { sessionId, firstMessage });
    },

    /**
     * Create a child session linked to this parent session
     * @param input Child session creation parameters
     * @returns The created child session with optional parent context
     */
    createChild: async (input: CreateChildSessionInput): Promise<CreateChildSessionResponse> => {
      const raw = await typedInvoke(
        "create_child_session",
        {
          input: {
            parent_session_id: input.parentSessionId,
            title: input.title,
            description: input.description,
            inherit_context: input.inheritContext,
          },
        },
        CreateChildSessionResponseSchema
      );
      return transformCreateChildSession(raw);
    },

    /**
     * Get the parent session context for a child session
     * Includes parent metadata, plan content, and proposals summary
     * @param sessionId The child session ID
     * @returns Parent session context or null if session has no parent
     */
    getParentContext: async (sessionId: string): Promise<ParentSessionContextResponse | null> => {
      const raw = await typedInvoke(
        "get_parent_session_context",
        { session_id: sessionId },
        ParentSessionContextResponseSchema.nullable()
      );
      return raw ? transformParentSessionContext(raw) : null;
    },

    /**
     * Get all child sessions of this session
     * @param sessionId The parent session ID
     * @returns Array of child sessions
     */
    getChildren: async (sessionId: string, purpose?: string): Promise<IdeationSessionResponse[]> => {
      const raw = await typedInvoke(
        "get_child_sessions",
        { session_id: sessionId, purpose: purpose ?? null },
        z.array(IdeationSessionResponseSchema)
      );
      return raw.map(transformSession);
    },

    getLatestChildSessionId: async (
      sessionId: string,
      purpose?: "general" | "verification",
      options: { includeArchived?: boolean } = {}
    ): Promise<LatestChildSessionIdResponse> => {
      const raw = await typedInvoke(
        "get_latest_child_session_id",
        {
          sessionId,
          purpose: purpose ?? null,
          includeArchived: options.includeArchived ?? true,
        },
        LatestChildSessionIdResponseSchema
      );
      return {
        sessionId: raw.session_id,
        purpose: raw.purpose ?? null,
        latestChildSessionId: raw.latest_child_session_id,
      };
    },
  },

  /**
   * Task proposal operations
   */
  proposals: {
    /**
     * Create a new task proposal
     * @param input Proposal creation data
     * @returns The created proposal
     */
    create: async (input: CreateProposalInput): Promise<TaskProposalResponse> => {
      const raw = await typedInvoke(
        "create_task_proposal",
        {
          input: {
            session_id: input.sessionId,
            title: input.title,
            category: input.category,
            description: input.description,
            steps: input.steps,
            acceptance_criteria: input.acceptanceCriteria,
            priority: input.priority,
            complexity: input.complexity,
          },
        },
        TaskProposalResponseSchema
      );
      return transformProposal(raw);
    },

    /**
     * Get a proposal by ID
     * @param proposalId The proposal ID
     * @returns The proposal or null if not found
     */
    get: async (proposalId: string): Promise<TaskProposalResponse | null> => {
      const raw = await typedInvoke(
        "get_task_proposal",
        { id: proposalId },
        TaskProposalResponseSchema.nullable()
      );
      return raw ? transformProposal(raw) : null;
    },

    /**
     * List all proposals for a session
     * @param sessionId The session ID
     * @returns Array of proposals
     */
    list: async (sessionId: string): Promise<TaskProposalResponse[]> => {
      const raw = await typedInvoke(
        "list_session_proposals",
        { sessionId },  // camelCase - Tauri auto-converts to snake_case for Rust
        z.array(TaskProposalResponseSchema)
      );
      return raw.map(transformProposal);
    },

    /**
     * Update a proposal
     * @param proposalId The proposal ID
     * @param input Partial update data
     * @returns The updated proposal
     */
    update: async (proposalId: string, input: UpdateProposalInput): Promise<TaskProposalResponse> => {
      const raw = await typedInvoke(
        "update_task_proposal",
        {
          id: proposalId,
          input: {
            title: input.title,
            description: input.description,
            category: input.category,
            steps: input.steps,
            acceptance_criteria: input.acceptanceCriteria,
            user_priority: input.userPriority,
            complexity: input.complexity,
          },
        },
        TaskProposalResponseSchema
      );
      return transformProposal(raw);
    },

    /**
     * Delete a proposal
     * @param proposalId The proposal ID
     */
    delete: async (proposalId: string): Promise<void> => {
      await invoke("delete_task_proposal", { id: proposalId });
    },

    /**
     * Reorder proposals within a session
     * @param sessionId The session ID
     * @param proposalIds Array of proposal IDs in desired order
     */
    reorder: async (sessionId: string, proposalIds: string[]): Promise<void> => {
      await invoke("reorder_proposals", {
        sessionId,
        proposalIds,
      });
    },

    /**
     * Assess priority for a single proposal
     * @param proposalId The proposal ID
     * @returns Priority assessment result
     */
    assessPriority: async (proposalId: string): Promise<PriorityAssessmentResponse> => {
      const raw = await typedInvoke(
        "assess_proposal_priority",
        { id: proposalId },
        PriorityAssessmentResponseSchema
      );
      return transformPriorityAssessment(raw);
    },

    /**
     * Assess priority for all proposals in a session
     * @param sessionId The session ID
     * @returns Array of priority assessments
     */
    assessAllPriorities: async (sessionId: string): Promise<PriorityAssessmentResponse[]> => {
      const raw = await typedInvoke(
        "assess_all_priorities",
        { sessionId },  // camelCase - Tauri auto-converts to snake_case for Rust
        z.array(PriorityAssessmentResponseSchema)
      );
      return raw.map(transformPriorityAssessment);
    },
  },

  /**
   * Proposal dependency operations
   */
  dependencies: {
    /**
     * Add a dependency between proposals
     * @param proposalId The proposal that depends on another
     * @param dependsOnId The proposal that is depended on
     */
    add: async (proposalId: string, dependsOnId: string): Promise<void> => {
      await invoke("add_proposal_dependency", {
        proposalId,
        dependsOnId,
      });
    },

    /**
     * Remove a dependency between proposals
     * @param proposalId The proposal that depends on another
     * @param dependsOnId The proposal that is depended on
     */
    remove: async (proposalId: string, dependsOnId: string): Promise<void> => {
      await invoke("remove_proposal_dependency", {
        proposalId,
        dependsOnId,
      });
    },

    /**
     * Get proposals that this proposal depends on
     * @param proposalId The proposal ID
     * @returns Array of proposal IDs this depends on
     */
    getDependencies: async (proposalId: string): Promise<string[]> => {
      return typedInvoke(
        "get_proposal_dependencies",
        { proposalId },
        z.array(z.string())
      );
    },

    /**
     * Get proposals that depend on this proposal
     * @param proposalId The proposal ID
     * @returns Array of proposal IDs that depend on this
     */
    getDependents: async (proposalId: string): Promise<string[]> => {
      return typedInvoke(
        "get_proposal_dependents",
        { proposalId },
        z.array(z.string())
      );
    },

    /**
     * Analyze dependencies and build graph for a session
     * @param sessionId The session ID
     * @returns Dependency graph with nodes, edges, and cycle info
     */
    analyze: async (sessionId: string): Promise<DependencyGraphResponse> => {
      const raw = await typedInvoke(
        "analyze_dependencies",
        { sessionId },
        DependencyGraphResponseSchema
      );
      return transformDependencyGraph(raw);
    },
  },

  /**
   * Apply proposals to Kanban operations
   */
  apply: {
    /**
     * Apply proposals to the Kanban board as tasks
     * @param input Apply options
     * @returns Result with created task IDs and warnings
     */
    toKanban: async (input: ApplyProposalsInput): Promise<ApplyProposalsResultResponse> => {
      const raw = await typedInvoke(
        "apply_proposals_to_kanban",
        {
          input: {
            session_id: input.sessionId,
            proposal_ids: input.proposalIds,
            target_column: input.targetColumn,
            ...(input.baseBranchOverride !== undefined && {
              base_branch_override: input.baseBranchOverride,
            }),
          },
        },
        ApplyProposalsResultResponseSchema
      );
      return transformApplyResult(raw);
    },
  },

  /**
   * Task dependency operations (for applied tasks)
   */
  taskDependencies: {
    /**
     * Get tasks that block a given task (tasks it depends on)
     * @param taskId The task ID
     * @returns Array of blocking task IDs
     */
    getBlockers: async (taskId: string): Promise<string[]> => {
      return typedInvoke(
        "get_task_blockers",
        { taskId },
        z.array(z.string())
      );
    },

    /**
     * Get tasks that are blocked by a given task
     * @param taskId The task ID
     * @returns Array of blocked task IDs
     */
    getBlocked: async (taskId: string): Promise<string[]> => {
      return typedInvoke(
        "get_blocked_tasks",
        { taskId },
        z.array(z.string())
      );
    },
  },

  /**
   * Ideation settings operations
   */
  settings: {
    /**
     * Get current ideation settings
     * @returns Current ideation settings
     */
    get: async (): Promise<IdeationSettings> => {
      const raw = await typedInvoke(
        "get_ideation_settings",
        {},
        IdeationSettingsResponseSchema
      );
      return transformIdeationSettings(raw);
    },

    /**
     * Update ideation settings
     * @param settings New settings to apply
     * @returns Updated settings
     */
    update: async (settings: IdeationSettings): Promise<IdeationSettings> => {
      const raw = await typedInvoke(
        "update_ideation_settings",
        {
          settings: {
            plan_mode: "optional",
            require_plan_approval: false,
            suggest_plans_for_complex: false,
            auto_link_proposals: false,
            require_accept_for_finalize: settings.requireAcceptForFinalize,
            require_verification_for_proposals: settings.requireVerificationForProposals,
            require_verification_for_accept: settings.requireVerificationForAccept,
            ext_require_verification_for_accept: nullableBoolToInt(settings.externalOverrides?.requireVerificationForAccept),
            ext_require_verification_for_proposals: nullableBoolToInt(settings.externalOverrides?.requireVerificationForProposals),
            ext_require_accept_for_finalize: nullableBoolToInt(settings.externalOverrides?.requireAcceptForFinalize),
          },
        },
        IdeationSettingsResponseSchema
      );
      return transformIdeationSettings(raw);
    },
  },

  /**
   * Plan verification operations through the configured local backend
   */
  verification: {
    /**
     * Get current verification status for a session's plan
     * @param sessionId The session ID
     * @returns Verification status response
     */
    getStatus: async (
      sessionId: string,
      generation?: number
    ): Promise<VerificationStatusResponse> => {
      const search = generation !== undefined
        ? `?generation=${encodeURIComponent(String(generation))}`
        : "";
      const res = await fetch(
        backendApiUrl(`ideation/sessions/${sessionId}/verification${search}`)
      );
      if (!res.ok) {
        throw new Error(`Failed to get verification status: ${res.status}`);
      }
      return toVerificationStatusResponse(
        VerificationResponseSchema.parse(await res.json())
      );
    },

    /**
     * Skip verification for a session's plan (user-initiated)
     * @param sessionId The session ID
     * @returns Updated verification status
     */
    skip: async (sessionId: string): Promise<VerificationStatusResponse> => {
      const res = await fetch(
        backendApiUrl(`ideation/sessions/${sessionId}/verification`),
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            session_id: sessionId,
            status: "skipped",
            in_progress: false,
            convergence_reason: "user_skipped",
          }),
        }
      );
      if (!res.ok) {
        throw new Error(`Failed to skip verification: ${res.status}`);
      }
      return toVerificationStatusResponse(
        VerificationResponseSchema.parse(await res.json())
      );
    },

    /**
     * Atomically revert plan to a prior version and skip verification.
     * Single-transaction endpoint — no partial failure risk (D7).
     * @param sessionId The session ID
     * @param planVersionToRestore The plan artifact version ID to restore content from
     * @returns Updated verification status
     */
    revertAndSkip: async (
      sessionId: string,
      planVersionToRestore: string
    ): Promise<VerificationStatusResponse> => {
      const res = await fetch(
        backendApiUrl(`ideation/sessions/${sessionId}/revert-and-skip`),
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ plan_version_to_restore: planVersionToRestore }),
        }
      );
      if (!res.ok) {
        throw new Error(`Failed to revert and skip: ${res.status}`);
      }
      return toVerificationStatusResponse(
        VerificationResponseSchema.parse(await res.json())
      );
    },
  },

  /**
   * Acceptance gate operations through the configured local backend
   */
  acceptance: {
    /**
     * Accept the pending finalize confirmation for a session.
     * Atomically transitions acceptance_status Pending → Accepted, then creates tasks.
     */
    accept: async (sessionId: string): Promise<{ status: string; sessionId: string }> => {
      const res = await fetch(
        backendApiUrl(`ideation/sessions/${sessionId}/accept-finalize`),
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ session_id: sessionId }),
        }
      );
      if (!res.ok) {
        const body = await res.json().catch(() => ({})) as Record<string, unknown>;
        throw new Error((body as { error?: string }).error ?? `Accept failed: ${res.status}`);
      }
      const data = await res.json() as { status: string; session_id: string };
      return { status: data.status, sessionId: data.session_id };
    },

    /**
     * Reject the pending finalize confirmation for a session.
     * Resets acceptance_status to null, allowing the agent to re-finalize.
     */
    reject: async (sessionId: string): Promise<{ status: string; sessionId: string }> => {
      const res = await fetch(
        backendApiUrl(`ideation/sessions/${sessionId}/reject-finalize`),
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ session_id: sessionId }),
        }
      );
      if (!res.ok) {
        const body = await res.json().catch(() => ({})) as Record<string, unknown>;
        throw new Error((body as { error?: string }).error ?? `Reject failed: ${res.status}`);
      }
      const data = await res.json() as { status: string; session_id: string };
      return { status: data.status, sessionId: data.session_id };
    },
  },
} as const;
