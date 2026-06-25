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
  mockGetConversationTimelinePage,
  mockGetConversationStats,
  mockListAgentSidebarConversations,
  mockListAgentConversationWorkspacePublicationEvents,
  mockListConversations,
  mockListConversationsPage,
  mockPublishAgentConversationWorkspace,
  mockReconcileAgentConversationWorkspacePublication,
  mockStartAgentConversation,
  mockSwitchAgentConversationMode,
} from "@/api-mock/chat";
import { mockReviewsApi } from "@/api-mock/reviews";
import { mockIdeationApi } from "@/api-mock/ideation";
import { mockExecutionApi } from "@/api-mock/execution";
import {
  mockPlanBranchApi,
  toSnakeCasePlanBranch,
} from "@/api-mock/plan-branch";
import { mockPlanApi } from "@/api-mock/plan";
import type { IdeationSessionResponse } from "@/api/ideation.types";
import type { ContextType } from "@/types/chat-conversation";
import type { ChatConversation } from "@/types/chat-conversation";
import type {
  AgentConversationWorkspace,
  AgentSidebarConversationsInput,
  ChatMessageResponse,
  ChatTimelineItemResponse,
} from "@/api/chat";
import type { GitAuthDiagnostics } from "@/hooks/useGithubSettings";

const mockReviewSettings = {
  require_human_review: false,
  max_fix_attempts: 3,
  max_revision_cycles: 2,
  ai_review_enabled: true,
  ai_review_auto_fix: true,
  require_fix_approval: false,
  auto_create_followup_agent_conversation: true,
};

const mockExternalMcpConfig = {
  enabled: true,
  port: 3848,
  host: "127.0.0.1",
  authToken: null as string | null,
  nodePath: null as string | null,
};

const mockAtlassianIntegrationSettings = {
  enabled: false,
  authMethod: "api_token",
  siteUrl: null as string | null,
  email: null as string | null,
  hasApiToken: false,
  oauthClientId: null as string | null,
  oauthRedirectUri: null as string | null,
  hasOauthClientSecret: false,
  hasOauthToken: false,
  oauthCloudId: null as string | null,
  oauthScopes: null as string | null,
  validationStatus: "not_configured",
  jiraAvailable: false,
  confluenceAvailable: false,
  lastValidatedAt: null as string | null,
  lastError: null as string | null,
  updatedAt: new Date(0).toISOString(),
};

const mockAgentConversationJiraIssues = new Map<string, unknown>();
const mockAgentConversationLinearIssues = new Map<string, unknown>();

function mockJiraIssue(input: {
  conversationId: string;
  projectId?: string | null;
  issueKey: string;
  issueId?: string | null;
  title?: string | null;
  issueUrl?: string | null;
}) {
  const now = new Date(0).toISOString();
  return {
    conversationId: input.conversationId,
    projectId: input.projectId ?? "mock-project",
    provider: "atlassian",
    issueKey: input.issueKey,
    issueId: input.issueId ?? input.issueKey,
    issueUrl: input.issueUrl ?? `https://example.atlassian.net/browse/${input.issueKey}`,
    title: input.title ?? `Mock issue ${input.issueKey}`,
    status: "To Do",
    assignee: null,
    reporter: "Mock Reporter",
    updatedAtRemote: now,
    descriptionMarkdown: "Mock Jira description.",
    descriptionText: "Mock Jira description.",
    acceptanceCriteriaMarkdown: null,
    acceptanceCriteriaText: null,
    comments: [],
    attachments: [],
    lastRefreshedAt: now,
    refreshStatus: "loaded",
    refreshError: null,
    assignedAt: now,
    assignedFromMessageId: null,
    manuallyAssigned: true,
    createdAt: now,
    updatedAt: now,
  };
}

const mockLinearWebhookConfig = {
  enabled: false,
  hasSigningSecret: false,
};

const mockLinearIntegrationSettings = {
  enabled: false,
  hasApiToken: false,
  validationStatus: "not_configured",
  issueSearchAvailable: false,
  lastValidatedAt: null as string | null,
  lastError: null as string | null,
  updatedAt: new Date(0).toISOString(),
};

const mockClickUpIntegrationSettings = {
  enabled: false,
  hasApiToken: false,
  workspaceId: null as string | null,
  validationStatus: "not_configured",
  taskSearchAvailable: false,
  lastValidatedAt: null as string | null,
  lastError: null as string | null,
  updatedAt: new Date(0).toISOString(),
};

const mockClickUpWorkspaces = [
  { id: "team-1", name: "Acme Workspace", color: "#ff6b35" },
  { id: "team-2", name: "Globex Workspace", color: null as string | null },
];

function mockLinearIssue(input: {
  conversationId: string;
  projectId?: string | null;
  issueId: string;
  issueKey?: string | null;
  title?: string | null;
  issueUrl?: string | null;
}) {
  const now = new Date(0).toISOString();
  return {
    conversationId: input.conversationId,
    projectId: input.projectId ?? "mock-project",
    provider: "linear",
    issueId: input.issueId,
    issueKey: input.issueKey ?? null,
    issueUrl:
      input.issueUrl ??
      (input.issueKey
        ? `https://linear.app/mock/issue/${input.issueKey}/mock`
        : null),
    title: input.title ?? `Mock issue ${input.issueKey ?? input.issueId}`,
    status: "Todo",
    assignee: null,
    reporter: "Mock Creator",
    updatedAtRemote: now,
    descriptionMarkdown: "Mock Linear description.",
    descriptionText: "Mock Linear description.",
    comments: [],
    attachments: [],
    lastRefreshedAt: now,
    refreshStatus: "loaded",
    refreshError: null,
    assignedAt: now,
    assignedFromMessageId: null,
    manuallyAssigned: true,
    createdAt: now,
    updatedAt: now,
  };
}

const mockAgentProviderSettings = {
  providers: [
    {
      provider: "codex",
      enabled: true,
      isDefault: true,
      model: "gpt-5.5",
      effort: "medium",
      approvalPolicy: "never",
      sandboxMode: "danger-full-access",
      claudePermissionMode: null,
      claudeDangerouslySkipPermissions: false,
      claudeAllowDangerouslySkipPermissions: false,
      cliManagementMode: "user_managed",
      autoUpdateEnabled: false,
      customBinaryEnabled: false,
      customBinaryPath: null,
      available: true,
      binaryFound: true,
      binaryPath: "/opt/homebrew/bin/codex",
      status: "ready",
      error: null,
      missingCoreExecFeatures: [],
      supportedEfforts: null,
      updatedAt: "2026-05-08T00:00:00Z",
    },
    {
      provider: "claude",
      enabled: false,
      isDefault: false,
      model: "claude-sonnet-4-6",
      effort: null,
      approvalPolicy: "never",
      sandboxMode: null,
      claudePermissionMode: "bypassPermissions",
      claudeDangerouslySkipPermissions: true,
      claudeAllowDangerouslySkipPermissions: true,
      cliManagementMode: "user_managed",
      autoUpdateEnabled: false,
      customBinaryEnabled: false,
      customBinaryPath: null,
      available: true,
      binaryFound: true,
      binaryPath: "/opt/homebrew/bin/claude",
      status: "ready",
      error: null,
      missingCoreExecFeatures: [],
      supportedEfforts: ["low", "medium", "high", "xhigh", "max"],
      updatedAt: "2026-05-08T00:00:00Z",
    },
  ],
  defaultProvider: "codex",
  requiresOnboarding: false,
};

