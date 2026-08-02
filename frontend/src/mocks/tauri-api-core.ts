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
import { getStore } from "@/api-mock/store";
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
  mockSetAgentConversationMuted,
  mockStartAgentConversation,
  mockSwitchAgentConversationMode,
  mockUpdateAgentConversationCoordinationMode,
} from "@/api-mock/chat";
import { mockReviewsApi } from "@/api-mock/reviews";
import { mockIdeationApi } from "@/api-mock/ideation";
import { mockExecutionApi } from "@/api-mock/execution";
import {
  mockPlanBranchApi,
  toSnakeCasePlanBranch,
} from "@/api-mock/plan-branch";
import { mockPlanApi } from "@/api-mock/plan";
import { mockArtifactApi } from "@/api-mock/artifact";
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
import type { NotificationCategory } from "@/types/notifications";
import type { InternalStatus, Task } from "@/types/task";

const mockReviewSettings = {
  require_human_review: false,
  require_workspace_review: true,
  max_fix_attempts: 3,
  max_revision_cycles: 2,
  ai_review_enabled: true,
  ai_review_auto_fix: true,
  require_fix_approval: false,
  auto_create_followup_agent_conversation: false,
  autofix_workspace_review_blocking_findings: true,
  workspace_review_fixer_cycle_cap: 3,
  run_task_validations: true,
};

let mockUpdateChannel: "stable" | "nightly" = "stable";

type MockUpdateChannelError = "read" | "write";

function getMockUpdateChannelError(): MockUpdateChannelError | undefined {
  return (
    window as Window & { __mockUpdateChannelError?: MockUpdateChannelError }
  ).__mockUpdateChannelError;
}

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
const mockAgentConversationGranolaNotes = new Map<string, unknown>();

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
    issueUrl:
      input.issueUrl ??
      `https://example.atlassian.net/browse/${input.issueKey}`,
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

const mockGranolaIntegrationSettings = {
  enabled: false,
  hasApiToken: false,
  validationStatus: "not_configured",
  lastValidatedAt: null as string | null,
  lastError: null as string | null,
  updatedAt: new Date(0).toISOString(),
};

const mockGranolaNotes = [
  {
    id: "not_1234567890ABCD",
    title: "Planning sync",
    url: "https://granola.ai/notes/not_1234567890ABCD",
    summary: "Mock Granola note summary for the planning sync.",
    createdAt: new Date(0).toISOString(),
    updatedAt: new Date(0).toISOString(),
  },
  {
    id: "not_ABCDEFGHIJKLMN",
    title: "Review follow-up",
    url: "https://granola.ai/notes/not_ABCDEFGHIJKLMN",
    summary: "Mock Granola note summary for a follow-up review.",
    createdAt: new Date(0).toISOString(),
    updatedAt: new Date(0).toISOString(),
  },
];

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

function mockGranolaNote(input: {
  conversationId: string;
  projectId?: string | null;
  noteId: string;
  title?: string | null;
  noteUrl?: string | null;
  summary?: string | null;
  includeTranscript?: boolean;
}) {
  const now = new Date(0).toISOString();
  const note = mockGranolaNotes.find((item) => item.id === input.noteId);
  return {
    conversationId: input.conversationId,
    projectId: input.projectId ?? "mock-project",
    provider: "granola",
    noteId: input.noteId,
    noteUrl: input.noteUrl ?? note?.url ?? null,
    title: input.title ?? note?.title ?? "Mock Granola note",
    summaryMarkdown: input.summary ?? note?.summary ?? null,
    transcript: [{ speaker: "Alex", text: "Mock transcript line." }],
    includeTranscript: input.includeTranscript ?? true,
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
      serviceTier: null,
      approvalPolicy: "never",
      sandboxMode: "danger-full-access",
      claudePermissionMode: null,
      claudeDangerouslySkipPermissions: false,
      claudeAllowDangerouslySkipPermissions: false,
      cliManagementMode: "user_managed",
      autoUpdateEnabled: false,
      customBinaryEnabled: false,
      customBinaryPath: null,
      customEnvFileEnabled: false,
      customEnvFilePath: null,
      available: true,
      binaryFound: true,
      binaryPath: "/opt/homebrew/bin/codex",
      status: "ready",
      error: null,
      missingCoreExecFeatures: [],
      supportedModelAliases: [
        "gpt-5.6-sol",
        "gpt-5.6-terra",
        "gpt-5.6-luna",
        "gpt-5.5",
      ],
      supportedEfforts: ["low", "medium", "high", "xhigh", "max", "ultra"],
      updatedAt: "2026-05-08T00:00:00Z",
    },
    {
      provider: "claude",
      enabled: false,
      isDefault: false,
      model: "claude-sonnet-5",
      effort: null,
      serviceTier: null,
      approvalPolicy: "never",
      sandboxMode: null,
      claudePermissionMode: "bypassPermissions",
      claudeDangerouslySkipPermissions: true,
      claudeAllowDangerouslySkipPermissions: true,
      cliManagementMode: "user_managed",
      autoUpdateEnabled: false,
      customBinaryEnabled: false,
      customBinaryPath: null,
      customEnvFileEnabled: false,
      customEnvFilePath: null,
      available: true,
      binaryFound: true,
      binaryPath: "/opt/homebrew/bin/claude",
      status: "ready",
      error: null,
      missingCoreExecFeatures: [],
      supportedModelAliases: [
        "fable",
        "claude-opus-5",
        "claude-opus-4-8",
        "claude-opus-4-7",
        "opus",
        "claude-sonnet-5",
        "claude-sonnet-4-6",
        "sonnet",
        "haiku",
      ],
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
      currentVersion: "2.1.197",
      latestVersion: "2.1.197",
      updateAvailable: false,
      action: "none",
      status: "claude CLI 2.1.197 is user-managed and current.",
      error: null,
    },
  ],
};

