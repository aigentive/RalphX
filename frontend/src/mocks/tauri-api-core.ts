/**
 * Mock implementation of @tauri-apps/api/core for web mode
 *
 * In web mode, invoke() calls go through the api proxy which uses mockApi.
 * This mock provides command handlers that return proper mock data.
 */

import {
  mockWorkflowsApi,
  mockProjectsApi,
  mockGetGitBranches,
  mockGetGitCurrentBranch,
  mockGetGitDefaultBranch,
} from "@/api-mock/projects";
import { mockTasksApi } from "@/api-mock/tasks";
import { mockTaskGraphApi } from "@/api-mock/task-graph";
import {
  mockCreateConversation,
  mockGetAgentConversationWorkspace,
  mockGetConversation,
  mockGetConversationStats,
  mockListAgentConversationWorkspacePublicationEvents,
  mockListConversations,
  mockListConversationsPage,
  mockPublishAgentConversationWorkspace,
  mockStartAgentConversation,
  mockSwitchAgentConversationMode,
} from "@/api-mock/chat";
import { mockReviewsApi } from "@/api-mock/reviews";
import { mockIdeationApi } from "@/api-mock/ideation";
import { mockExecutionApi } from "@/api-mock/execution";
import { mockPlanBranchApi, toSnakeCasePlanBranch } from "@/api-mock/plan-branch";
import { mockPlanApi } from "@/api-mock/plan";
import type { IdeationSessionResponse } from "@/api/ideation.types";
import type { ContextType } from "@/types/chat-conversation";
import type { ChatConversation } from "@/types/chat-conversation";
import type { ChatMessageResponse } from "@/api/chat";
import type { GitAuthDiagnostics } from "@/hooks/useGithubSettings";

const mockReviewSettings = {
  require_human_review: false,
  max_fix_attempts: 3,
  max_revision_cycles: 2,
  ai_review_enabled: true,
  ai_review_auto_fix: true,
  require_fix_approval: false,
};

const mockExternalMcpConfig = {
  enabled: true,
  port: 3848,
  host: "127.0.0.1",
  authToken: null as string | null,
  nodePath: null as string | null,
};

function toSnakeConversation(conversation: ChatConversation) {
  return {
    id: conversation.id,
    context_type: conversation.contextType,
    context_id: conversation.contextId,
    claude_session_id: conversation.claudeSessionId,
    provider_session_id: conversation.providerSessionId,
    provider_harness: conversation.providerHarness,
    upstream_provider: conversation.upstreamProvider,
    provider_profile: conversation.providerProfile,
    agent_mode: conversation.agentMode,
    title: conversation.title,
    message_count: conversation.messageCount,
    last_message_at: conversation.lastMessageAt,
    created_at: conversation.createdAt,
    updated_at: conversation.updatedAt,
    archived_at: conversation.archivedAt,
  };
}

function toSnakeMessage(message: ChatMessageResponse) {
  return {
    id: message.id,
    role: message.role,
    content: message.content,
    metadata: message.metadata,
    tool_calls: message.toolCalls,
    content_blocks: message.contentBlocks,
    sender: message.sender,
    attribution_source: message.attributionSource,
    provider_harness: message.providerHarness,
    provider_session_id: message.providerSessionId,
    upstream_provider: message.upstreamProvider,
    provider_profile: message.providerProfile,
    logical_model: message.logicalModel,
    effective_model_id: message.effectiveModelId,
    logical_effort: message.logicalEffort,
    effective_effort: message.effectiveEffort,
    input_tokens: message.inputTokens,
    output_tokens: message.outputTokens,
    cache_creation_tokens: message.cacheCreationTokens,
    cache_read_tokens: message.cacheReadTokens,
    estimated_usd: message.estimatedUsd,
    created_at: message.createdAt,
  };
}

function toSnakeIdeationSession(session: IdeationSessionResponse) {
  return {
    id: session.id,
    project_id: session.projectId,
    title: session.title,
    title_source: session.titleSource,
    status: session.status,
    plan_artifact_id: session.planArtifactId,
    seed_task_id: session.seedTaskId,
    parent_session_id: session.parentSessionId,
    team_mode: session.teamMode,
    team_config: session.teamConfig
      ? {
          max_teammates: session.teamConfig.maxTeammates,
          model_ceiling: session.teamConfig.modelCeiling,
          budget_limit: session.teamConfig.budgetLimit ?? null,
          composition_mode: session.teamConfig.compositionMode ?? null,
        }
      : null,
    created_at: session.createdAt,
    updated_at: session.updatedAt,
    archived_at: session.archivedAt,
    converted_at: session.convertedAt,
    verification_status: session.verificationStatus,
    verification_in_progress: session.verificationInProgress,
    gap_score: session.gapScore,
    source_project_id: session.sourceProjectId ?? null,
    source_session_id: session.sourceSessionId ?? null,
    source_task_id: session.sourceTaskId ?? null,
    source_context_type: session.sourceContextType ?? null,
    source_context_id: session.sourceContextId ?? null,
    spawn_reason: session.spawnReason ?? null,
    blocker_fingerprint: session.blockerFingerprint ?? null,
    inherited_plan_artifact_id: session.inheritedPlanArtifactId ?? null,
    session_purpose: session.sessionPurpose,
    acceptance_status: session.acceptanceStatus,
    analysis_base_ref_kind: session.analysisBaseRefKind ?? null,
    analysis_base_ref: session.analysisBaseRef ?? null,
    analysis_base_display_name: session.analysisBaseDisplayName ?? null,
    analysis_workspace_kind: session.analysisWorkspaceKind ?? "project_root",
    analysis_workspace_path: session.analysisWorkspacePath ?? null,
    analysis_base_commit: session.analysisBaseCommit ?? null,
    analysis_base_locked_at: session.analysisBaseLockedAt ?? null,
    last_effective_model: session.lastEffectiveModel ?? null,
  };
}