const mockManagedProviderCliStatuses = {
  providers: [
    {
      provider: "codex",
      cliManagementMode: "user_managed",
      autoUpdateEnabled: false,
      customBinaryEnabled: false,
      customBinaryPath: null,
      supported: true,
      installed: true,
      binaryPath: "/opt/homebrew/bin/codex",
      currentVersion: "0.136.0",
      latestVersion: "0.137.0",
      updateAvailable: true,
      action: "none",
      status:
        "codex CLI 0.136.0 is user-managed; 0.137.0 is available. RX will not update it unless management is enabled.",
      error: null,
    },
    {
      provider: "claude",
      cliManagementMode: "user_managed",
      autoUpdateEnabled: false,
      customBinaryEnabled: false,
      customBinaryPath: null,
      supported: true,
      installed: true,
      binaryPath: "/Users/example/.local/bin/claude",
      currentVersion: "2.1.170",
      latestVersion: "2.1.175",
      updateAvailable: true,
      action: "none",
      status:
        "claude CLI 2.1.170 is user-managed; 2.1.175 is available. RX will not update it unless management is enabled.",
      error: null,
    },
  ],
};

const mockAgentModels = [
  {
    provider: "codex",
    modelId: "gpt-5.5",
    label: "GPT-5.5",
    menuLabel: "GPT-5.5",
    description: "Frontier Codex model for complex agent work.",
    supportedEfforts: ["low", "medium", "high", "xhigh"],
    defaultEffort: "medium",
    source: "built_in",
    enabled: true,
    createdAt: null,
    updatedAt: null,
  },
  {
    provider: "codex",
    modelId: "gpt-5.4-mini",
    label: "GPT-5.4 Mini",
    menuLabel: "GPT-5.4 Mini",
    description: "Fast Codex model for lighter agent work.",
    supportedEfforts: ["low", "medium", "high"],
    defaultEffort: "medium",
    source: "built_in",
    enabled: true,
    createdAt: null,
    updatedAt: null,
  },
  {
    provider: "claude",
    modelId: "claude-sonnet-4-6",
    label: "Claude Sonnet 4.6",
    menuLabel: "Claude Sonnet",
    description: "Balanced Claude model for agent work.",
    supportedEfforts: ["medium"],
    defaultEffort: "medium",
    source: "built_in",
    enabled: true,
    createdAt: null,
    updatedAt: null,
  },
];

const mockAgentLanes = [
  "ideation_primary",
  "ideation_verifier",
  "ideation_subagent",
  "ideation_verifier_subagent",
  "execution_worker",
  "execution_reviewer",
  "execution_reexecutor",
  "execution_merger",
] as const;

function mockAgentLaneSettings(projectId: string | null) {
  return mockAgentLanes.map((lane) => ({
    projectId,
    lane,
    harness: "codex",
    model: null,
    effort: null,
    approvalPolicy: "never",
    sandboxMode: "danger-full-access",
    updatedAt: "2026-05-08T00:00:00Z",
  }));
}

function mockAgentHarnessAvailability(projectId: string | null) {
  return mockAgentLanes.map((lane) => ({
    projectId,
    lane,
    configuredHarness: "codex",
    effectiveHarness: "codex",
    binaryPath: "/opt/homebrew/bin/codex",
    binaryFound: true,
    probeSucceeded: true,
    available: true,
    missingCoreExecFeatures: [],
    error: null,
  }));
}

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