// Matches RawMcpCatalogSchema (snake_case serialization from the backend).
const mockMcpCatalog = {
  eligible_providers: ["codex"],
  eligible_default_provider: "codex",
  probed_at: "2026-05-08T00:00:00Z",
  probe_stale: false,
  provider_diagnostics: {},
  policy_diagnostics: [],
  servers: [
    {
      provider: "codex",
      server_id: "ralphx",
      native_scope: "user",
      native_state: "enabled",
      effective_enabled: true,
      configured_state: "follow",
      effective_state: "follow",
      effective_source: "required_internal",
      known_tools: [
        {
          tool_name: "list_agent_tasks",
          configured_state: "follow",
          effective_state: "follow",
          effective_source: "required_internal",
        },
        {
          tool_name: "create_agent_task",
          configured_state: "follow",
          effective_state: "follow",
          effective_source: "required_internal",
        },
      ],
      disabled_tools: [],
      locked: true,
      locked_reason: "Required internal RalphX server.",
      diagnostic: null,
      conflict_kind: null,
      repair_status: null,
    },
    {
      provider: "codex",
      server_id: "github",
      native_scope: "user",
      native_state: "enabled",
      effective_enabled: true,
      configured_state: "follow",
      effective_state: "follow",
      effective_source: "provider_native",
      known_tools: [
        {
          tool_name: "search_issues",
          configured_state: "follow",
          effective_state: "follow",
          effective_source: "provider_native",
        },
      ],
      disabled_tools: [],
      locked: false,
      locked_reason: null,
      diagnostic: null,
      conflict_kind: null,
      repair_status: null,
    },
  ],
};