function mockGitAuthDiagnostics(): GitAuthDiagnostics {
  return window.__mockGitAuthDiagnostics ?? {
    fetchUrl: "git@github.com:mock/project.git",
    pushUrl: "git@github.com:mock/project.git",
    fetchKind: "SSH",
    pushKind: "SSH",
    mixedAuthModes: false,
    canSwitchToSsh: false,
    suggestedSshUrl: null,
  };
}

async function getMockConversationPayload(conversationId: string) {
  const controller =
    typeof window !== "undefined" ? window.__mockChatApi : undefined;
  const { conversation, messages } = controller
    ? await controller.getConversation(conversationId)
    : await mockGetConversation(conversationId);
  return {
    conversation: toSnakeConversation(conversation),
    messages: messages.map(toSnakeMessage),
  };
}

const mockWorkspaceFileChanges = [
  {
    path: "frontend/src/components/agents/AgentsView.tsx",
    status: "modified",
    additions: 48,
    deletions: 14,
  },
  {
    path: "frontend/src/components/agents/AgentComposerSurface.tsx",
    status: "modified",
    additions: 72,
    deletions: 21,
  },
  {
    path: "frontend/tests/visual/views/agents/agents.spec.ts",
    status: "added",
    additions: 260,
    deletions: 0,
  },
  {
    path: "src-tauri/src/application/agent_workspace/publisher.rs",
    status: "modified",
    additions: 31,
    deletions: 9,
  },
  {
    path: "config/harnesses/codex.yaml",
    status: "modified",
    additions: 6,
    deletions: 3,
  },
] as const;

const mockWorkspaceCommits = [
  {
    sha: "abc123def4567890abc123def4567890abc123de",
    short_sha: "abc123d",
    message: "Update agent workspace",
    author: "Agent",
    timestamp: "2026-04-26T09:00:00Z",
  },
] as const;

function mockWorkspaceFileDiff(filePath: string) {
  const language = filePath.endsWith(".tsx")
    ? "tsx"
    : filePath.endsWith(".rs")
      ? "rust"
      : filePath.endsWith(".yaml") || filePath.endsWith(".yml")
        ? "yaml"
        : "text";
  return {
    file_path: filePath,
    old_content: `// Previous mock content for ${filePath}\nexport const previous = true;\n`,
    new_content: `// Updated mock content for ${filePath}\nexport const previous = false;\nexport const reviewed = true;\n`,
    language,
  };
}

/**
 * Command handlers map - routes Tauri commands to mock implementations
 */
const commandHandlers: Record<
  string,
  (args: Record<string, unknown>) => Promise<unknown>