function toSnakeAgentWorkspace(workspace: AgentConversationWorkspace | null) {
  if (!workspace) return null;
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
    mode_switch_locked: workspace.modeSwitchLocked ?? false,
    mode_switch_lock_reason: workspace.modeSwitchLockReason ?? null,
    publication_pr_number: workspace.publicationPrNumber,
    publication_pr_url: workspace.publicationPrUrl,
    publication_pr_status: workspace.publicationPrStatus,
    publication_push_status: workspace.publicationPushStatus,
    auto_publish_enabled: workspace.autoPublishEnabled ?? true,
    auto_publish_initial_pr_enabled: workspace.autoPublishInitialPrEnabled ?? false,
    auto_publish_paused_pr_autofix_enabled:
      workspace.autoPublishPausedPrAutofixEnabled ?? null,
    auto_publish_paused_pr_auto_merge_desired:
      workspace.autoPublishPausedPrAutoMergeDesired ?? null,
    status: workspace.status,
    created_at: workspace.createdAt,
    updated_at: workspace.updatedAt,
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

function toSnakeTimelineItem(item: ChatTimelineItemResponse) {
  return {
    id: item.id,
    conversation_id: item.conversationId,
    message_id: item.messageId,
    run_id: item.runId,
    sequence: item.sequence,
    block_index: item.blockIndex,
    role: item.role,
    kind: item.kind,
    status: item.status,
    content: item.content,
    content_blocks: item.contentBlocks,
    tool_call: item.toolCall,
    metadata: item.metadata,
    provider_harness: item.providerHarness,
    provider_session_id: item.providerSessionId,
    upstream_provider: item.upstreamProvider,
    provider_profile: item.providerProfile,
    logical_model: item.logicalModel,
    effective_model_id: item.effectiveModelId,
    logical_effort: item.logicalEffort,
    effective_effort: item.effectiveEffort,
    input_tokens: item.inputTokens,
    output_tokens: item.outputTokens,
    cache_creation_tokens: item.cacheCreationTokens,
    cache_read_tokens: item.cacheReadTokens,
    estimated_usd: item.estimatedUsd,
    created_at: item.createdAt,
    updated_at: item.updatedAt,
    finalized_at: item.finalizedAt,
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
    session_flow: session.sessionFlow ?? "ideation",
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
  return (
    window.__mockGitAuthDiagnostics ?? {
      fetchUrl: "git@github.com:mock/project.git",
      pushUrl: "git@github.com:mock/project.git",
      fetchKind: "SSH",
      pushKind: "SSH",
      mixedAuthModes: false,
      githubHttpsCredentialHelperConfigured: false,
      canSwitchToSsh: false,
      suggestedSshUrl: null,
    }
  );
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

const mockTicketingCapabilities = {
  supportsBoards: true,
  supportsKanban: true,
  kanbanWrite: false,
  statusWrite: false,
  assignmentWrite: false,
  commentWrite: false,
  labelWrite: false,
  freshness: "manual",
};

const mockTicketingColumns = [
  { id: "todo", name: "To Do", category: "todo", order: 0, color: null },
  { id: "in_progress", name: "In Progress", category: "in_progress", order: 1, color: null },
  { id: "review", name: "In Review", category: "in_progress", order: 2, color: null },
  { id: "done", name: "Done", category: "done", order: 3, color: null },
];

const mockTicketingTickets = [
  {
    ref: { provider: "jira", id: "10001", key: "RX-1" },
    title: "Fix merge race in transition handler",
    state: { id: "todo", name: "To Do", category: "todo", color: null },
    assignee: { id: "user-1", name: "A. Demian", email: null, avatarUrl: null },
    reporter: { id: "user-2", name: "Platform", email: null, avatarUrl: null },
    labels: ["backend", "race-condition"],
    priority: "High",
    updatedAt: "2026-06-19T22:00:00.000Z",
    url: "https://example.atlassian.net/browse/RX-1",
    associationCount: 2,
  },
  {
    ref: { provider: "jira", id: "10002", key: "RX-2" },
    title: "Add Linear webhook backfill",
    state: { id: "in_progress", name: "In Progress", category: "in_progress", color: null },
    assignee: null,
    reporter: { id: "user-2", name: "Platform", email: null, avatarUrl: null },
    labels: ["integrations"],
    priority: "Medium",
    updatedAt: "2026-06-18T18:30:00.000Z",
    url: "https://example.atlassian.net/browse/RX-2",
    associationCount: 0,
  },
  {
    ref: { provider: "jira", id: "10003", key: "RX-3" },
    title: "Ticketing dashboard shell",
    state: { id: "review", name: "In Review", category: "in_progress", color: null },
    assignee: { id: "user-1", name: "A. Demian", email: null, avatarUrl: null },
    reporter: { id: "user-2", name: "Platform", email: null, avatarUrl: null },
    labels: ["frontend"],
    priority: "Medium",
    updatedAt: "2026-06-19T19:20:00.000Z",
    url: "https://example.atlassian.net/browse/RX-3",
    associationCount: 1,
  },
  {
    ref: { provider: "clickup", id: "cu-1001", key: "CU-1001" },
    title: "Demo ClickUp dashboard task",
    state: { id: "in_progress", name: "In Progress", category: "in_progress", color: null },
    assignee: { id: "cu-user-1", name: "A. Demian", email: null, avatarUrl: null },
    reporter: { id: "cu-user-2", name: "Platform", email: null, avatarUrl: null },
    labels: ["integrations", "frontend"],
    priority: "High",
    updatedAt: "2026-06-20T15:00:00.000Z",
    url: "https://app.clickup.com/t/cu-1001",
    associationCount: 0,
  },
  {
    ref: { provider: "clickup", id: "cu-1002", key: "CU-1002" },
    title: "Validate ClickUp personal API token",
    state: { id: "todo", name: "To Do", category: "todo", color: null },
    assignee: null,
    reporter: { id: "cu-user-2", name: "Platform", email: null, avatarUrl: null },
    labels: ["backend"],
    priority: "Medium",
    updatedAt: "2026-06-20T12:30:00.000Z",
    url: "https://app.clickup.com/t/cu-1002",
    associationCount: 0,
  },
  {
    ref: { provider: "clickup", id: "cu-1003", key: "CU-1003" },
    title: "List ClickUp Spaces as dashboard containers",
    state: { id: "done", name: "Done", category: "done", color: null },
    assignee: { id: "cu-user-1", name: "A. Demian", email: null, avatarUrl: null },
    reporter: { id: "cu-user-2", name: "Platform", email: null, avatarUrl: null },
    labels: ["frontend"],
    priority: "Low",
    updatedAt: "2026-06-19T09:00:00.000Z",
    url: "https://app.clickup.com/t/cu-1003",
    associationCount: 0,
  },
];

const mockTicketingAssociations = {
  tasks: [
    {
      id: "task-1",
      title: "Fix merge race",
      subtitle: "branch ready · PR open",
      status: "executing",
      active: true,
      deepLink: { view: "kanban", id: "task-1" },
    },
  ],
  proposals: [],
  sessions: [
    {
      id: "session-1",
      title: "Transition hardening",
      subtitle: "1 linked conversation",
      status: "active",
      active: false,
      deepLink: { view: "ideation", id: "session-1" },
    },
  ],
  conversations: [],
  pullRequests: [],
  checks: [],
  qa: [],
  specs: [],
  fetchedAt: "2026-06-19T22:00:00.000Z",
};

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
  search_agent_composer_entries: async (args) => {
    const input = args.input as { query?: string; limit?: number } | undefined;
    const query = input?.query?.toLowerCase() ?? "";
    const entries = [
      { path: "src/main.tsx", kind: "file", parentPath: "src" },
      { path: "src/components", kind: "directory", parentPath: "src" },
      {
        path: "src/components/agents/AgentComposerSurface.tsx",
        kind: "file",
        parentPath: "src/components/agents",
      },
      {
        path: "src-tauri/src/lib.rs",
        kind: "file",
        parentPath: "src-tauri/src",
      },
    ].filter((entry) => entry.path.toLowerCase().includes(query));
    return {
      entries: entries.slice(0, input?.limit ?? 80),
      truncated: false,
    };
  },
  search_agent_composer_plan_references: async (args) => {
    const input = args.input as { query?: string; limit?: number } | undefined;
    const query = input?.query?.toLowerCase() ?? "";
    const plans = [
      {
        sessionId: "mock-planning-session",
        artifactId: "mock-plan-artifact",
        title: "Mock Implementation Plan",
        status: "approved",
        artifactVersion: 1,
        updatedAt: new Date().toISOString(),
        approvedAt: new Date().toISOString(),
      },
    ].filter((plan) =>
      `${plan.title} ${plan.sessionId} ${plan.artifactId} ${plan.status}`
        .toLowerCase()
        .includes(query),
    );
    return {
      plans: plans.slice(0, input?.limit ?? 12),
      truncated: false,
    };
  },
  list_agent_composer_skills: async () => ({
    skills: [
      {
        id: "internal:workspace-swe",
        name: "workspace-swe",
        displayName: null,
        description: "Apply RalphX workspace engineering guidance.",
        source: "ralphx-internal",
        providerHarness: null,
        scope: "RalphX",
        invocationKind: "internal-directive",
        invocationValue: "workspace-swe",
        enabled: true,
        sourcePath: "plugins/app/skills/workspace-swe/SKILL.md",
      },
      {
        id: "claude:project:review",
        name: "review",
        displayName: null,
        description: "Claude project review skill.",
        source: "harness-native",
        providerHarness: "claude",
        scope: "project",
        invocationKind: "harness-native-token",
        invocationValue: "/review",
        enabled: true,
        sourcePath: ".claude/skills/review/SKILL.md",
      },
      {
        id: "codex:plugin:github:yeet",
        name: "github:yeet",
        displayName: null,
        description: "Publish local changes to GitHub.",
        source: "harness-native",
        providerHarness: "codex",
        scope: "plugin",
        invocationKind: "harness-native-token",
        invocationValue: "$github:yeet",
        enabled: true,
        sourcePath: ".codex/plugins/cache/github/skills/yeet/SKILL.md",
      },
    ],
  }),
  get_agent_provider_settings: async () => mockAgentProviderSettings,
  get_managed_provider_cli_status: async () => mockManagedProviderCliStatuses,
  install_or_update_managed_provider_cli: async (args) => {
    const input = args.input as { provider?: string };
    const status = mockManagedProviderCliStatuses.providers.find(
      (entry) => entry.provider === input.provider,
    );
    if (!status || !status.supported) {
      throw new Error(
        "Managed CLI installs are not available for this provider.",
      );
    }
    Object.assign(status, {
      cliManagementMode: "rx_managed",
      installed: true,
      customBinaryEnabled: false,
      currentVersion: status.latestVersion ?? "0.137.0",
      updateAvailable: false,
      action: "none",
      status: `RX-managed ${status.provider} ${status.latestVersion ?? "0.137.0"} is installed.`,
    });
    return {
      provider: status.provider,
      success: true,
      status,
      stdout: "mock install complete",
      stderr: null,
    };
  },
  auto_update_managed_provider_clis: async () => ({
    updated: [],
    skipped: mockManagedProviderCliStatuses.providers,
  }),
  get_ui_feature_flags: async () => ({
    activityPage: true,
    extensibilityPage: true,
    battleMode: true,
    teamMode: false,
    atlassianOauth: false,
    ticketingDashboard: false,
  }),
  get_atlassian_integration_settings: async () =>
    mockAtlassianIntegrationSettings,
  save_atlassian_integration_settings: async (args) => {
    const input = args.input as {
      authMethod?: "api_token" | "oauth";
      siteUrl?: string | null;
      email?: string | null;
      apiToken?: string | null;
      oauthClientId?: string | null;
      oauthClientSecret?: string | null;
      oauthRedirectUri?: string | null;
    };
    mockAtlassianIntegrationSettings.authMethod =
      input.authMethod ?? mockAtlassianIntegrationSettings.authMethod;
    mockAtlassianIntegrationSettings.siteUrl = input.siteUrl ?? null;
    mockAtlassianIntegrationSettings.email = input.email ?? null;
    mockAtlassianIntegrationSettings.hasApiToken =
      Boolean(input.apiToken) || mockAtlassianIntegrationSettings.hasApiToken;
    mockAtlassianIntegrationSettings.oauthClientId =
      input.oauthClientId ?? null;
    mockAtlassianIntegrationSettings.oauthRedirectUri =
      input.oauthRedirectUri ?? null;
    mockAtlassianIntegrationSettings.hasOauthClientSecret =
      Boolean(input.oauthClientSecret) ||
      mockAtlassianIntegrationSettings.hasOauthClientSecret;
    mockAtlassianIntegrationSettings.enabled = false;
    mockAtlassianIntegrationSettings.validationStatus =
      mockAtlassianIntegrationSettings.authMethod === "oauth"
        ? mockAtlassianIntegrationSettings.siteUrl &&
          mockAtlassianIntegrationSettings.oauthClientId &&
          mockAtlassianIntegrationSettings.oauthRedirectUri &&
          mockAtlassianIntegrationSettings.hasOauthClientSecret
          ? "pending"
          : "not_configured"
        : mockAtlassianIntegrationSettings.siteUrl &&
            mockAtlassianIntegrationSettings.email &&
            mockAtlassianIntegrationSettings.hasApiToken
          ? "pending"
          : "not_configured";
    return mockAtlassianIntegrationSettings;
  },
  build_atlassian_oauth_authorization_url: async () => ({
    authorizationUrl: "https://auth.atlassian.com/authorize?mock=1",
    state: "mock-state",
    scopes: "read:jira-work offline_access",
    redirectUri: "http://127.0.0.1:8765/atlassian/oauth/callback",
  }),
  start_atlassian_oauth_local_callback: async () => ({
    authorizationUrl: "https://auth.atlassian.com/authorize?mock=1",
    state: "mock-state",
    scopes: "read:jira-work offline_access",
    redirectUri: "http://127.0.0.1:8765/atlassian/oauth/callback",
  }),
  complete_atlassian_oauth_local_callback: async () => {
    Object.assign(mockAtlassianIntegrationSettings, {
      authMethod: "oauth",
      enabled: true,
      hasOauthToken: true,
      oauthCloudId: "mock-cloud-id",
      validationStatus: "valid",
      jiraAvailable: true,
      confluenceAvailable: true,
      lastValidatedAt: new Date(0).toISOString(),
      lastError: null,
    });
    return mockAtlassianIntegrationSettings;
  },
  exchange_atlassian_oauth_code: async () => {
    Object.assign(mockAtlassianIntegrationSettings, {
      authMethod: "oauth",
      enabled: true,
      hasOauthToken: true,
      oauthCloudId: "mock-cloud-id",
      validationStatus: "valid",
      jiraAvailable: true,
      confluenceAvailable: true,
      lastValidatedAt: new Date(0).toISOString(),
      lastError: null,
    });
    return mockAtlassianIntegrationSettings;
  },
  validate_atlassian_integration: async () => {
    Object.assign(mockAtlassianIntegrationSettings, {
      enabled: true,
      validationStatus: "valid",
      jiraAvailable: true,
      confluenceAvailable: true,
      lastValidatedAt: new Date(0).toISOString(),
      lastError: null,
    });
    return mockAtlassianIntegrationSettings;
  },
  disconnect_atlassian_integration: async () => {
    Object.assign(mockAtlassianIntegrationSettings, {
      enabled: false,
      authMethod: "api_token",
      siteUrl: null,
      email: null,
      hasApiToken: false,
      oauthClientId: null,
      oauthRedirectUri: null,
      hasOauthClientSecret: false,
      hasOauthToken: false,
      oauthCloudId: null,
      oauthScopes: null,
      validationStatus: "not_configured",
      jiraAvailable: false,
      confluenceAvailable: false,
      lastValidatedAt: null,
      lastError: null,
      updatedAt: new Date(0).toISOString(),
    });
    return mockAtlassianIntegrationSettings;
  },
  search_atlassian_resources: async (args) => {
    const input = args.input as { kind?: string; query?: string };
    const query = input.query?.trim() ?? "";
    if (input.kind !== "jira" || query.length === 0) {
      return { resources: [] };
    }
    const key = /^[a-z]+-\d+$/i.test(query) ? query.toUpperCase() : "RX-42";
    return {
      resources: [
        {
          kind: "jira",
          id: key,
          key,
          title: `Mock issue for ${query}`,
          url: `https://example.atlassian.net/browse/${key}`,
          excerpt: "Mock Jira search result",
        },
      ],
    };
  },
  get_linear_integration_settings: async () => mockLinearIntegrationSettings,
  save_linear_integration_settings: async (args) => {
    const input = args.input as { apiToken?: string | null };
    mockLinearIntegrationSettings.hasApiToken =
      Boolean(input.apiToken?.trim()) ||
      mockLinearIntegrationSettings.hasApiToken;
    mockLinearIntegrationSettings.enabled = false;
    mockLinearIntegrationSettings.validationStatus =
      mockLinearIntegrationSettings.hasApiToken ? "pending" : "not_configured";
    mockLinearIntegrationSettings.issueSearchAvailable = false;
    mockLinearIntegrationSettings.lastError = null;
    mockLinearIntegrationSettings.updatedAt = new Date(0).toISOString();
    return mockLinearIntegrationSettings;
  },
  validate_linear_integration: async () => {
    Object.assign(mockLinearIntegrationSettings, {
      enabled: true,
      validationStatus: "valid",
      issueSearchAvailable: true,
      lastValidatedAt: new Date(0).toISOString(),
      lastError: null,
      updatedAt: new Date(0).toISOString(),
    });
    return mockLinearIntegrationSettings;
  },
  disconnect_linear_integration: async () => {
    Object.assign(mockLinearIntegrationSettings, {
      enabled: false,
      hasApiToken: false,
      validationStatus: "not_configured",
      issueSearchAvailable: false,
      lastValidatedAt: null,
      lastError: null,
      updatedAt: new Date(0).toISOString(),
    });
    return mockLinearIntegrationSettings;
  },
  search_linear_issues: async () => ({ issues: [] }),
  get_clickup_integration_settings: async () => mockClickUpIntegrationSettings,
  save_clickup_integration_settings: async (args) => {
    const input = args.input as {
      apiToken?: string | null;
      workspaceId?: string | null;
    };
    // Tri-state token: only re-gate the connection when the token changes.
    if (input.apiToken !== undefined) {
      mockClickUpIntegrationSettings.hasApiToken = Boolean(
        input.apiToken?.trim(),
      );
      mockClickUpIntegrationSettings.enabled = false;
      mockClickUpIntegrationSettings.validationStatus =
        mockClickUpIntegrationSettings.hasApiToken
          ? "pending"
          : "not_configured";
      mockClickUpIntegrationSettings.taskSearchAvailable = false;
    }
    // Tri-state workspace: undefined leaves it untouched, "" clears it.
    if (input.workspaceId !== undefined) {
      mockClickUpIntegrationSettings.workspaceId = input.workspaceId?.trim()
        ? input.workspaceId
        : null;
    }
    mockClickUpIntegrationSettings.lastError = null;
    mockClickUpIntegrationSettings.updatedAt = new Date(0).toISOString();
    return mockClickUpIntegrationSettings;
  },
  validate_clickup_integration: async () => {
    Object.assign(mockClickUpIntegrationSettings, {
      enabled: true,
      validationStatus: "valid",
      taskSearchAvailable: true,
      lastValidatedAt: new Date(0).toISOString(),
      lastError: null,
      updatedAt: new Date(0).toISOString(),
    });
    return mockClickUpIntegrationSettings;
  },
  disconnect_clickup_integration: async () => {
    Object.assign(mockClickUpIntegrationSettings, {
      enabled: false,
      hasApiToken: false,
      workspaceId: null,
      validationStatus: "not_configured",
      taskSearchAvailable: false,
      lastValidatedAt: null,
      lastError: null,
      updatedAt: new Date(0).toISOString(),
    });
    return mockClickUpIntegrationSettings;
  },
  list_clickup_workspaces: async () => ({ workspaces: mockClickUpWorkspaces }),
  search_clickup_tasks: async () => ({ tasks: [] }),
  get_agent_conversation_linear_issue: async (args) => {
    const input = args.input as { conversationId: string };
    return {
      issue: mockAgentConversationLinearIssues.get(input.conversationId) ?? null,
    };
  },
  assign_agent_conversation_linear_issue: async (args) => {
    const input = args.input as {
      conversationId: string;
      projectId?: string | null;
      issueId: string;
      issueKey?: string | null;
      title?: string | null;
      issueUrl?: string | null;
    };
    const issue = mockLinearIssue(input);
    mockAgentConversationLinearIssues.set(input.conversationId, issue);
    return { issue };
  },
  refresh_agent_conversation_linear_issue: async (args) => {
    const input = args.input as { conversationId: string };
    const existing = mockAgentConversationLinearIssues.get(input.conversationId);
    if (!existing || typeof existing !== "object") {
      return { issue: null };
    }
    const issue = {
      ...existing,
      lastRefreshedAt: new Date(0).toISOString(),
      refreshStatus: "loaded",
      refreshError: null,
    };
    mockAgentConversationLinearIssues.set(input.conversationId, issue);
    return { issue };
  },
  clear_agent_conversation_linear_issue: async (args) => {
    const input = args.input as { conversationId: string };
    mockAgentConversationLinearIssues.delete(input.conversationId);
    return { issue: null };
  },
  get_linear_webhook_config: async () => mockLinearWebhookConfig,
  list_ticketing_providers: async () => [
    {
      provider: "jira",
      label: "Jira",
      enabled: true,
      connectionStatus: "connected",
      capabilities: mockTicketingCapabilities,
      fetchedAt: "2026-06-19T22:00:00.000Z",
      staleAt: null,
      permissionMessage: null,
      errorMessage: null,
    },
    {
      provider: "linear",
      label: "Linear",
      enabled: true,
      connectionStatus: "connected",
      capabilities: { ...mockTicketingCapabilities, freshness: "webhook" },
      fetchedAt: "2026-06-19T22:00:00.000Z",
      staleAt: null,
      permissionMessage: null,
      errorMessage: null,
    },
    {
      provider: "clickup",
      label: "ClickUp",
      enabled: true,
      connectionStatus: "connected",
      capabilities: mockTicketingCapabilities,
      fetchedAt: "2026-06-19T22:00:00.000Z",
      staleAt: null,
      permissionMessage: null,
      errorMessage: null,
    },
  ],
  list_ticketing_containers: async (args) => {
    const provider = (args.provider as string | undefined) ?? "jira";
    const ticketCount = mockTicketingTickets.filter(
      (ticket) => ticket.ref.provider === provider,
    ).length;
    if (provider === "clickup") {
      // ClickUp containers are Spaces within the selected Workspace (Team).
      return [
        {
          provider,
          id: "space-eng",
          key: null,
          name: "Engineering",
          kind: "project",
          parentId: null,
          ticketCount,
        },
      ];
    }
    return [
      {
        // Jira/Linear containers are projects; the container id is the project key.
        provider,
        id: "RX",
        key: "RX",
        name: "RalphX",
        kind: "project",
        parentId: null,
        ticketCount,
      },
    ];
  },
  list_ticketing_columns: async () => mockTicketingColumns,
  list_tickets: async (args) => {
    const query = args.query as { provider?: string; filters?: { text?: string } } | undefined;
    const provider = query?.provider ?? "jira";
    const text = query?.filters?.text?.toLowerCase().trim() ?? "";
    const items = mockTicketingTickets
      .filter((ticket) => ticket.ref.provider === provider)
      .filter((ticket) => {
        if (!text) return true;
        return `${ticket.ref.key ?? ""} ${ticket.title} ${ticket.labels.join(" ")}`
          .toLowerCase()
          .includes(text);
      });
    return {
      items,
      nextCursor: null,
      total: items.length,
      fetchedAt: "2026-06-19T22:00:00.000Z",
    };
  },
  get_ticket_detail: async (args) => {
    const ticketRef = args.ticketRef as { id?: string } | undefined;
    const ticket =
      mockTicketingTickets.find((item) => item.ref.id === ticketRef?.id) ??
      mockTicketingTickets[0];
    return {
      ...ticket,
      descriptionMarkdown:
        "When two agents transition the same task, the workflow should stay consistent and preserve review history.",
      descriptionText:
        "When two agents transition the same task, the workflow should stay consistent and preserve review history.",
      acceptanceCriteriaMarkdown: "- No double-transition under contention\n- Activity timeline remains ordered",
      comments: [
        {
          id: "comment-1",
          author: { id: "user-2", name: "Platform", email: null, avatarUrl: null },
          bodyMarkdown: "Reproduced on the transition hardening branch.",
          bodyText: "Reproduced on the transition hardening branch.",
          createdAt: "2026-06-19T20:00:00.000Z",
          updatedAt: "2026-06-19T20:00:00.000Z",
        },
      ],
      attachments: [],
      transitions: [],
      fetchedAt: "2026-06-19T22:00:00.000Z",
    };
  },
  list_ticket_transitions: async () => [],
  list_ticket_labels: async (args) => {
    const provider = args.provider as string | undefined;
    if (provider === "linear") {
      return [
        { id: "label-bug", name: "Bug" },
        { id: "label-feature", name: "Feature" },
      ];
    }
    return [];
  },
  set_ticket_labels: async (args) => {
    const input = args.input as {
      provider?: string;
      ticketRef?: { provider?: string; id?: string; key?: string | null };
      labels?: string[];
      clientOperationId?: string;
    } | undefined;
    const labels = input?.labels ?? [];
    return {
      ticketRef: input?.ticketRef ?? { provider: input?.provider ?? "jira", id: "10001" },
      operation: {
        id: "op-labels-1",
        operation: "set_labels",
        clientOperationId: input?.clientOperationId ?? "mock-op",
        status: "succeeded",
        providerOperationId: null,
        errorMessage: null,
        linked: true,
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
      idempotent: false,
      labels: { labels },
      refreshedAt: new Date().toISOString(),
    };
  },
  get_ticket_associations: async () => mockTicketingAssociations,
  get_conversation_ticket: async () => null,
  refresh_tickets: async () => ({ refreshedAt: "2026-06-19T22:00:00.000Z" }),
  save_linear_webhook_signing_secret: async (args) => {
    const input = args.input as { signingSecret?: string; enabled?: boolean };
    if (!input.signingSecret?.trim()) {
      throw new Error("Linear webhook signing secret cannot be empty");
    }
    mockLinearWebhookConfig.enabled = input.enabled ?? true;
    mockLinearWebhookConfig.hasSigningSecret = true;
    return mockLinearWebhookConfig;
  },
  get_agent_conversation_jira_issue: async (args) => {
    const input = args.input as { conversationId: string };
    return {
      issue: mockAgentConversationJiraIssues.get(input.conversationId) ?? null,
    };
  },
  assign_agent_conversation_jira_issue: async (args) => {
    const input = args.input as {
      conversationId: string;
      projectId?: string | null;
      issueKey: string;
      issueId?: string | null;
      title?: string | null;
      issueUrl?: string | null;
    };
    const issue = mockJiraIssue(input);
    mockAgentConversationJiraIssues.set(input.conversationId, issue);
    return { issue };
  },
  refresh_agent_conversation_jira_issue: async (args) => {
    const input = args.input as { conversationId: string };
    const existing = mockAgentConversationJiraIssues.get(input.conversationId);
    if (!existing || typeof existing !== "object") {
      return { issue: null };
    }
    const issue = {
      ...existing,
      lastRefreshedAt: new Date(0).toISOString(),
      refreshStatus: "loaded",
      refreshError: null,
    };
    mockAgentConversationJiraIssues.set(input.conversationId, issue);
    return { issue };
  },
  clear_agent_conversation_jira_issue: async (args) => {
    const input = args.input as { conversationId: string };
    mockAgentConversationJiraIssues.delete(input.conversationId);
    return { issue: null };
  },
  update_agent_provider_settings: async (args) => {
    const input = args.input as Partial<
      (typeof mockAgentProviderSettings.providers)[number]
    > & { provider?: string; isDefault?: boolean };
    const provider = mockAgentProviderSettings.providers.find(
      (entry) => entry.provider === input.provider,
    );
    if (provider) {
      Object.assign(provider, input, { updatedAt: new Date(0).toISOString() });
      if (provider.customBinaryEnabled) {
        provider.cliManagementMode = "user_managed";
        provider.autoUpdateEnabled = false;
      } else if (provider.cliManagementMode === "rx_managed") {
        provider.customBinaryEnabled = false;
      }
      if (input.isDefault) {
        for (const entry of mockAgentProviderSettings.providers) {
          entry.isDefault = entry.provider === provider.provider;
        }
        mockAgentProviderSettings.defaultProvider = provider.provider;
        mockAgentProviderSettings.requiresOnboarding = false;
      }
    }
    return mockAgentProviderSettings;
  },
  list_agent_models: async () => mockAgentModels,
  get_agent_lane_settings: async (args) =>
    mockAgentLaneSettings(
      (args.projectId as string | null | undefined) ?? null,
    ),
  get_agent_harness_availability: async (args) => {
    const input = args.input as { projectId?: string | null } | undefined;
    return mockAgentHarnessAvailability(
      input?.projectId ?? (args.projectId as string | null | undefined) ?? null,
    );
  },
  update_agent_lane_settings: async (args) => {
    const input = args.input as {
      projectId?: string | null;
      lane: (typeof mockAgentLanes)[number];
      harness: string;
      model?: string | null;
      effort?: string | null;
      approvalPolicy?: string | null;
      sandboxMode?: string | null;
    };
    return {
      projectId: input.projectId ?? null,
      lane: input.lane,
      harness: input.harness,
      model: input.model ?? null,
      effort: input.effort ?? null,
      approvalPolicy: input.approvalPolicy ?? null,
      sandboxMode: input.sandboxMode ?? null,
      updatedAt: "2026-05-08T00:00:00Z",
    };
  },
  get_project: async (args) => mockProjectsApi.get(args.projectId as string),
  get_git_branches: async (args) =>
    mockGetGitBranches(args.workingDirectory as string),
  get_git_current_branch: async (args) =>
    mockGetGitCurrentBranch(args.workingDirectory as string),
  get_git_default_branch: async (args) =>
    mockGetGitDefaultBranch(args.workingDirectory as string),
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
      githubHttpsCredentialHelperConfigured: false,
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
  setup_gh_git_auth: async () => {
    const current = mockGitAuthDiagnostics();
    if (
      current.fetchUrl?.startsWith("https://github.com/") ||
      current.pushUrl?.startsWith("https://github.com/")
    ) {
      window.__mockGitAuthDiagnostics = {
        ...current,
        githubHttpsCredentialHelperConfigured: true,
      };
    }
    return true;
  },
  resume_deferred_git_startup: async () => true,
  update_github_pr_enabled: async () => null,

  // Plan commands
  get_active_plan: async (args) =>
    mockPlanApi.getActivePlan(args.projectId as string),
  set_active_plan: async (args) =>
    mockPlanApi.setActivePlan(
      args.projectId as string,
      args.ideationSessionId as string,
      args.source as Parameters<typeof mockPlanApi.setActivePlan>[2],
    ),
  clear_active_plan: async (args) =>
    mockPlanApi.clearActivePlan(args.projectId as string),
  list_plan_selector_candidates: async (args) =>
    mockPlanApi.listCandidates(
      args.projectId as string,
      args.query as string | undefined,
    ),
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

    if (args.statuses !== undefined)
      params.statuses = args.statuses as string[];
    if (args.offset !== undefined) params.offset = args.offset as number;
    if (args.limit !== undefined) params.limit = args.limit as number;
    if (args.includeArchived !== undefined)
      params.includeArchived = args.includeArchived as boolean;
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
    const response = await mockTasksApi.getTasksAwaitingReview(
      args.project_id as string,
    );
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
          args.contextId as string,
        )
      : await mockListConversations(
          args.contextType as ContextType,
          args.contextId as string,
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
          (args.archivedOnly as boolean | undefined) ?? false,
        )
      : await mockListConversationsPage(
          args.contextType as ContextType,
          args.contextId as string,
          args.limit as number,
          (args.offset as number | undefined) ?? 0,
          (args.includeArchived as boolean | undefined) ?? false,
          args.search as string | undefined,
          (args.archivedOnly as boolean | undefined) ?? false,
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
  list_agent_sidebar_conversations: async (args) => {
    const controller =
      typeof window !== "undefined" ? window.__mockChatApi : undefined;
    const input = args.input as AgentSidebarConversationsInput;
    const response = controller
      ? await controller.listAgentSidebarConversations(input)
      : await mockListAgentSidebarConversations(input);

    return {
      groups: response.groups.map((group) => ({
        key: group.key,
        label: group.label,
        total: group.total,
        offset: group.offset,
        limit: group.limit,
        has_more: group.hasMore,
        rows: group.rows.map((row) => ({
          conversation: toSnakeConversation(row.conversation),
          workspace: toSnakeAgentWorkspace(row.workspace),
          ref_kind: row.refKind === "pull-request" ? "pull_request" : "branch",
          ref_label: row.refLabel,
          publication_state: row.publicationState,
          publication_label: row.publicationLabel,
        })),
      })),
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
  get_agent_conversation_summary: async (args) => {
    const payload = await getMockConversationPayload(
      args.conversationId as string,
    );
    return payload.conversation;
  },
  get_agent_conversation_messages_page: async (args) => {
    const limit = (args.limit as number | undefined) ?? 50;
    const offset = (args.offset as number | undefined) ?? 0;
    const payload = await getMockConversationPayload(
      args.conversationId as string,
    );
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
  get_agent_conversation_timeline_page: async (args) => {
    const controller =
      typeof window !== "undefined" ? window.__mockChatApi : undefined;
    const limit = (args.limit as number | undefined) ?? 40;
    const beforeSequence =
      typeof args.beforeSequence === "number"
        ? args.beforeSequence
        : typeof args.before_sequence === "number"
          ? args.before_sequence
          : null;
    const payload = controller
      ? await controller.getConversationTimelinePage(
          args.conversationId as string,
          limit,
          beforeSequence,
        )
      : await mockGetConversationTimelinePage(
          args.conversationId as string,
          limit,
          beforeSequence,
        );
    return {
      conversation: toSnakeConversation(payload.conversation),
      items: payload.items.map(toSnakeTimelineItem),
      limit: payload.limit,
      before_sequence: payload.beforeSequence,
      total_item_count: payload.totalItemCount,
      has_older: payload.hasOlder,
      oldest_loaded_sequence: payload.oldestLoadedSequence,
      newest_loaded_sequence: payload.newestLoadedSequence,
    };
  },
  get_agent_conversation_workspace: async (args) => {
    const workspace = await mockGetAgentConversationWorkspace(
      args.conversationId as string,
    );
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
      mode_switch_locked: workspace.modeSwitchLocked ?? false,
      mode_switch_lock_reason: workspace.modeSwitchLockReason ?? null,
      publication_pr_number: workspace.publicationPrNumber,
      publication_pr_url: workspace.publicationPrUrl,
      publication_pr_status: workspace.publicationPrStatus,
      publication_push_status: workspace.publicationPushStatus,
      auto_publish_enabled: workspace.autoPublishEnabled ?? true,
      auto_publish_initial_pr_enabled: workspace.autoPublishInitialPrEnabled ?? false,
      auto_publish_paused_pr_autofix_enabled:
        workspace.autoPublishPausedPrAutofixEnabled ?? null,
      auto_publish_paused_pr_auto_merge_desired:
        workspace.autoPublishPausedPrAutoMergeDesired ?? null,
      status: workspace.status,
      created_at: workspace.createdAt,
      updated_at: workspace.updatedAt,
    };
  },
  list_agent_conversation_workspace_publication_events: async (args) => {
    const events = await mockListAgentConversationWorkspacePublicationEvents(
      args.conversationId as string,
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
  reconcile_agent_conversation_workspace_publication: async (args) => {
    await mockReconcileAgentConversationWorkspacePublication(
      args.conversationId as string,
    );
    return undefined;
  },
  publish_agent_conversation_workspace: async (args) => {
    const result = await mockPublishAgentConversationWorkspace(
      args.conversationId as string,
    );
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
            mode_switch_locked: workspace.modeSwitchLocked ?? false,
            mode_switch_lock_reason: workspace.modeSwitchLockReason ?? null,
            publication_pr_number: workspace.publicationPrNumber,
            publication_pr_url: workspace.publicationPrUrl,
            publication_pr_status: workspace.publicationPrStatus,
            publication_push_status: workspace.publicationPushStatus,
            auto_publish_enabled: workspace.autoPublishEnabled ?? true,
            auto_publish_initial_pr_enabled: workspace.autoPublishInitialPrEnabled ?? false,
            auto_publish_paused_pr_autofix_enabled:
              workspace.autoPublishPausedPrAutofixEnabled ?? null,
            auto_publish_paused_pr_auto_merge_desired:
              workspace.autoPublishPausedPrAutoMergeDesired ?? null,
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
  get_agent_conversation_workspace_review: async () => ({
    changes: mockWorkspaceFileChanges.map((change) => ({ ...change })),
    commits: mockWorkspaceCommits.map((commit) => ({ ...commit })),
    base_ref: "main",
    head_ref: "HEAD",
  }),
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
      input.title,
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
    const input = args.input as Parameters<
      typeof mockStartAgentConversation
    >[0];
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
            mode_switch_locked: workspace.modeSwitchLocked ?? false,
            mode_switch_lock_reason: workspace.modeSwitchLockReason ?? null,
            publication_pr_number: workspace.publicationPrNumber,
            publication_pr_url: workspace.publicationPrUrl,
            publication_pr_status: workspace.publicationPrStatus,
            publication_push_status: workspace.publicationPushStatus,
            auto_publish_enabled: workspace.autoPublishEnabled ?? true,
            auto_publish_initial_pr_enabled: workspace.autoPublishInitialPrEnabled ?? false,
            auto_publish_paused_pr_autofix_enabled:
              workspace.autoPublishPausedPrAutofixEnabled ?? null,
            auto_publish_paused_pr_auto_merge_desired:
              workspace.autoPublishPausedPrAutoMergeDesired ?? null,
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
    const input = args.input as Parameters<
      typeof mockSwitchAgentConversationMode
    >[0];
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
            mode_switch_locked: workspace.modeSwitchLocked ?? false,
            mode_switch_lock_reason: workspace.modeSwitchLockReason ?? null,
            publication_pr_number: workspace.publicationPrNumber,
            publication_pr_url: workspace.publicationPrUrl,
            publication_pr_status: workspace.publicationPrStatus,
            publication_push_status: workspace.publicationPushStatus,
            auto_publish_enabled: workspace.autoPublishEnabled ?? true,
            auto_publish_initial_pr_enabled: workspace.autoPublishInitialPrEnabled ?? false,
            auto_publish_paused_pr_autofix_enabled:
              workspace.autoPublishPausedPrAutofixEnabled ?? null,
            auto_publish_paused_pr_auto_merge_desired:
              workspace.autoPublishPausedPrAutoMergeDesired ?? null,
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
        provider_messages_with_usage:
          stats.usageCoverage.providerMessagesWithUsage,
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
    const sessions = await mockIdeationApi.sessions.list(
      args.projectId as string,
    );
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
    const proposals = await mockIdeationApi.proposals.list(
      args.session_id as string,
    );
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
  list_reviews: async (args) =>
    mockReviewsApi.getPending(args.projectId as string),

  // Task graph commands
  get_task_dependency_graph: async (args) =>
    mockTaskGraphApi.getDependencyGraph(
      args.projectId as string,
      args.includeArchived as boolean | undefined,
      (args.executionPlanId as string | null | undefined) ?? null,
      (args.sessionId as string | null | undefined) ??
        (args.ideationSessionId as string | null | undefined) ??
        null,
    ),
  get_task_timeline_events: async (args) =>
    mockTaskGraphApi.getTimelineEvents(
      args.projectId as string,
      (args.limit as number | undefined) ?? 50,
      (args.offset as number | undefined) ?? 0,
    ),

  // Execution commands (Phase 82)
  get_execution_status: async (args) => {
    const status = await mockExecutionApi.getStatus(
      args.projectId as string | undefined,
    );
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
    const response = await mockExecutionApi.pause(
      args.projectId as string | undefined,
    );
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
    const response = await mockExecutionApi.resume(
      args.projectId as string | undefined,
    );
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
    const response = await mockExecutionApi.stop(
      args.projectId as string | undefined,
    );
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
    const settings = await mockExecutionApi.getSettings(
      args.projectId as string | undefined,
    );
    // Transform to snake_case as backend would return
    return {
      max_concurrent_tasks: settings.maxConcurrentTasks,
      project_ideation_max: settings.projectIdeationMax,
      auto_commit: settings.autoCommit,
      pause_on_failure: settings.pauseOnFailure,
      agent_workspace_pr_autofix_default:
        settings.agentWorkspacePrAutofixDefault,
      agent_workspace_pr_auto_merge_default:
        settings.agentWorkspacePrAutoMergeDefault,
    };
  },
  update_execution_settings: async (args) => {
    const input = args.input as {
      max_concurrent_tasks: number;
      project_ideation_max: number;
      auto_commit: boolean;
      pause_on_failure: boolean;
      agent_workspace_pr_autofix_default: boolean;
      agent_workspace_pr_auto_merge_default: boolean;
    };
    const settings = await mockExecutionApi.updateSettings(
      {
        maxConcurrentTasks: input.max_concurrent_tasks,
        projectIdeationMax: input.project_ideation_max,
        autoCommit: input.auto_commit,
        pauseOnFailure: input.pause_on_failure,
        agentWorkspacePrAutofixDefault:
          input.agent_workspace_pr_autofix_default,
        agentWorkspacePrAutoMergeDefault:
          input.agent_workspace_pr_auto_merge_default,
      },
      args.projectId as string | undefined,
    );
    return {
      max_concurrent_tasks: settings.maxConcurrentTasks,
      project_ideation_max: settings.projectIdeationMax,
      auto_commit: settings.autoCommit,
      pause_on_failure: settings.pauseOnFailure,
      agent_workspace_pr_autofix_default:
        settings.agentWorkspacePrAutofixDefault,
      agent_workspace_pr_auto_merge_default:
        settings.agentWorkspacePrAutoMergeDefault,
    };
  },
  set_active_project: async (args) => {
    await mockExecutionApi.setActiveProject(
      args.projectId as string | undefined,
    );
  },
  get_global_execution_settings: async () => {
    const settings = await mockExecutionApi.getGlobalSettings();
    // Transform to snake_case as backend would return
    return {
      global_max_concurrent: settings.globalMaxConcurrent,
      workspace_max_concurrent: settings.workspaceMaxConcurrent,
      global_ideation_max: settings.globalIdeationMax,
      allow_ideation_borrow_idle_execution:
        settings.allowIdeationBorrowIdleExecution,
    };
  },
  update_global_execution_settings: async (args) => {
    const input = args.input as {
      global_max_concurrent: number;
      workspace_max_concurrent: number;
      global_ideation_max: number;
      allow_ideation_borrow_idle_execution: boolean;
    };
    const settings = await mockExecutionApi.updateGlobalSettings({
      globalMaxConcurrent: input.global_max_concurrent,
      workspaceMaxConcurrent: input.workspace_max_concurrent,
      globalIdeationMax: input.global_ideation_max,
      allowIdeationBorrowIdleExecution:
        input.allow_ideation_borrow_idle_execution,
    });
    return {
      global_max_concurrent: settings.globalMaxConcurrent,
      workspace_max_concurrent: settings.workspaceMaxConcurrent,
      global_ideation_max: settings.globalIdeationMax,
      allow_ideation_borrow_idle_execution:
        settings.allowIdeationBorrowIdleExecution,
    };
  },
  get_review_settings: async () => ({ ...mockReviewSettings }),
  update_review_settings: async (args) => {
    const input = args.input as {
      requireHumanReview?: boolean;
      maxFixAttempts?: number;
      maxRevisionCycles?: number;
      autoCreateFollowupAgentConversation?: boolean;
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
    if (input.autoCreateFollowupAgentConversation !== undefined) {
      mockReviewSettings.auto_create_followup_agent_conversation =
        input.autoCreateFollowupAgentConversation;
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
      mockExternalMcpConfig.authToken =
        input.authToken === "" ? null : input.authToken;
    }
    if (input.nodePath !== undefined) {
      mockExternalMcpConfig.nodePath =
        input.nodePath === "" ? null : input.nodePath;
    }
  },

  // Plan branch commands
  get_plan_branch: async (args) => {
    const branch = await mockPlanBranchApi.getByPlan(
      args.planArtifactId as string,
    );
    return branch ? toSnakeCasePlanBranch(branch) : null;
  },
  get_project_plan_branches: async (args) => {
    const branches = await mockPlanBranchApi.getByProject(
      args.projectId as string,
    );
    return branches.map(toSnakeCasePlanBranch);
  },
  enable_feature_branch: async (args) => {
    const input = args.input as {
      plan_artifact_id: string;
      session_id: string;
      project_id: string;
    };
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
  terminalId = "default",
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
  args?: Record<string, unknown>,
): Promise<T> {
  // Add delay if configured (for testing loading states)
  const delay = (window as Window & { __mockInvokeDelay?: number })
    .__mockInvokeDelay;
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
  console.debug(
    `[mock] invoke("${cmd}", ${JSON.stringify(args)}) - no handler`,
  );
  console.warn(
    `[web-mode] No mock handler for "${cmd}". ` +
      `Add handler to tauri-api-core.ts or use api.* methods.`,
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
  _once?: boolean,
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
    console.debug(
      `[mock] PluginListener.unregister(${this.plugin}:${this.event})`,
    );
  }
}

/**
 * Mock addPluginListener - register plugin event listeners
 */
export async function addPluginListener<T>(
  plugin: string,
  event: string,
  _handler: (payload: T) => void,
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