const mockAgentModels = [
  {
    provider: "codex",
    modelId: "gpt-5.6-sol",
    label: "GPT-5.6 Sol",
    menuLabel: "GPT-5.6 Sol",
    description: "Flagship GPT-5.6 model for complex coding and agentic work.",
    supportedEfforts: ["low", "medium", "high", "xhigh", "max", "ultra"],
    defaultEffort: "medium",
    source: "built_in",
    enabled: true,
    createdAt: null,
    updatedAt: null,
  },
  {
    provider: "codex",
    modelId: "gpt-5.6-terra",
    label: "GPT-5.6 Terra",
    menuLabel: "GPT-5.6 Terra",
    description: "High-intelligence GPT-5.6 model for substantial coding tasks.",
    supportedEfforts: ["low", "medium", "high", "xhigh", "max", "ultra"],
    defaultEffort: "medium",
    source: "built_in",
    enabled: true,
    createdAt: null,
    updatedAt: null,
  },
  {
    provider: "codex",
    modelId: "gpt-5.6-luna",
    label: "GPT-5.6 Luna",
    menuLabel: "GPT-5.6 Luna",
    description: "Efficient GPT-5.6 model for capable everyday coding work.",
    supportedEfforts: ["low", "medium", "high", "xhigh", "max"],
    defaultEffort: "medium",
    source: "built_in",
    enabled: true,
    createdAt: null,
    updatedAt: null,
  },
  {
    provider: "codex",
    modelId: "gpt-5.5",
    label: "GPT-5.5",
    menuLabel: "GPT-5.5",
    description: "Frontier Codex model for complex agent work.",
    supportedEfforts: ["low", "medium", "high", "xhigh"],
    defaultEffort: "xhigh",
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
    modelId: "claude-opus-5",
    label: "Claude Opus 5",
    menuLabel: "Claude Opus 5",
    description: "Exact Claude Opus 5 model id; requires Claude Code 2.1.219 or newer.",
    supportedEfforts: ["low", "medium", "high", "xhigh", "max"],
    defaultEffort: "high",
    source: "built_in",
    enabled: true,
    createdAt: null,
    updatedAt: null,
  },
  {
    provider: "claude",
    modelId: "claude-opus-4-8",
    label: "Claude Opus 4.8",
    menuLabel: "Claude Opus 4.8",
    description: "Exact Claude Opus 4.8 model id; requires Claude Code 2.1.154 or newer.",
    supportedEfforts: ["low", "medium", "high", "xhigh", "max"],
    defaultEffort: "high",
    source: "built_in",
    enabled: true,
    createdAt: null,
    updatedAt: null,
  },
  {
    provider: "claude",
    modelId: "claude-opus-4-7",
    label: "Claude Opus 4.7",
    menuLabel: "Claude Opus 4.7",
    description: "Exact Claude Opus 4.7 model id; requires Claude Code 2.1.111 or newer.",
    supportedEfforts: ["low", "medium", "high", "xhigh", "max"],
    defaultEffort: "high",
    source: "built_in",
    enabled: true,
    createdAt: null,
    updatedAt: null,
  },
  {
    provider: "claude",
    modelId: "claude-sonnet-5",
    label: "Claude Sonnet 5",
    menuLabel: "Claude Sonnet 5",
    description: "Balanced Claude model for agent work.",
    supportedEfforts: ["low", "medium", "high", "xhigh", "max"],
    defaultEffort: "high",
    source: "built_in",
    enabled: true,
    createdAt: null,
    updatedAt: null,
  },
  {
    provider: "claude",
    modelId: "claude-sonnet-4-6",
    label: "Claude Sonnet 4.6",
    menuLabel: "Claude Sonnet 4.6",
    description: "Pinned Claude Sonnet 4.6 model for stable agent work.",
    supportedEfforts: ["low", "medium", "high", "max"],
    defaultEffort: "high",
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
  "execution_branch_updater",
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

const mockManualRoleFamilies = [
  ["workspace", "Workspace", "workspace_chat", "Chat", "General project conversation and workspace assistance."],
  ["workspace", "Workspace", "workspace_edit", "Edit", "Implements requested changes in the project workspace."],
  ["workspace", "Workspace", "workspace_plan", "Plan", "Develops implementation plans for project changes."],
  ["workspace", "Workspace", "workspace_ideation", "Ideation", "Explores product and technical ideas into actionable plans."],
  ["workspace", "Workspace", "workspace_review_pr", "Review PR", "Reviews pull request changes and reports actionable findings."],
  ["workspace", "Workspace", "workspace_automation", "Automation", "Runs configured project automation conversations."],
  ["automation", "Automation", "automation_plan_judge", "Plan Judge", "Evaluates whether an automation plan is ready to execute."],
  ["automation", "Automation", "automation_result_judge", "Result Judge", "Evaluates automation results and required follow-up."],
  ["feedback_loops", "Feedback Loops", "workspace_reviewer", "Reviewer", "Reviews workspace changes and identifies issues."],
  ["feedback_loops", "Feedback Loops", "workspace_repair", "Repair", "Repairs workspace setup, branch, or execution problems."],
  ["feedback_loops", "Feedback Loops", "workspace_merge_repair", "Merge Repair", "Resolves merge conflicts and incomplete merge states."],
  ["feedback_loops", "Feedback Loops", "workspace_pr_fixer", "PR Fixer", "Addresses pull request feedback and failing checks."],
  ["ideation", "Ideation", "ideation_primary", "Primary", "Leads ideation and produces the working plan."],
  ["ideation", "Ideation", "ideation_verifier", "Verifier", "Challenges an ideation plan before implementation."],
  ["ideation", "Ideation", "ideation_subagent", "Subagent", "Explores a focused question for the ideation lead."],
  ["ideation", "Ideation", "ideation_verifier_subagent", "Verifier Subagent", "Investigates a focused verification concern."],
  ["delegation", "Delegation", "delegated_subagent", "Delegated Subagent", "Handles a bounded task delegated by another agent."],
  ["execution", "Execution", "execution_worker", "Worker", "Implements an execution-plan task in its isolated workspace."],
  ["execution", "Execution", "execution_qa_prep", "QA Prep", "Prepares changed code for quality validation."],
  ["execution", "Execution", "execution_qa_refiner", "QA Refiner", "Refines changes in response to quality findings."],
  ["execution", "Execution", "execution_qa_tester", "QA Tester", "Runs targeted tests and reports behavioral evidence."],
  ["execution", "Execution", "execution_reviewer", "Reviewer", "Reviews completed execution work for correctness and scope."],
  ["execution", "Execution", "execution_reexecutor", "Re-executor", "Implements follow-up changes requested by review."],
  ["execution", "Execution", "execution_merger", "Merger", "Completes approved branch integration and merge cleanup."],
  ["utility", "Utility", "utility_lightweight", "Lightweight", "Handles small utility tasks with minimal runtime overhead."],
  ["utility", "Utility", "utility_pr_describer", "PR Describer", "Summarizes a completed change for pull request publication."],
  ["utility", "Utility", "utility_project_analyzer", "Project Analyzer", "Inspects a project and reports relevant implementation context."],
  ["utility", "Utility", "memory_capture", "Memory Capture", "Extracts durable project knowledge from completed work."],
  ["utility", "Utility", "memory_maintainer", "Memory Maintainer", "Curates and updates stored project knowledge."],
] as const;

const mockManualRoleDefault = {
  provider: "codex",
  model: "gpt-5.5",
  effort: "xhigh",
  service_tier: "provider_default",
  coordination_mode: "solo",
  persona_id: null,
  approval_policy: "never",
  sandbox_mode: "danger-full-access",
};

const mockManualRoleDefaultStore = new Map<string, Record<string, unknown>>();

function mockManualRoleScopeKey(projectId: string | null, role: string) {
  return `${projectId ?? "__global__"}:${role}`;
}

function mockManualRoleControls(family: string) {
  const workspaceRole = family === "workspace";
  return {
    capabilities: [
      { value: "solo", enabled: true, disabled_reason: null },
      {
        value: "rx_native_team",
        enabled: workspaceRole,
        disabled_reason: workspaceRole
          ? null
          : "Team is available only for Workspace root roles",
      },
      {
        value: "rx_native_workflow",
        enabled: workspaceRole,
        disabled_reason: workspaceRole
          ? null
          : "Workflow is available only for Workspace root roles",
      },
      {
        value: "codex_native_ultra",
        enabled: workspaceRole,
        disabled_reason: workspaceRole
          ? null
          : "Codex Ultra is available only for Workspace root roles",
      },
    ],
    speeds: [
      { value: "provider_default", enabled: true, disabled_reason: null },
      { value: "standard", enabled: true, disabled_reason: null },
      {
        value: "fast",
        enabled: true,
        disabled_reason: null,
      },
    ],
    persona: {
      enabled: workspaceRole,
      disabled_reason: workspaceRole
        ? null
        : "Persona is limited to Workspace Project conversations in V1",
    },
  };
}

function mockManualRoleDefaults(projectId: string | null) {
  return {
    project_id: projectId,
    roles: mockManualRoleFamilies.map(
      ([family, familyDisplayName, role, displayName, description]) => {
        const scopedConfigured = projectId === null
          ? null
          : mockManualRoleDefaultStore.get(
            mockManualRoleScopeKey(projectId, role),
          ) ?? null;
        const globalConfigured =
          mockManualRoleDefaultStore.get(mockManualRoleScopeKey(null, role)) ??
          (role === "workspace_edit" ? mockManualRoleDefault : null);
        const configured = projectId === null
          ? globalConfigured
          : scopedConfigured;
        return {
          role,
          display_name: displayName,
          description,
          family,
          family_display_name: familyDisplayName,
          configured,
          effective: configured ?? globalConfigured ?? mockManualRoleDefault,
          source: configured
            ? (projectId === null ? "global_ui" : "project_ui")
            : (globalConfigured ? "global_ui" : "provider_default"),
          diagnostics: [],
          controls: mockManualRoleControls(family),
        };
      },
    ),
  };
}

const mockWorkspaceReviewRuntimeSettings: Record<
  string,
  Array<{
    projectId: string | null;
    provider: string;
    model: string | null;
    effort: string | null;
    updatedAt: string;
  }>
> = {};

function workspaceReviewScopeKey(projectId: string | null) {
  return projectId ?? "__global__";
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
    coordination_mode: conversation.coordinationMode,
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
    auto_publish_initial_pr_enabled:
      workspace.autoPublishInitialPrEnabled ?? false,
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
  {
    id: "in_progress",
    name: "In Progress",
    category: "in_progress",
    order: 1,
    color: null,
  },
  {
    id: "review",
    name: "In Review",
    category: "in_progress",
    order: 2,
    color: null,
  },
  { id: "done", name: "Done", category: "done", order: 3, color: null },
];

const mockTicketingTickets = [
  {
    ref: { provider: "jira", id: "10001", key: "RX-1" },
    title: "Fix merge race in transition handler",
    state: { id: "todo", name: "To Do", category: "todo", color: null },
    assignee: { id: "user-1", name: "A. Dev", email: null, avatarUrl: null },
    reporter: { id: "user-2", name: "Platform", email: null, avatarUrl: null },
    labels: ["backend", "race-condition"],
    priority: "High",
    updatedAt: "2026-06-20T12:00:00.000Z",
    url: "https://example.atlassian.net/browse/RX-1",
    associationCount: 2,
  },
  {
    ref: { provider: "jira", id: "10002", key: "RX-2" },
    title: "Add Linear webhook backfill",
    state: {
      id: "in_progress",
      name: "In Progress",
      category: "in_progress",
      color: null,
    },
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
    state: {
      id: "review",
      name: "In Review",
      category: "in_progress",
      color: null,
    },
    assignee: { id: "user-1", name: "A. Dev", email: null, avatarUrl: null },
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
    state: {
      id: "in_progress",
      name: "In Progress",
      category: "in_progress",
      color: null,
    },
    assignee: { id: "cu-user-1", name: "A. Dev", email: null, avatarUrl: null },
    reporter: {
      id: "cu-user-2",
      name: "Platform",
      email: null,
      avatarUrl: null,
    },
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
    reporter: {
      id: "cu-user-2",
      name: "Platform",
      email: null,
      avatarUrl: null,
    },
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
    assignee: { id: "cu-user-1", name: "A. Dev", email: null, avatarUrl: null },
    reporter: {
      id: "cu-user-2",
      name: "Platform",
      email: null,
      avatarUrl: null,
    },
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

const mockNotificationSettings = {
  desktop_enabled: true,
  desktop_only_when_unfocused: true,
  focused_toasts_enabled: true,
  desktop_agent_requests_enabled: true,
  desktop_agent_waiting_enabled: true,
  desktop_reviews_enabled: true,
  desktop_task_failures_enabled: true,
  desktop_automation_approvals_enabled: true,
  desktop_automation_run_completions_enabled: false,
  desktop_git_github_enabled: true,
  muted_project_ids: [],
};

const TASK_ATTENTION_CATEGORIES: Partial<Record<InternalStatus, NotificationCategory>> = {
  review_passed: "review_needed",
  escalated: "review_escalated",
  qa_failed: "qa_failed",
  merge_conflict: "merge_conflict",
  merge_incomplete: "merge_incomplete",
  failed: "task_failed",
};

function taskAttentionCategory(task: Task): NotificationCategory | undefined {
  if (task.internalStatus === "blocked") {
    return task.blockedReason?.startsWith("human:") ? "task_blocked" : undefined;
  }
  return TASK_ATTENTION_CATEGORIES[task.internalStatus];
}

function mockAttentionItems() {
  return Array.from(getStore().tasks.values()).flatMap((task) => {
    const category = taskAttentionCategory(task);
    return category === undefined ? [] : [{
      id: `task:${task.id}:${category}`,
      category,
      title: task.title,
      detail: task.description,
      projectId: task.projectId,
      createdAt: task.updatedAt,
      target: { kind: "task" as const, projectId: task.projectId, taskId: task.id },
    }];
  });
}

function mockNotificationPage(args: Record<string, unknown>) {
  const projectId = args.projectId as string | undefined;
  const offset = typeof args.cursor === "string" ? Number.parseInt(args.cursor, 10) : 0;
  const limit = typeof args.limit === "number" ? args.limit : 50;
  const notifications = Array.from(getStore().notifications.values())
    .filter((notification) => projectId === undefined || notification.projectId === projectId)
    .sort((left, right) => right.createdAt.localeCompare(left.createdAt));
  const start = Number.isFinite(offset) && offset >= 0 ? offset : 0;
  const page = notifications.slice(start, start + limit);
  const nextOffset = start + page.length;

  return {
    notifications: page,
    cursor: nextOffset < notifications.length ? String(nextOffset) : null,
    hasMore: nextOffset < notifications.length,
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
  get_artifact_version_history: async (args) =>
    mockArtifactApi.getVersionHistory(args.id as string),

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
  get_mcp_catalog: async () => mockMcpCatalog,
  refresh_mcp_catalog: async () => mockMcpCatalog,
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
  get_ui_feature_flags: async () => {
    const overrides =
      typeof window !== "undefined" ? window.__mockUiFeatureFlags : undefined;
    return {
      activityPage: true,
      extensibilityPage: true,
      automationsPage: true,
      atlassianOauth: false,
      ticketingDashboard: false,
      ...overrides,
    };
  },
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
  get_granola_integration_settings: async () => mockGranolaIntegrationSettings,
  save_granola_integration_settings: async (args) => {
    const input = args.input as { apiToken?: string | null };
    if (input.apiToken !== undefined) {
      mockGranolaIntegrationSettings.hasApiToken = Boolean(
        input.apiToken?.trim(),
      );
      mockGranolaIntegrationSettings.enabled = false;
      mockGranolaIntegrationSettings.validationStatus =
        mockGranolaIntegrationSettings.hasApiToken
          ? "pending"
          : "not_configured";
    }
    mockGranolaIntegrationSettings.lastError = null;
    mockGranolaIntegrationSettings.updatedAt = new Date(0).toISOString();
    return mockGranolaIntegrationSettings;
  },
  validate_granola_integration_settings: async () => {
    Object.assign(mockGranolaIntegrationSettings, {
      enabled: true,
      hasApiToken: true,
      validationStatus: "valid",
      lastValidatedAt: new Date(0).toISOString(),
      lastError: null,
      updatedAt: new Date(0).toISOString(),
    });
    return mockGranolaIntegrationSettings;
  },
  list_granola_notes: async () => ({
    notes: mockGranolaNotes,
    hasMore: false,
    cursor: null,
  }),
  get_granola_note_detail: async (args) => {
    const input = args.input as { noteId: string };
    const note =
      mockGranolaNotes.find((item) => item.id === input.noteId) ??
      mockGranolaNotes[0];
    return {
      ...note,
      transcript: [{ speaker: "Alex", text: "Mock transcript line." }],
    };
  },
  get_agent_conversation_granola_note: async (args) => {
    const input = args.input as { conversationId: string };
    return {
      note: mockAgentConversationGranolaNotes.get(input.conversationId) ?? null,
    };
  },
  assign_agent_conversation_granola_note: async (args) => {
    const input = args.input as {
      conversationId: string;
      projectId?: string | null;
      noteId: string;
      title?: string | null;
      noteUrl?: string | null;
      summary?: string | null;
      includeTranscript?: boolean;
    };
    const note = mockGranolaNote(input);
    mockAgentConversationGranolaNotes.set(input.conversationId, note);
    return { note };
  },
  refresh_agent_conversation_granola_note: async (args) => {
    const input = args.input as { conversationId: string };
    const existing = mockAgentConversationGranolaNotes.get(
      input.conversationId,
    );
    if (!existing) {
      return { note: null };
    }
    return { note: existing };
  },
  clear_agent_conversation_granola_note: async (args) => {
    const input = args.input as { conversationId: string };
    mockAgentConversationGranolaNotes.delete(input.conversationId);
    return { note: null };
  },
  get_agent_conversation_linear_issue: async (args) => {
    const input = args.input as { conversationId: string };
    return {
      issue:
        mockAgentConversationLinearIssues.get(input.conversationId) ?? null,
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
    const existing = mockAgentConversationLinearIssues.get(
      input.conversationId,
    );
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
    const query = args.query as
      { provider?: string; filters?: { text?: string } } | undefined;
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
  list_ticket_filter_options: async (args) => {
    const query = args.query as { provider?: string } | undefined;
    const provider = query?.provider ?? "jira";
    const assignees = Array.from(
      new Set(
        mockTicketingTickets
          .filter((ticket) => ticket.ref.provider === provider)
          .flatMap((ticket) => {
            const assignees =
              "assignees" in ticket && Array.isArray(ticket.assignees)
                ? ticket.assignees
                : [];
            const assignee =
              "assignee" in ticket && ticket.assignee ? [ticket.assignee] : [];
            return [...assignees, ...assignee] as Array<{ name?: string | null }>;
          })
          .map((assignee) => assignee.name)
          .filter((name): name is string => Boolean(name)),
      ),
    ).sort((left, right) => left.localeCompare(right));

    return {
      assignees,
      sprints: [],
      complete: true,
      truncated: false,
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
      acceptanceCriteriaMarkdown:
        "- No double-transition under contention\n- Activity timeline remains ordered",
      comments: [
        {
          id: "comment-1",
          author: {
            id: "user-2",
            name: "Platform",
            email: null,
            avatarUrl: null,
          },
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
    const input = args.input as
      | {
          provider?: string;
          ticketRef?: { provider?: string; id?: string; key?: string | null };
          labels?: string[];
          clientOperationId?: string;
        }
      | undefined;
    const labels = input?.labels ?? [];
    return {
      ticketRef: input?.ticketRef ?? {
        provider: input?.provider ?? "jira",
        id: "10001",
      },
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
  get_manual_role_defaults: async (args) =>
    mockManualRoleDefaults((args.projectId as string | null | undefined) ?? null),
  update_manual_role_default: async (args) => {
    const input = args.input as {
      projectId?: string | null;
      role: string;
      value: Record<string, unknown>;
    };
    mockManualRoleDefaultStore.set(
      mockManualRoleScopeKey(input.projectId ?? null, input.role),
      input.value,
    );
    return input.value;
  },
  clear_manual_role_default: async (args) => {
    const input = args.input as { projectId?: string | null; role: string };
    return mockManualRoleDefaultStore.delete(
      mockManualRoleScopeKey(input.projectId ?? null, input.role),
    );
  },
  get_start_composer_role_default: async () => ({
    role: "workspace_edit",
    source: "global_ui",
    value: mockManualRoleDefault,
  }),
  get_agent_conversation_role_default: async () => ({
    role: "workspace_edit",
    source: "global_ui",
    value: mockManualRoleDefault,
  }),
  reset_agent_conversation_role_default: async () => ({
    role: "workspace_edit",
    source: "global_ui",
    value: mockManualRoleDefault,
  }),
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
  get_workspace_review_runtime_settings: async (args) => {
    const projectId = (args.projectId as string | null | undefined) ?? null;
    return [
      ...(mockWorkspaceReviewRuntimeSettings[
        workspaceReviewScopeKey(projectId)
      ] ?? []),
    ];
  },
  update_workspace_review_runtime_settings: async (args) => {
    const input = args.input as {
      projectId?: string | null;
      provider: string;
      model?: string | null;
      effort?: string | null;
    };
    const projectId = input.projectId ?? null;
    const scopeKey = workspaceReviewScopeKey(projectId);
    const rows = (mockWorkspaceReviewRuntimeSettings[scopeKey] ??= []);
    const existing = rows.find((row) => row.provider === input.provider);
    const row = {
      projectId,
      provider: input.provider,
      model: input.model ?? null,
      effort: input.effort ?? null,
      updatedAt: "2026-05-08T00:00:00Z",
    };
    if (existing) {
      Object.assign(existing, row);
      return existing;
    }
    rows.push(row);
    return row;
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
  get_github_connection_status: async () => ({
    state: (window.__mockGhAuthStatus ?? true) ? "authenticated" : "unauthenticated",
    diagnostic: (window.__mockGhAuthStatus ?? true) ? null : "missing_credentials",
    ghInstalled: true,
    authenticated: window.__mockGhAuthStatus ?? true,
    host: "github.com",
    account: "mock-octocat",
  }),
  get_github_branch_overview: async () => ({
    currentBranch: "feature/mock-branch",
    sourcesUnavailable: [],
    branches: [
      {
        branchName: "feature/mock-branch",
        isCurrent: true,
        prNumber: 42,
        prTitle: "Mock branch overview",
        prUrl: "https://github.com/aigentive/ralphx.app/pull/42",
        prStatus: "open",
        prIsDraft: false,
        prUpdatedAt: "2026-06-28T00:00:00Z",
        prAuthorLogin: "mock-octocat",
        prBaseRefName: "main",
        rxConversationCount: 1,
        rxConversations: [
          { conversationId: "mock-conversation", title: "Mock agent" },
        ],
        ticketCount: 1,
        ticketLinks: [
          {
            provider: "jira",
            label: "RX-42",
            title: "Mock ticket",
            url: "https://example.atlassian.net/browse/RX-42",
          },
        ],
        ticketLabels: ["Jira RX-42"],
      },
      {
        branchName: "feature/no-pr",
        isCurrent: false,
        prNumber: null,
        prTitle: null,
        prUrl: null,
        prStatus: null,
        prIsDraft: false,
        prUpdatedAt: null,
        prAuthorLogin: null,
        prBaseRefName: null,
        rxConversationCount: 0,
        rxConversations: [],
        ticketCount: 0,
        ticketLinks: [],
        ticketLabels: [],
      },
      {
        branchName: "feature/merged",
        isCurrent: false,
        prNumber: 41,
        prTitle: "Merged mock branch",
        prUrl: "https://github.com/aigentive/ralphx.app/pull/41",
        prStatus: "merged",
        prIsDraft: false,
        prUpdatedAt: "2026-06-27T00:00:00Z",
        prAuthorLogin: "mock-octocat",
        prBaseRefName: "main",
        rxConversationCount: 0,
        rxConversations: [],
        ticketCount: 0,
        ticketLinks: [],
        ticketLabels: [],
      },
      {
        branchName: "ralphx/ticket/clickup-cu-1",
        isCurrent: false,
        prNumber: null,
        prTitle: null,
        prUrl: null,
        prStatus: null,
        prIsDraft: false,
        prUpdatedAt: null,
        prAuthorLogin: null,
        prBaseRefName: null,
        rxConversationCount: 0,
        rxConversations: [],
        ticketCount: 1,
        ticketLinks: [
          {
            provider: "clickup",
            label: "cu-1",
            title: null,
            url: null,
          },
        ],
        ticketLabels: ["ClickUp cu-1"],
      },
    ],
  }),
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
  update_github_pr_enabled: async (args) => {
    const projectId = args.projectId as string;
    const project = getStore().projects.get(projectId);
    if (!project) {
      throw new Error(`Project not found: ${projectId}`);
    }
    getStore().projects.set(projectId, {
      ...project,
      githubPrEnabled: args.enabled === true,
    });
    return null;
  },

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

  // Notification commands
  list_attention_items: async (args) => {
    const projectId = args.projectId as string | undefined;
    return mockAttentionItems().filter(
      (item) => projectId === undefined || item.projectId === projectId,
    );
  },
  get_unread_notification_count: async (args) => {
    const projectId = args.projectId as string | undefined;
    return Array.from(getStore().notifications.values()).filter(
      (notification) => notification.readAt === null && (projectId === undefined || notification.projectId === projectId),
    ).length;
  },
  list_notifications: async (args) => mockNotificationPage(args),
  mark_notification_read: async (args) => {
    const notification = getStore().notifications.get(args.id as string);
    if (notification) {
      getStore().notifications.set(notification.id, { ...notification, readAt: new Date().toISOString() });
    }
    return null;
  },
  mark_all_notifications_read: async (args) => {
    const projectId = args.projectId as string | undefined;
    const store = getStore();
    Array.from(store.notifications.values()).forEach((notification) => {
      if (notification.readAt === null && (projectId === undefined || notification.projectId === projectId)) {
        store.notifications.set(notification.id, { ...notification, readAt: new Date().toISOString() });
      }
    });
    return null;
  },
  set_dock_badge_count: async () => null,
  get_notification_settings: async () => mockNotificationSettings,
  update_notification_settings: async () => mockNotificationSettings,

  // Task commands
  get_session_task_history_availability: async (args) => {
    const availability = await mockTasksApi.getSessionHistoryAvailability(
      args.projectId as string,
      args.ideationSessionId as string,
    );
    return {
      has_history: availability.hasHistory,
      task_count: availability.taskCount,
    };
  },
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
  retry_branch_update: async (args) => {
    const task = await mockTasksApi.retryBranchUpdate(args.taskId as string);
    return {
      id: task.id,
      project_id: task.projectId,
      category: task.category,
      title: task.title,
      description: task.description,
      internal_status: task.internalStatus,
      priority: task.priority,
      needs_review_point: task.needsReviewPoint,
      created_at: task.createdAt,
      updated_at: task.updatedAt,
      started_at: task.startedAt,
      completed_at: task.completedAt,
      archived_at: task.archivedAt,
      blocked_reason: task.blockedReason,
      task_branch: task.taskBranch ?? null,
      metadata: task.metadata ?? null,
    };
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
      coordination_mode: conversation.coordinationMode,
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
        coordination_mode: conversation.coordinationMode,
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
          attention_lane: row.attentionLane ?? "needs",
          parked_delegate_count: row.parkedDelegateCount ?? 0,
          action_verb: row.actionVerb ?? "",
          is_muted: row.isMuted ?? false,
        })),
      })),
    };
  },
  set_agent_conversation_muted: async (args) => {
    const controller =
      typeof window !== "undefined" ? window.__mockChatApi : undefined;
    const input = args.input as {
      conversationId: string;
      muted: boolean;
    };
    if (controller?.setAgentConversationMuted) {
      await controller.setAgentConversationMuted(
        input.conversationId,
        input.muted,
      );
    } else {
      await mockSetAgentConversationMuted(input.conversationId, input.muted);
    }
    return null;
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
      auto_publish_initial_pr_enabled:
        workspace.autoPublishInitialPrEnabled ?? false,
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
            auto_publish_initial_pr_enabled:
              workspace.autoPublishInitialPrEnabled ?? false,
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
  get_agent_conversation_workspace_staged_file_changes: async () => [],
  get_agent_conversation_workspace_unstaged_file_changes: async () => [],
  get_agent_conversation_workspace_cumulative_file_changes: async () =>
    mockWorkspaceFileChanges.map((change) => ({ ...change })),
  get_agent_conversation_workspace_pr_annotations: async (args) => {
    const workspace = await mockGetAgentConversationWorkspace(
      args.conversationId as string,
    );
    return {
      pr_number: workspace?.publicationPrNumber ?? 0,
      head_sha: null,
      annotations: [],
      sources_unavailable: [],
    };
  },
  get_agent_conversation_workspace_review_hunk_annotations: async () => ({
    artifact_id: null,
    artifact_version: null,
    target_scope: null,
    head_sha: null,
    diff_fingerprint: null,
    annotations: [],
  }),
  get_agent_conversation_workspace_review: async (args) => {
    const workspace = await mockGetAgentConversationWorkspace(
      args.conversationId as string,
    );
    const publicationStatus = workspace?.publicationPrStatus
      ?.trim()
      .toLowerCase();
    const supportsWorktreeModes =
      publicationStatus !== "merged" && publicationStatus !== "closed";
    return {
      changes: mockWorkspaceFileChanges.map((change) => ({ ...change })),
      commits: mockWorkspaceCommits.map((commit) => ({ ...commit })),
      base_ref: "main",
      head_ref: "HEAD",
      supports_worktree_modes: supportsWorktreeModes,
    };
  },
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
      coordination_mode: conversation.coordinationMode,
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
        coordination_mode: conversation.coordinationMode,
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
            auto_publish_initial_pr_enabled:
              workspace.autoPublishInitialPrEnabled ?? false,
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
        coordination_mode: conversation.coordinationMode,
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
            auto_publish_initial_pr_enabled:
              workspace.autoPublishInitialPrEnabled ?? false,
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
  update_agent_conversation_coordination_mode: async (args) => {
    const input = args.input as Parameters<
      typeof mockUpdateAgentConversationCoordinationMode
    >[0];
    const conversation = await mockUpdateAgentConversationCoordinationMode(input);
    return toSnakeConversation(conversation);
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
      processedTokens: number | null;
      estimatedUsd: number | null;
    }) => ({
      input_tokens: usage.inputTokens,
      output_tokens: usage.outputTokens,
      cache_creation_tokens: usage.cacheCreationTokens,
      cache_read_tokens: usage.cacheReadTokens,
      processed_tokens: usage.processedTokens,
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
        effective_run_conversation_count:
          stats.usageCoverage.effectiveRunConversationCount,
        effective_message_conversation_count:
          stats.usageCoverage.effectiveMessageConversationCount,
        legacy_estimated_sample_count:
          stats.usageCoverage.legacyEstimatedSampleCount,
        fallback_estimated_sample_count:
          stats.usageCoverage.fallbackEstimatedSampleCount,
        uncounted_sample_count: stats.usageCoverage.uncountedSampleCount,
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
  get_update_channel: async () => {
    if (getMockUpdateChannelError() === "read") {
      throw new Error("Mock update channel read failure");
    }
    return mockUpdateChannel;
  },
  set_update_channel: async (args) => {
    if (getMockUpdateChannelError() === "write") {
      throw new Error("Mock update channel write failure");
    }
    const updateChannel = args.updateChannel;
    if (updateChannel === "stable" || updateChannel === "nightly") {
      mockUpdateChannel = updateChannel;
    }
    return mockUpdateChannel;
  },
  list_release_notes_versions: async () => ["0.76.0", "0.75.0", "0.74.0"],
  get_release_notes_for_version: async (args) => ({
    version: String(args.version),
    body: `## RalphX ${String(args.version)}\n\n- Release history improvements\n- Faster agent workflows`,
    source: "development_checkout",
  }),
  get_current_release_notes: async () => ({
    version: "0.76.0",
    body: null,
    source: "development_checkout",
  }),
  get_last_seen_release_notes_version: async () => "0.76.0",
  mark_release_notes_seen: async () => undefined,
  update_review_settings: async (args) => {
    const input = args.input as {
      requireHumanReview?: boolean;
      requireWorkspaceReview?: boolean;
      maxFixAttempts?: number;
      maxRevisionCycles?: number;
      autoCreateFollowupAgentConversation?: boolean;
      autofixWorkspaceReviewBlockingFindings?: boolean;
      workspaceReviewFixerCycleCap?: number;
      runTaskValidations?: boolean;
    };
    if (input.requireHumanReview !== undefined) {
      mockReviewSettings.require_human_review = input.requireHumanReview;
    }
    if (input.requireWorkspaceReview !== undefined) {
      mockReviewSettings.require_workspace_review =
        input.requireWorkspaceReview;
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
    if (input.autofixWorkspaceReviewBlockingFindings !== undefined) {
      mockReviewSettings.autofix_workspace_review_blocking_findings =
        input.autofixWorkspaceReviewBlockingFindings;
    }
    if (input.workspaceReviewFixerCycleCap !== undefined) {
      mockReviewSettings.workspace_review_fixer_cycle_cap = Math.max(
        0,
        input.workspaceReviewFixerCycleCap,
      );
    }
    if (input.runTaskValidations !== undefined) {
      mockReviewSettings.run_task_validations = input.runTaskValidations;
    }
    return { ...mockReviewSettings };
  },
  get_task_validation_summary: async (args) => {
    const taskId = (args.taskId ?? args.task_id ?? "mock-task") as string;
    return {
      task_id: taskId,
      project_id: "mock-project",
      policy_enabled: mockReviewSettings.run_task_validations,
      latest_run: null,
      commands: [],
      legacy_validation_cache: null,
      disabled_reason: mockReviewSettings.run_task_validations
        ? null
        : "Run Task Validations is disabled in Review Policy",
    };
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
  get_startup_status: async () => ({
    boot_id: "web-mode-boot",
    attempt_id: 1,
    stage: "ready",
    started_at: new Date().toISOString(),
    stage_started_at: new Date().toISOString(),
    completed_at: new Date().toISOString(),
    app_state_ready: true,
    runtime_ready: true,
    background_complete: true,
    retry_allowed: false,
    progress: null,
    message_code: "ready",
    failure_code: null,
    diagnostic_summary: null,
  }),
  retry_startup: async () => ({
    boot_id: "web-mode-boot",
    attempt_id: 2,
    stage: "ready",
    started_at: new Date().toISOString(),
    stage_started_at: new Date().toISOString(),
    completed_at: new Date().toISOString(),
    app_state_ready: true,
    runtime_ready: true,
    background_complete: true,
    retry_allowed: false,
    progress: null,
    message_code: "ready",
    failure_code: null,
    diagnostic_summary: null,
  }),
  report_startup_frontend_milestone: async () => null,
  open_startup_logs: async () => null,
  get_startup_diagnostics: async () => ({
    attempt_id: 1,
    stage: "ready",
    message_code: "ready",
    failure_code: null,
    can_retry: false,
  }),
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