> = {
  // Workflow commands
  get_active_workflow_columns: async () => {
    const columns = await mockWorkflowsApi.getActiveColumns();
    // Transform to snake_case as backend would return
    return columns.map((col) => ({
      id: col.id,
      name: col.name,
      maps_to: col.mapsTo,
      color: col.color,
      icon: col.icon,
      groups: col.groups?.map((g) => ({
        id: g.id,
        label: g.label,
        statuses: g.statuses,
        icon: g.icon,
        accent_color: g.accentColor,
        can_drag_from: g.canDragFrom,
        can_drop_to: g.canDropTo,
      })),
    }));
  },
  list_workflows: async () => mockWorkflowsApi.list(),

  // Project commands
  list_projects: async () => mockProjectsApi.list(),
  get_project: async (args) => mockProjectsApi.get(args.projectId as string),
  get_git_branches: async (args) => mockGetGitBranches(args.workingDirectory as string),
  get_git_current_branch: async (args) => mockGetGitCurrentBranch(args.workingDirectory as string),
  get_git_default_branch: async (args) => mockGetGitDefaultBranch(args.workingDirectory as string),
  get_git_remote_url: async () => mockGitAuthDiagnostics().fetchUrl,
  get_git_auth_diagnostics: async () => mockGitAuthDiagnostics(),
  switch_git_origin_to_ssh: async () => {
    const current = mockGitAuthDiagnostics();
    const sshUrl = current.suggestedSshUrl ?? "git@github.com:mock/project.git";
    const updated: GitAuthDiagnostics = {
      fetchUrl: sshUrl,
      pushUrl: sshUrl,
      fetchKind: "SSH",
      pushKind: "SSH",
      mixedAuthModes: false,
      canSwitchToSsh: false,
      suggestedSshUrl: null,
    };
    window.__mockGitAuthDiagnostics = updated;
    return updated;
  },
  check_gh_auth: async () => window.__mockGhAuthStatus ?? true,
  login_gh_with_browser: async () => {
    window.__mockGhAuthStatus = true;
    return true;
  },
  setup_gh_git_auth: async () => true,
  resume_deferred_git_startup: async () => true,
  update_github_pr_enabled: async () => null,

  // Plan commands
  get_active_plan: async (args) => mockPlanApi.getActivePlan(args.projectId as string),
  set_active_plan: async (args) =>
    mockPlanApi.setActivePlan(
      args.projectId as string,
      args.ideationSessionId as string,
      args.source as Parameters<typeof mockPlanApi.setActivePlan>[2]
    ),
  clear_active_plan: async (args) => mockPlanApi.clearActivePlan(args.projectId as string),
  list_plan_selector_candidates: async (args) =>
    mockPlanApi.listCandidates(args.projectId as string, args.query as string | undefined),
  get_active_execution_plan: async (args) =>
    // In web-mode mocks, execution-plan filtering reuses the active plan id as the stable filter key.
    mockPlanApi.getActivePlan(args.projectId as string),

  // Task commands
  list_tasks: async (args) => {
    // Build params object, only including defined properties
    const params: {
      projectId: string;
      statuses?: string[];
      offset?: number;
      limit?: number;
      includeArchived?: boolean;
      ideationSessionId?: string | null;
      executionPlanId?: string | null;
    } = { projectId: args.projectId as string };

    if (args.statuses !== undefined) params.statuses = args.statuses as string[];
    if (args.offset !== undefined) params.offset = args.offset as number;
    if (args.limit !== undefined) params.limit = args.limit as number;
    if (args.includeArchived !== undefined) params.includeArchived = args.includeArchived as boolean;
    if (args.ideationSessionId !== undefined) {
      params.ideationSessionId = args.ideationSessionId as string | null;
    }
    if (args.executionPlanId !== undefined) {
      params.executionPlanId = args.executionPlanId as string | null;
    }

    const response = await mockTasksApi.list(params);
    // Transform to snake_case as backend would return
    return {
      tasks: response.tasks.map((t) => ({
        id: t.id,
        project_id: t.projectId,
        category: t.category,
        title: t.title,
        description: t.description,
        internal_status: t.internalStatus,
        priority: t.priority,
        needs_review_point: t.needsReviewPoint,
        created_at: t.createdAt,
        updated_at: t.updatedAt,
        started_at: t.startedAt,
        completed_at: t.completedAt,
        archived_at: t.archivedAt,
        blocked_reason: t.blockedReason,
        task_branch: t.taskBranch ?? null,
        metadata: t.metadata ?? null,
      })),
      total: response.total,
      offset: response.offset,
      has_more: response.hasMore,
    };
  },
  get_tasks_awaiting_review: async (args) => {
    const response = await mockTasksApi.getTasksAwaitingReview(args.project_id as string);
    // Convert to snake_case for Tauri response
    return response.map((task) => ({
      id: task.id,
      title: task.title,
      description: task.description,
      category: task.category,
      priority: task.priority,
      internal_status: task.internalStatus,
      created_at: task.createdAt,
      updated_at: task.updatedAt,
      project_id: task.projectId,
      blocked_reason: task.blockedReason,
    }));
  },

  // Chat commands
  list_agent_conversations: async (args) => {
    const controller =
      typeof window !== "undefined" ? window.__mockChatApi : undefined;
    const conversations = controller
      ? await controller.listConversations(
        args.contextType as ContextType,
        args.contextId as string
      )
      : await mockListConversations(
        args.contextType as ContextType,
        args.contextId as string
      );

    return conversations.map((conversation) => ({
      id: conversation.id,
      context_type: conversation.contextType,
      context_id: conversation.contextId,
      claude_session_id: conversation.claudeSessionId,
      provider_session_id: conversation.providerSessionId,
      provider_harness: conversation.providerHarness,
      upstream_provider: conversation.upstreamProvider,
      provider_profile: conversation.providerProfile,
      agent_mode: conversation.agentMode,
      title: conversation.title,
      message_count: conversation.messageCount,
      last_message_at: conversation.lastMessageAt,
      created_at: conversation.createdAt,
      updated_at: conversation.updatedAt,
      archived_at: conversation.archivedAt,
    }));
  },
  list_agent_conversations_page: async (args) => {
    const controller =
      typeof window !== "undefined" ? window.__mockChatApi : undefined;
    const response = controller
      ? await controller.listConversationsPage(
        args.contextType as ContextType,
        args.contextId as string,
        args.limit as number,
        (args.offset as number | undefined) ?? 0,
        (args.includeArchived as boolean | undefined) ?? false,
        args.search as string | undefined,
        (args.archivedOnly as boolean | undefined) ?? false
      )
      : await mockListConversationsPage(
        args.contextType as ContextType,
        args.contextId as string,
        args.limit as number,
        (args.offset as number | undefined) ?? 0,
        (args.includeArchived as boolean | undefined) ?? false,
        args.search as string | undefined,
        (args.archivedOnly as boolean | undefined) ?? false
      );

    return {
      conversations: response.conversations.map((conversation) => ({
        id: conversation.id,
        context_type: conversation.contextType,
        context_id: conversation.contextId,
        claude_session_id: conversation.claudeSessionId,
        provider_session_id: conversation.providerSessionId,
        provider_harness: conversation.providerHarness,
        upstream_provider: conversation.upstreamProvider,
        provider_profile: conversation.providerProfile,
        agent_mode: conversation.agentMode,
        title: conversation.title,
        message_count: conversation.messageCount,
        last_message_at: conversation.lastMessageAt,
        created_at: conversation.createdAt,
        updated_at: conversation.updatedAt,
        archived_at: conversation.archivedAt,
      })),
      limit: response.limit,
      offset: response.offset,
      total: response.total,
      has_more: response.hasMore,
    };
  },
  get_conversation: async (args) => {
    const controller =
      typeof window !== "undefined" ? window.__mockChatApi : undefined;
    return controller
      ? controller.getConversation(args.conversationId as string)
      : mockGetConversation(args.conversationId as string);
  },
  get_agent_conversation: async (args) =>
    getMockConversationPayload(args.conversationId as string),
  get_agent_conversation_messages_page: async (args) => {
    const limit = (args.limit as number | undefined) ?? 50;
    const offset = (args.offset as number | undefined) ?? 0;
    const payload = await getMockConversationPayload(args.conversationId as string);
    const messages = payload.messages.slice(offset, offset + limit);
    return {
      conversation: payload.conversation,
      messages,
      limit,
      offset,
      total_message_count: payload.messages.length,
      has_older: offset + messages.length < payload.messages.length,
    };
  },
  get_agent_conversation_workspace: async (args) => {
    const workspace = await mockGetAgentConversationWorkspace(args.conversationId as string);
    if (!workspace) {
      return null;
    }
    return {
      conversation_id: workspace.conversationId,
      project_id: workspace.projectId,
      mode: workspace.mode,
      base_ref_kind: workspace.baseRefKind,
      base_ref: workspace.baseRef,
      base_display_name: workspace.baseDisplayName,
      base_commit: workspace.baseCommit,
      branch_name: workspace.branchName,
      worktree_path: workspace.worktreePath,
      linked_ideation_session_id: workspace.linkedIdeationSessionId,
      linked_plan_branch_id: workspace.linkedPlanBranchId,
      publication_pr_number: workspace.publicationPrNumber,
      publication_pr_url: workspace.publicationPrUrl,
      publication_pr_status: workspace.publicationPrStatus,
      publication_push_status: workspace.publicationPushStatus,
      status: workspace.status,
      created_at: workspace.createdAt,
      updated_at: workspace.updatedAt,
    };
  },
  list_agent_conversation_workspace_publication_events: async (args) => {
    const events = await mockListAgentConversationWorkspacePublicationEvents(
      args.conversationId as string
    );
    return events.map((event) => ({
      id: event.id,
      conversation_id: event.conversationId,
      step: event.step,
      status: event.status,
      summary: event.summary,
      classification: event.classification,
      created_at: event.createdAt,
    }));
  },
  publish_agent_conversation_workspace: async (args) => {
    const result = await mockPublishAgentConversationWorkspace(args.conversationId as string);
    const workspace = result.workspace;
    return {
      workspace: workspace
        ? {
            conversation_id: workspace.conversationId,
            project_id: workspace.projectId,
            mode: workspace.mode,
            base_ref_kind: workspace.baseRefKind,
            base_ref: workspace.baseRef,
            base_display_name: workspace.baseDisplayName,
            base_commit: workspace.baseCommit,
            branch_name: workspace.branchName,
            worktree_path: workspace.worktreePath,
            linked_ideation_session_id: workspace.linkedIdeationSessionId,
            linked_plan_branch_id: workspace.linkedPlanBranchId,
            publication_pr_number: workspace.publicationPrNumber,
            publication_pr_url: workspace.publicationPrUrl,
            publication_pr_status: workspace.publicationPrStatus,
            publication_push_status: workspace.publicationPushStatus,
            status: workspace.status,
            created_at: workspace.createdAt,
            updated_at: workspace.updatedAt,
          }
        : null,
      commit_sha: result.commitSha,
      pushed: result.pushed,
      created_pr: result.createdPr,
      pr_number: result.prNumber,
      pr_url: result.prUrl,
    };
  },
  get_agent_conversation_workspace_file_changes: async () =>
    mockWorkspaceFileChanges.map((change) => ({ ...change })),
  get_agent_conversation_workspace_file_diff: async (args) =>
    mockWorkspaceFileDiff(args.filePath as string),
  get_agent_conversation_workspace_commits: async () => ({
    commits: mockWorkspaceCommits.map((commit) => ({ ...commit })),
  }),
  get_agent_conversation_workspace_commit_file_changes: async () =>
    mockWorkspaceFileChanges.map((change) => ({ ...change })),
  get_agent_conversation_workspace_commit_file_diff: async (args) =>
    mockWorkspaceFileDiff(args.filePath as string),
  create_agent_conversation: async (args) => {
    const input = args.input as {
      contextType: ContextType;
      contextId: string;
      title?: string;
    };
    const conversation = await mockCreateConversation(
      input.contextType,
      input.contextId,
      input.title
    );
    return {
      id: conversation.id,
      context_type: conversation.contextType,
      context_id: conversation.contextId,
      claude_session_id: conversation.claudeSessionId,
      provider_session_id: conversation.providerSessionId,
      provider_harness: conversation.providerHarness,
      upstream_provider: conversation.upstreamProvider,
      provider_profile: conversation.providerProfile,
      agent_mode: conversation.agentMode,
      title: conversation.title,
      message_count: conversation.messageCount,
      last_message_at: conversation.lastMessageAt,
      created_at: conversation.createdAt,
      updated_at: conversation.updatedAt,
      archived_at: conversation.archivedAt,
    };
  },
  start_agent_conversation: async (args) => {
    const input = args.input as Parameters<typeof mockStartAgentConversation>[0];
    const result = await mockStartAgentConversation(input);
    const conversation = result.conversation;
    const workspace = result.workspace;
    return {
      conversation: {
        id: conversation.id,
        context_type: conversation.contextType,
        context_id: conversation.contextId,
        claude_session_id: conversation.claudeSessionId,
        provider_session_id: conversation.providerSessionId,
        provider_harness: conversation.providerHarness,
        upstream_provider: conversation.upstreamProvider,
        provider_profile: conversation.providerProfile,
        agent_mode: conversation.agentMode,
        title: conversation.title,
        message_count: conversation.messageCount,
        last_message_at: conversation.lastMessageAt,
        created_at: conversation.createdAt,
        updated_at: conversation.updatedAt,
        archived_at: conversation.archivedAt,
      },
      workspace: workspace
        ? {
            conversation_id: workspace.conversationId,
            project_id: workspace.projectId,
            mode: workspace.mode,
            base_ref_kind: workspace.baseRefKind,
            base_ref: workspace.baseRef,
            base_display_name: workspace.baseDisplayName,
            base_commit: workspace.baseCommit,
            branch_name: workspace.branchName,
            worktree_path: workspace.worktreePath,
            linked_ideation_session_id: workspace.linkedIdeationSessionId,
            linked_plan_branch_id: workspace.linkedPlanBranchId,
            publication_pr_number: workspace.publicationPrNumber,
            publication_pr_url: workspace.publicationPrUrl,
            publication_pr_status: workspace.publicationPrStatus,
            publication_push_status: workspace.publicationPushStatus,
            status: workspace.status,
            created_at: workspace.createdAt,
            updated_at: workspace.updatedAt,
          }
        : null,
      send_result: {
        conversation_id: result.sendResult.conversationId,
        agent_run_id: result.sendResult.agentRunId,
        is_new_conversation: result.sendResult.isNewConversation,
        was_queued: result.sendResult.wasQueued,
        queued_as_pending: result.sendResult.queuedAsPending,
        queued_message_id: result.sendResult.queuedMessageId,
      },
    };
  },
  switch_agent_conversation_mode: async (args) => {
    const input = args.input as Parameters<typeof mockSwitchAgentConversationMode>[0];
    const result = await mockSwitchAgentConversationMode(input);
    const conversation = result.conversation;
    const workspace = result.workspace;
    return {
      conversation: {
        id: conversation.id,
        context_type: conversation.contextType,
        context_id: conversation.contextId,
        claude_session_id: conversation.claudeSessionId,
        provider_session_id: conversation.providerSessionId,
        provider_harness: conversation.providerHarness,
        upstream_provider: conversation.upstreamProvider,
        provider_profile: conversation.providerProfile,
        agent_mode: conversation.agentMode,
        title: conversation.title,
        message_count: conversation.messageCount,
        last_message_at: conversation.lastMessageAt,
        created_at: conversation.createdAt,
        updated_at: conversation.updatedAt,
        archived_at: conversation.archivedAt,
      },
      workspace: workspace
        ? {
            conversation_id: workspace.conversationId,
            project_id: workspace.projectId,
            mode: workspace.mode,
            base_ref_kind: workspace.baseRefKind,
            base_ref: workspace.baseRef,
            base_display_name: workspace.baseDisplayName,
            base_commit: workspace.baseCommit,
            branch_name: workspace.branchName,
            worktree_path: workspace.worktreePath,
            linked_ideation_session_id: workspace.linkedIdeationSessionId,
            linked_plan_branch_id: workspace.linkedPlanBranchId,
            publication_pr_number: workspace.publicationPrNumber,
            publication_pr_url: workspace.publicationPrUrl,
            publication_pr_status: workspace.publicationPrStatus,
            publication_push_status: workspace.publicationPushStatus,
            status: workspace.status,
            created_at: workspace.createdAt,
            updated_at: workspace.updatedAt,
          }
        : null,
    };
  },
  get_agent_conversation_stats: async (args) => {
    const stats = await mockGetConversationStats(args.conversationId as string);
    if (!stats) {
      return null;
    }

    const toSnakeUsage = (usage: {
      inputTokens: number;
      outputTokens: number;
      cacheCreationTokens: number;
      cacheReadTokens: number;
      estimatedUsd: number | null;
    }) => ({
      input_tokens: usage.inputTokens,
      output_tokens: usage.outputTokens,
      cache_creation_tokens: usage.cacheCreationTokens,
      cache_read_tokens: usage.cacheReadTokens,
      estimated_usd: usage.estimatedUsd,
    });

    return {
      conversation_id: stats.conversationId,
      context_type: stats.contextType,
      context_id: stats.contextId,
      provider_harness: stats.providerHarness,
      upstream_provider: stats.upstreamProvider,
      provider_profile: stats.providerProfile,
      message_usage_totals: toSnakeUsage(stats.messageUsageTotals),
      run_usage_totals: toSnakeUsage(stats.runUsageTotals),
      effective_usage_totals: toSnakeUsage(stats.effectiveUsageTotals),
      usage_coverage: {
        provider_message_count: stats.usageCoverage.providerMessageCount,
        provider_messages_with_usage: stats.usageCoverage.providerMessagesWithUsage,
        run_count: stats.usageCoverage.runCount,
        runs_with_usage: stats.usageCoverage.runsWithUsage,
        effective_totals_source: stats.usageCoverage.effectiveTotalsSource,
      },
      attribution_coverage: {
        provider_message_count: stats.attributionCoverage.providerMessageCount,
        provider_messages_with_attribution:
          stats.attributionCoverage.providerMessagesWithAttribution,
        run_count: stats.attributionCoverage.runCount,
        runs_with_attribution: stats.attributionCoverage.runsWithAttribution,
      },
      by_harness: stats.byHarness.map((bucket) => ({
        key: bucket.key,
        count: bucket.count,
        usage: toSnakeUsage(bucket.usage),
      })),
      by_upstream_provider: stats.byUpstreamProvider.map((bucket) => ({
        key: bucket.key,
        count: bucket.count,
        usage: toSnakeUsage(bucket.usage),
      })),
      by_model: stats.byModel.map((bucket) => ({
        key: bucket.key,
        count: bucket.count,
        usage: toSnakeUsage(bucket.usage),
      })),
      by_effort: stats.byEffort.map((bucket) => ({
        key: bucket.key,
        count: bucket.count,
        usage: toSnakeUsage(bucket.usage),
      })),
    };
  },
  open_agent_terminal: async (args) => {
    const input = args.input as {
      conversationId: string;
      terminalId?: string;
    };
    return mockAgentTerminalSnapshot(input.conversationId, input.terminalId);
  },
  write_agent_terminal: async () => undefined,
  resize_agent_terminal: async (args) => {
    const input = args.input as {
      conversationId: string;
      terminalId?: string;
    };
    return mockAgentTerminalSnapshot(input.conversationId, input.terminalId);
  },
  clear_agent_terminal: async (args) => {
    const input = args.input as {
      conversationId: string;
      terminalId?: string;
    };
    return {
      ...mockAgentTerminalSnapshot(input.conversationId, input.terminalId),
      history: "",
    };
  },
  restart_agent_terminal: async (args) => {
    const input = args.input as {
      conversationId: string;
      terminalId?: string;
    };
    return mockAgentTerminalSnapshot(input.conversationId, input.terminalId);
  },
  close_agent_terminal: async () => undefined,

  // Ideation commands
  list_ideation_sessions: async (args) => {
    const sessions = await mockIdeationApi.sessions.list(args.projectId as string);
    return sessions.map(toSnakeIdeationSession);
  },
  get_ideation_session: async (args) => {
    const session = await mockIdeationApi.sessions.get(args.id as string);
    if (!session) return null;
    return toSnakeIdeationSession(session);
  },
  get_ideation_session_with_data: async (args) => {
    const data = await mockIdeationApi.sessions.getWithData(args.id as string);
    if (!data) return null;
    return {
      session: toSnakeIdeationSession(data.session),
      proposals: data.proposals.map((p) => ({
        id: p.id,
        session_id: p.sessionId,
        title: p.title,
        description: p.description,
        category: p.category,
        steps: p.steps,
        acceptance_criteria: p.acceptanceCriteria,
        suggested_priority: p.suggestedPriority,
        priority_score: p.priorityScore,
        priority_reason: p.priorityReason,
        estimated_complexity: p.estimatedComplexity,
        user_priority: p.userPriority,
        user_modified: p.userModified,
        status: p.status,
        created_task_id: p.createdTaskId,
        plan_artifact_id: p.planArtifactId,
        plan_version_at_creation: p.planVersionAtCreation,
        sort_order: p.sortOrder,
        created_at: p.createdAt,
        updated_at: p.updatedAt,
      })),
      messages: data.messages,
    };
  },
  list_session_proposals: async (args) => {
    const proposals = await mockIdeationApi.proposals.list(args.session_id as string);
    // Transform to snake_case as backend would return
    return proposals.map((p) => ({
      id: p.id,
      session_id: p.sessionId,
      title: p.title,
      description: p.description,
      category: p.category,
      steps: p.steps,
      acceptance_criteria: p.acceptanceCriteria,
      suggested_priority: p.suggestedPriority,
      priority_score: p.priorityScore,
      priority_reason: p.priorityReason,
      estimated_complexity: p.estimatedComplexity,
      user_priority: p.userPriority,
      user_modified: p.userModified,
      status: p.status,
      created_task_id: p.createdTaskId,
      plan_artifact_id: p.planArtifactId,
      plan_version_at_creation: p.planVersionAtCreation,
      sort_order: p.sortOrder,
      created_at: p.createdAt,
      updated_at: p.updatedAt,
    }));
  },

  // Review commands
  list_reviews: async (args) => mockReviewsApi.getPending(args.projectId as string),

  // Task graph commands
  get_task_dependency_graph: async (args) =>
    mockTaskGraphApi.getDependencyGraph(
      args.projectId as string,
      args.includeArchived as boolean | undefined,
      (args.executionPlanId as string | null | undefined) ?? null,
      (args.sessionId as string | null | undefined)
        ?? (args.ideationSessionId as string | null | undefined)
        ?? null
    ),
  get_task_timeline_events: async (args) =>
    mockTaskGraphApi.getTimelineEvents(
      args.projectId as string,
      (args.limit as number | undefined) ?? 50,
      (args.offset as number | undefined) ?? 0
    ),

  // Execution commands (Phase 82)
  get_execution_status: async (args) => {
    const status = await mockExecutionApi.getStatus(args.projectId as string | undefined);
    // Transform to snake_case as backend would return
    return {
      is_paused: status.isPaused,
      halt_mode: status.haltMode,
      running_count: status.runningCount,
      max_concurrent: status.maxConcurrent,
      global_max_concurrent: status.globalMaxConcurrent,
      queued_count: status.queuedCount,
      can_start_task: status.canStartTask,
    };
  },
  pause_execution: async (args) => {
    const response = await mockExecutionApi.pause(args.projectId as string | undefined);
    return {
      success: response.success,
      status: {
        is_paused: response.status.isPaused,
        halt_mode: response.status.haltMode,
        running_count: response.status.runningCount,
        max_concurrent: response.status.maxConcurrent,
        global_max_concurrent: response.status.globalMaxConcurrent,
        queued_count: response.status.queuedCount,
        can_start_task: response.status.canStartTask,
      },
    };
  },
  resume_execution: async (args) => {
    const response = await mockExecutionApi.resume(args.projectId as string | undefined);
    return {
      success: response.success,
      status: {
        is_paused: response.status.isPaused,
        halt_mode: response.status.haltMode,
        running_count: response.status.runningCount,
        max_concurrent: response.status.maxConcurrent,
        global_max_concurrent: response.status.globalMaxConcurrent,
        queued_count: response.status.queuedCount,
        can_start_task: response.status.canStartTask,
      },
    };
  },
  stop_execution: async (args) => {
    const response = await mockExecutionApi.stop(args.projectId as string | undefined);
    return {
      success: response.success,
      status: {
        is_paused: response.status.isPaused,
        halt_mode: response.status.haltMode,
        running_count: response.status.runningCount,
        max_concurrent: response.status.maxConcurrent,
        global_max_concurrent: response.status.globalMaxConcurrent,
        queued_count: response.status.queuedCount,
        can_start_task: response.status.canStartTask,
      },
    };
  },
  get_execution_settings: async (args) => {
    const settings = await mockExecutionApi.getSettings(args.projectId as string | undefined);
    // Transform to snake_case as backend would return
    return {
      max_concurrent_tasks: settings.maxConcurrentTasks,
      project_ideation_max: settings.projectIdeationMax,
      auto_commit: settings.autoCommit,
      pause_on_failure: settings.pauseOnFailure,
    };
  },
  update_execution_settings: async (args) => {
    const input = args.input as {
      max_concurrent_tasks: number;
      project_ideation_max: number;
      auto_commit: boolean;
      pause_on_failure: boolean;
    };
    const settings = await mockExecutionApi.updateSettings({
      maxConcurrentTasks: input.max_concurrent_tasks,
      projectIdeationMax: input.project_ideation_max,
      autoCommit: input.auto_commit,
      pauseOnFailure: input.pause_on_failure,
    }, args.projectId as string | undefined);
    return {
      max_concurrent_tasks: settings.maxConcurrentTasks,
      project_ideation_max: settings.projectIdeationMax,
      auto_commit: settings.autoCommit,
      pause_on_failure: settings.pauseOnFailure,
    };
  },
  set_active_project: async (args) => {
    await mockExecutionApi.setActiveProject(args.projectId as string | undefined);
  },
  get_global_execution_settings: async () => {
    const settings = await mockExecutionApi.getGlobalSettings();
    // Transform to snake_case as backend would return
    return {
      global_max_concurrent: settings.globalMaxConcurrent,
      global_ideation_max: settings.globalIdeationMax,
      allow_ideation_borrow_idle_execution: settings.allowIdeationBorrowIdleExecution,
    };
  },
  update_global_execution_settings: async (args) => {
    const input = args.input as {
      global_max_concurrent: number;
      global_ideation_max: number;
      allow_ideation_borrow_idle_execution: boolean;
    };
    const settings = await mockExecutionApi.updateGlobalSettings({
      globalMaxConcurrent: input.global_max_concurrent,
      globalIdeationMax: input.global_ideation_max,
      allowIdeationBorrowIdleExecution: input.allow_ideation_borrow_idle_execution,
    });
    return {
      global_max_concurrent: settings.globalMaxConcurrent,
      global_ideation_max: settings.globalIdeationMax,
      allow_ideation_borrow_idle_execution: settings.allowIdeationBorrowIdleExecution,
    };
  },
  get_review_settings: async () => ({ ...mockReviewSettings }),
  update_review_settings: async (args) => {
    const input = args.input as {
      requireHumanReview?: boolean;
      maxFixAttempts?: number;
      maxRevisionCycles?: number;
    };
    if (input.requireHumanReview !== undefined) {
      mockReviewSettings.require_human_review = input.requireHumanReview;
    }
    if (input.maxFixAttempts !== undefined) {
      mockReviewSettings.max_fix_attempts = input.maxFixAttempts;
    }
    if (input.maxRevisionCycles !== undefined) {
      mockReviewSettings.max_revision_cycles = input.maxRevisionCycles;
    }
    return { ...mockReviewSettings };
  },
  get_external_mcp_config: async () => ({ ...mockExternalMcpConfig }),
  update_external_mcp_config: async (args) => {
    const input = args.input as {
      enabled?: boolean;
      port?: number;
      host?: string;
      authToken?: string;
      nodePath?: string;
    };
    if (input.enabled !== undefined) {
      mockExternalMcpConfig.enabled = input.enabled;
    }
    if (input.port !== undefined) {
      mockExternalMcpConfig.port = input.port;
    }
    if (input.host !== undefined) {
      mockExternalMcpConfig.host = input.host;
    }
    if (input.authToken !== undefined) {
      mockExternalMcpConfig.authToken = input.authToken === "" ? null : input.authToken;
    }
    if (input.nodePath !== undefined) {
      mockExternalMcpConfig.nodePath = input.nodePath === "" ? null : input.nodePath;
    }
  },

  // Plan branch commands
  get_plan_branch: async (args) => {
    const branch = await mockPlanBranchApi.getByPlan(args.planArtifactId as string);
    return branch ? toSnakeCasePlanBranch(branch) : null;
  },
  get_project_plan_branches: async (args) => {
    const branches = await mockPlanBranchApi.getByProject(args.projectId as string);
    return branches.map(toSnakeCasePlanBranch);
  },
  enable_feature_branch: async (args) => {
    const input = args.input as { plan_artifact_id: string; session_id: string; project_id: string };
    const branch = await mockPlanBranchApi.enable({
      planArtifactId: input.plan_artifact_id,
      sessionId: input.session_id,
      projectId: input.project_id,
    });
    return toSnakeCasePlanBranch(branch);
  },
  // Health check
  health_check: async () => ({ status: "ok" }),
};

function mockAgentTerminalSnapshot(
  conversationId: string,
  terminalId = "default"
) {
  return {
    conversationId,
    terminalId,
    cwd: "/tmp/ralphx/mock-agent-worktree",
    workspaceBranch: "ralphx/mock/agent-conversation",
    status: "running",
    pid: 42_001,
    history: "",
    exitCode: null,
    exitSignal: null,
    updatedAt: new Date().toISOString(),
  };
}

/**
 * Mock invoke function
 *
 * Routes commands to appropriate mock handlers.
 * Falls back to returning empty/null for unknown commands.
 * Respects window.__mockInvokeDelay for testing loading states.
 */
export async function invoke<T>(
  cmd: string,
  args?: Record<string, unknown>
): Promise<T> {
  // Add delay if configured (for testing loading states)
  const delay = (window as Window & { __mockInvokeDelay?: number }).__mockInvokeDelay;
  if (delay && delay > 0) {
    await new Promise((resolve) => setTimeout(resolve, delay));
  }

  const handler = commandHandlers[cmd];

  if (handler) {
    console.debug(`[mock] invoke("${cmd}") - using mock handler`);
    const result = await handler(args ?? {});
    return result as T;
  }

  // Unknown command - log warning and return sensible defaults
  console.debug(`[mock] invoke("${cmd}", ${JSON.stringify(args)}) - no handler`);
  console.warn(
    `[web-mode] No mock handler for "${cmd}". ` +
      `Add handler to tauri-api-core.ts or use api.* methods.`
  );

  // Return empty arrays for list commands, null otherwise
  if (cmd.startsWith("list_") || cmd.startsWith("get_all_")) {
    return [] as T;
  }
  return null as T;
}

/**
 * Mock transformCallback - used internally by Tauri for callbacks
 */
export function transformCallback<T>(
  callback?: (response: T) => void,
  _once?: boolean
): number {
  if (callback) {
    console.debug("[mock] transformCallback registered");
  }
  return 0;
}

/**
 * Mock Channel class - used for streaming responses
 */
export class Channel<T = unknown> {
  id: number = 0;
  private _onmessage: ((response: T) => void) | undefined;

  set onmessage(handler: (response: T) => void) {
    this._onmessage = handler;
  }

  get onmessage(): ((response: T) => void) | undefined {
    return this._onmessage;
  }

  toJSON(): string {
    return `__CHANNEL__:${this.id}`;
  }
}

/**
 * Mock Resource class - used for managed resources
 */
export class Resource {
  readonly rid: number;

  constructor(rid: number) {
    this.rid = rid;
  }

  async close(): Promise<void> {
    console.debug(`[mock] Resource.close(${this.rid})`);
  }
}

/**
 * Mock PluginListener - used for plugin event listeners
 */
export class PluginListener {
  plugin: string;
  event: string;
  channelId: number;

  constructor(plugin: string, event: string, channelId: number) {
    this.plugin = plugin;
    this.event = event;
    this.channelId = channelId;
  }

  async unregister(): Promise<void> {
    console.debug(`[mock] PluginListener.unregister(${this.plugin}:${this.event})`);
  }
}

/**
 * Mock addPluginListener - register plugin event listeners
 */
export async function addPluginListener<T>(
  plugin: string,
  event: string,
  _handler: (payload: T) => void
): Promise<PluginListener> {
  console.debug(`[mock] addPluginListener(${plugin}, ${event})`);
  return new PluginListener(plugin, event, 0);
}

/**
 * Mock isTauri - always returns false in web mode
 */
export function isTauri(): boolean {
  return false;
}

/**
 * Mock convertFileSrc - returns the path as-is (can't convert without Tauri)
 */
export function convertFileSrc(filePath: string, _protocol?: string): string {
  console.debug(`[mock] convertFileSrc(${filePath}) - returning path as-is`);
  return filePath;
}
