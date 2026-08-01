// Application state container for dependency injection
// Holds repository trait objects that can be swapped for testing

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Instant;
use tauri::{AppHandle, Manager, Runtime};
use tokio::sync::Mutex;

use super::services::PrPollerRegistry;
use crate::application::agent_capability_gate::AgentCapabilityGate;
use crate::application::app_paths::AppPaths;
use crate::application::chat_service::AppChatService;
use crate::application::notification_service::{
    NoopDesktopNotifier, NoopNotificationEventEmitter, NotificationEventEmitter,
    NotificationService, TauriDesktopNotifier, TauriNotificationEventEmitter, WindowFocusState,
};
use crate::application::runtime_factory::{
    build_chat_service_from_deps, build_task_scheduler_from_deps,
    build_transition_service_from_deps, ChatRuntimeFactoryDeps, RuntimeFactoryDeps,
};
use crate::application::startup_git_auth_preflight::StartupGitAuthRecoveryState;
use crate::application::startup_status::StartupCoordinator;
use crate::application::task_cleanup_service::TaskCleanupService;
use crate::application::tasks_feature_toggle_service::TasksFeatureToggleService;
use crate::application::AgentClientBundle;
use crate::application::AgentTerminalService;
use crate::application::AtlassianIntegrationService;
use crate::application::ClickUpIntegrationService;
use crate::application::EmptyAtlassianApiClient;
use crate::application::EmptyClickUpApiClient;
use crate::application::EmptyGranolaApiClient;
use crate::application::EmptyLinearApiClient;
use crate::application::ExternalIssueLinkService;
use crate::application::GranolaIntegrationService;
use crate::application::LinearIntegrationService;
use crate::application::PermissionState;
use crate::application::QuestionState;
use crate::application::RemoteEnvironmentService;
use crate::application::ResumeValidator;
use crate::application::TaskSchedulerService;
use crate::application::TaskTransitionService;
use crate::application::TicketingStatusCatalogService;
use crate::application::UnavailableAtlassianApiClient;
use crate::application::UnavailableClickUpApiClient;
use crate::application::UnavailableGranolaApiClient;
use crate::application::UnavailableLinearApiClient;
use crate::commands::ExecutionState;

pub type ApplicationExecutionState = ExecutionState;
use crate::domain::agents::{
    default_approval_policy_for_harness, default_sandbox_mode_for_harness, AgentHarnessKind,
    AgentProviderSettings, AgenticClient, LogicalEffort, RoutingRole,
    WorkspaceReviewRuntimeSettings, DEFAULT_AGENT_HARNESS,
};
use crate::domain::entities::ProjectId;
use crate::domain::ideation::{IdeationSettings, TasksFeatureState};
use crate::domain::qa::QASettings;
use crate::domain::repositories::{
    ActivePlanRepository, ActivityEventRepository, AgentConversationGranolaNoteRepository,
    AgentConversationIssueRepository, AgentConversationJiraIssueRepository,
    AgentConversationLinearIssueRepository, AgentConversationMuteRepository,
    AgentConversationWorkspaceRepository, AgentLaneSettingsRepository,
    AgentModelRegistryRepository, AgentProfileRepository, AgentProviderSettingsRepository,
    AgentRunRepository, AgentTaskRepository, AgentWorkflowRepository,
    AgentWorkspaceRepairRepository, ApiKeyRepository, AppStateRepository, ArtifactBucketRepository,
    ArtifactFlowRepository, ArtifactRepository, AutomationRepository, AutomationRunRepository,
    BranchUpdateRepository, ChatAttachmentRepository, ChatConversationRepository,
    ChatMessageRepository, ChatTimelineRepository, ConversationFolderReferenceRepository,
    DelegatedSessionRepository, ExecutionPlanRepository, ExecutionSettingsRepository,
    ExternalEventsRepository, GlobalExecutionSettingsRepository, IdeationEffortSettingsRepository,
    IdeationModelSettingsRepository, IdeationSessionRepository, IdeationSettingsRepository,
    ManualRoleDefaultRepository, McpPolicyRepository, MemoryArchiveRepository,
    MemoryEntryRepository, MemoryEventRepository, MethodologyRepository, NotificationRepository,
    NotificationSettingsRepository, OrphanWorktreeCleanupMarkerRepository, PersonaRepository,
    PlanArtifactApprovalRepository, PlanBranchRepository, PlanSelectionStatsRepository,
    ProcessRepository, ProjectRepository, ProposalDependencyRepository, QueuedMessageRepository,
    RemoteAgentStopRequestRepository, RemoteConversationStartRequestRepository, ReviewRepository,
    ReviewSettingsRepository, SessionLinkRepository, TaskDependencyRepository,
    TaskProposalRepository, TaskQARepository, TaskRepository, TaskStepRepository,
    TicketCanonicalBranchRepository, UiFeatureFlagOverridesRepository, ValidationRunRepository,
    WebhookRegistrationRepository, WorkflowRepository, WorkspaceReviewRuntimeSettingsRepository,
};
use crate::domain::services::{
    GithubServiceTrait, MemoryRunningAgentRegistry, MessageQueue, RunningAgentRegistry,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::memory::{
    InMemoryMemoryEntryRepository, InMemoryMemoryEventRepository, MemoryActivePlanRepository,
    MemoryActivityEventRepository, MemoryAgentConversationGranolaNoteRepository,
    MemoryAgentConversationIssueRepository, MemoryAgentConversationJiraIssueRepository,
    MemoryAgentConversationLinearIssueRepository, MemoryAgentConversationMuteRepository,
    MemoryAgentConversationWorkspaceRepository, MemoryAgentLaneSettingsRepository,
    MemoryAgentModelRegistryRepository, MemoryAgentProfileRepository,
    MemoryAgentProviderSettingsRepository, MemoryAgentRunRepository, MemoryAgentTaskRepository,
    MemoryApiKeyRepository, MemoryAppStateRepository, MemoryArtifactBucketRepository,
    MemoryArtifactFlowRepository, MemoryArtifactRepository,
    MemoryAtlassianIntegrationSettingsRepository, MemoryAutomationRepository,
    MemoryAutomationRunRepository, MemoryBranchUpdateRepository, MemoryChatAttachmentRepository,
    MemoryChatConversationRepository, MemoryChatMessageRepository, MemoryChatTimelineRepository,
    MemoryClickUpIntegrationSettingsRepository, MemoryConversationFolderReferenceRepository,
    MemoryDelegatedSessionRepository, MemoryExecutionPlanRepository,
    MemoryExecutionSettingsRepository, MemoryExternalEventsRepository,
    MemoryExternalIssueLinkRepository, MemoryGlobalExecutionSettingsRepository,
    MemoryGranolaIntegrationSettingsRepository, MemoryIdeationEffortSettingsRepository,
    MemoryIdeationModelSettingsRepository, MemoryIdeationSessionRepository,
    MemoryIdeationSettingsRepository, MemoryLinearIntegrationSettingsRepository,
    MemoryManualRoleDefaultRepository, MemoryMcpPolicyRepository, MemoryMethodologyRepository,
    MemoryNotificationRepository, MemoryNotificationSettingsRepository,
    MemoryOrphanWorktreeCleanupMarkerRepository, MemoryPermissionRepository,
    MemoryPersonaRepository, MemoryPlanArtifactApprovalRepository, MemoryPlanBranchRepository,
    MemoryPlanSelectionStatsRepository, MemoryProcessRepository, MemoryProjectRepository,
    MemoryProposalDependencyRepository, MemoryQuestionRepository, MemoryQueuedMessageRepository,
    MemoryRemoteAgentStopRequestRepository, MemoryRemoteConversationStartRequestRepository,
    MemoryReviewIssueRepository, MemoryReviewRepository, MemoryReviewSettingsRepository,
    MemorySecretStore, MemorySessionLinkRepository, MemoryTaskDependencyRepository,
    MemoryTaskProposalRepository, MemoryTaskQARepository, MemoryTaskRepository,
    MemoryTaskStepRepository, MemoryTicketCanonicalBranchRepository,
    MemoryTicketingStatusCatalogRepository, MemoryUiFeatureFlagOverridesRepository,
    MemoryValidationRunRepository, MemoryWebhookRegistrationRepository, MemoryWorkflowRepository,
    MemoryWorkspaceReviewRuntimeSettingsRepository,
};
use crate::infrastructure::secret_store::MacosKeychainSecretStore;
use crate::infrastructure::sqlite::migrations::{run_migrations_with_observer, MigrationProgress};
use crate::infrastructure::sqlite::ReviewIssueRepository;
use crate::infrastructure::sqlite::{
    open_connection, run_migrations, SqliteActivePlanRepository, SqliteActivityEventRepository,
    SqliteAgentConversationGranolaNoteRepository, SqliteAgentConversationIssueRepository,
    SqliteAgentConversationJiraIssueRepository, SqliteAgentConversationLinearIssueRepository,
    SqliteAgentConversationMuteRepository, SqliteAgentConversationWorkspaceRepository,
    SqliteAgentLaneSettingsRepository, SqliteAgentModelRegistryRepository,
    SqliteAgentProfileRepository, SqliteAgentProviderSettingsRepository, SqliteAgentRunRepository,
    SqliteAgentTaskRepository, SqliteAgentWorkflowRepository, SqliteApiKeyRepository,
    SqliteAppStateRepository, SqliteArtifactBucketRepository, SqliteArtifactFlowRepository,
    SqliteArtifactRepository, SqliteAtlassianIntegrationSettingsRepository,
    SqliteAutomationRepository, SqliteAutomationRunRepository, SqliteBranchUpdateRepository,
    SqliteChatAttachmentRepository, SqliteChatConversationRepository, SqliteChatMessageRepository,
    SqliteChatTimelineRepository, SqliteClickUpIntegrationSettingsRepository,
    SqliteConversationFolderReferenceRepository, SqliteDelegatedSessionRepository,
    SqliteExecutionPlanRepository, SqliteExecutionSettingsRepository,
    SqliteExternalEventsRepository, SqliteExternalIssueLinkRepository,
    SqliteGlobalExecutionSettingsRepository, SqliteGranolaIntegrationSettingsRepository,
    SqliteIdeationEffortSettingsRepository, SqliteIdeationModelSettingsRepository,
    SqliteIdeationSessionRepository, SqliteIdeationSettingsRepository,
    SqliteLinearIntegrationSettingsRepository, SqliteManualRoleDefaultRepository,
    SqliteMcpPolicyRepository, SqliteMemoryArchiveRepository, SqliteMemoryEntryRepository,
    SqliteMemoryEventRepository, SqliteMethodologyRepository, SqliteNotificationRepository,
    SqliteNotificationSettingsRepository, SqliteOrphanWorktreeCleanupMarkerRepository,
    SqlitePermissionRepository, SqlitePersonaRepository, SqlitePlanArtifactApprovalRepository,
    SqlitePlanBranchRepository, SqlitePlanSelectionStatsRepository, SqliteProcessRepository,
    SqliteProjectRepository, SqliteProposalDependencyRepository, SqliteQuestionRepository,
    SqliteQueuedMessageRepository, SqliteRemoteAgentStopRequestRepository,
    SqliteRemoteConversationStartRequestRepository, SqliteReviewIssueRepository,
    SqliteReviewRepository, SqliteReviewSettingsRepository, SqliteRunningAgentRegistry,
    SqliteSessionLinkRepository, SqliteTaskDependencyRepository, SqliteTaskProposalRepository,
    SqliteTaskQARepository, SqliteTaskRepository, SqliteTaskStepRepository,
    SqliteTicketCanonicalBranchRepository, SqliteTicketingStatusCatalogRepository,
    SqliteUiFeatureFlagOverridesRepository, SqliteValidationRunRepository,
    SqliteWebhookRegistrationRepository, SqliteWorkflowRepository,
    SqliteWorkspaceReviewRuntimeSettingsRepository,
};
use crate::infrastructure::HyperAtlassianApiClient;
use crate::infrastructure::HyperClickUpApiClient;
use crate::infrastructure::HyperLinearApiClient;
use crate::infrastructure::{GhCliGithubService, HyperGranolaApiClient};
use ralphx_events::{EventSink, InternalEventBus, NullEventSink};

pub(crate) struct ResolvedBackgroundAgentRuntime {
    pub client: Arc<dyn AgenticClient>,
    pub harness: Option<AgentHarnessKind>,
    pub model: Option<String>,
    pub cli_path_override: Option<PathBuf>,
    pub logical_effort: Option<LogicalEffort>,
    pub approval_policy: Option<String>,
    pub sandbox_mode: Option<String>,
    pub service_tier: Option<String>,
    pub env: HashMap<String, String>,
}

impl ResolvedBackgroundAgentRuntime {
    pub(crate) fn env_with_overrides(
        &self,
        overrides: HashMap<String, String>,
    ) -> HashMap<String, String> {
        let mut env = self.env.clone();
        env.extend(overrides);
        env
    }
}

/// Application state container for dependency injection
/// Holds repository trait objects that can be swapped for testing vs production
#[derive(Clone)]
pub struct AppState {
    /// Task repository (SQLite in production, in-memory for tests)
    pub task_repo: Arc<dyn TaskRepository>,
    /// Durable branch-update operation and canonical Git target authority.
    pub branch_update_repo: Arc<dyn BranchUpdateRepository>,
    /// Task step repository for tracking execution progress
    pub task_step_repo: Arc<dyn TaskStepRepository>,
    /// Project repository (SQLite in production, in-memory for tests)
    pub project_repo: Arc<dyn ProjectRepository>,
    /// API key repository for external API authentication
    pub api_key_repo: Arc<dyn ApiKeyRepository>,
    /// Client-side remote environment registry, pairing, reconciler, and the
    /// active-environment authority for the Rust proxy surface (PR 2.1).
    pub remote_environment_service: Arc<RemoteEnvironmentService>,
    /// Native Atlassian/Jira/Confluence integration service.
    pub atlassian_integration_service: Arc<AtlassianIntegrationService>,
    /// Native Linear integration service.
    pub linear_integration_service: Arc<LinearIntegrationService>,
    /// Native ClickUp integration service.
    pub clickup_integration_service: Arc<ClickUpIntegrationService>,
    /// Native Granola integration service.
    pub granola_integration_service: Arc<GranolaIntegrationService>,
    /// Provider-neutral external issue link and sync service.
    pub external_issue_link_service: Arc<ExternalIssueLinkService>,
    /// Provider status catalog and per-scope presentation service.
    pub ticketing_status_catalog_service: Arc<TicketingStatusCatalogService>,
    /// Agent profile repository (SQLite in production)
    pub agent_profile_repo: Arc<dyn AgentProfileRepository>,
    /// TaskQA repository for QA artifacts
    pub task_qa_repo: Arc<dyn TaskQARepository>,
    /// Review repository for code reviews
    pub review_repo: Arc<dyn ReviewRepository>,
    /// Review settings repository
    pub review_settings_repo: Arc<dyn ReviewSettingsRepository>,
    /// Persisted UI feature flag overrides.
    pub ui_feature_flag_overrides_repo: Arc<dyn UiFeatureFlagOverridesRepository>,
    /// Live authoritative gates for Agent conversation orchestration capabilities.
    pub agent_capability_gate: Arc<AgentCapabilityGate>,
    /// Durable task validation run/result repository
    pub validation_run_repo: Arc<dyn ValidationRunRepository>,
    /// Provider-keyed Workspace Review runtime defaults repository
    pub workspace_review_runtime_settings_repo: Arc<dyn WorkspaceReviewRuntimeSettingsRepository>,
    /// Review issue repository for tracking structured issues from reviews
    pub review_issue_repo: Arc<dyn ReviewIssueRepository>,
    /// Provider-neutral agent clients used by runtime construction and harness routing.
    pub agent_clients: AgentClientBundle,
    /// Global QA settings
    pub qa_settings: Arc<tokio::sync::RwLock<QASettings>>,
    /// Execution settings repository (per-project settings)
    pub execution_settings_repo: Arc<dyn ExecutionSettingsRepository>,
    /// Global execution settings repository (cross-project limits)
    /// Phase 82: Contains global_max_concurrent cap
    pub global_execution_settings_repo: Arc<dyn GlobalExecutionSettingsRepository>,
    /// Ideation session repository
    pub ideation_session_repo: Arc<dyn IdeationSessionRepository>,
    /// Native plan artifact approval repository
    pub plan_approval_repo: Arc<dyn PlanArtifactApprovalRepository>,
    /// Ideation settings repository
    pub ideation_settings_repo: Arc<dyn IdeationSettingsRepository>,
    /// Delegated specialist session repository
    pub delegated_session_repo: Arc<dyn DelegatedSessionRepository>,
    /// Lightweight agent task repository for todo/dependency tracking
    pub agent_task_repo: Arc<dyn AgentTaskRepository>,
    /// Durable scripted Agent workflow state and current-run authority.
    pub agent_workflow_repo: Arc<dyn AgentWorkflowRepository>,
    /// Durable Agent conversation issues surfaced in the Agents UI.
    pub agent_conversation_issue_repo: Arc<dyn AgentConversationIssueRepository>,
    /// Ideation effort settings repository (global and per-project effort overrides)
    pub ideation_effort_settings_repo: Arc<dyn IdeationEffortSettingsRepository>,
    /// Ideation model settings repository (global and per-project model overrides)
    pub ideation_model_settings_repo: Arc<dyn IdeationModelSettingsRepository>,
    /// Provider-neutral lane settings repository for multi-harness routing
    pub agent_lane_settings_repo: Arc<dyn AgentLaneSettingsRepository>,
    /// Exact manual routing-role defaults at global and project scopes.
    pub manual_role_default_repo: Arc<dyn ManualRoleDefaultRepository>,
    /// Provider-native MCP deny/override policy at global and project scopes.
    pub mcp_policy_repo: Arc<dyn McpPolicyRepository>,
    /// Provider/model compatibility and custom model registry
    pub agent_model_registry_repo: Arc<dyn AgentModelRegistryRepository>,
    /// Global enabled/default provider settings
    pub agent_provider_settings_repo: Arc<dyn AgentProviderSettingsRepository>,
    /// Session link repository for managing parent-child session relationships
    pub session_link_repo: Arc<dyn SessionLinkRepository>,
    /// Task proposal repository
    pub task_proposal_repo: Arc<dyn TaskProposalRepository>,
    /// Proposal dependency repository
    pub proposal_dependency_repo: Arc<dyn ProposalDependencyRepository>,
    /// Chat message repository
    pub chat_message_repo: Arc<dyn ChatMessageRepository>,
    /// Normalized visible chat timeline repository
    pub chat_timeline_repo: Arc<dyn ChatTimelineRepository>,
    /// Chat conversation repository (for context-aware chat)
    pub chat_conversation_repo: Arc<dyn ChatConversationRepository>,
    /// Persona repository for persisted agent personas.
    pub persona_repo: Arc<dyn PersonaRepository>,
    /// Conversation-owned branch/worktree repository for Agents starter workspaces
    pub agent_conversation_workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    /// Repair-attempt persistence shares the concrete workspace repository instance.
    pub agent_workspace_repair_repo: Arc<dyn AgentWorkspaceRepairRepository>,
    /// Command-composed publisher callback used only after a repair-owned push has an observed
    /// receipt. The handle is shared with the HTTP AppState graph during runtime wiring.
    pub(crate) agent_workspace_repair_publish_continuation: Arc<
        RwLock<
            Option<
                Arc<dyn crate::application::publish_resilience::AgentWorkspaceRepairPublishContinuation>,
            >,
        >,
    >,
    /// Command-composed PR-fix publisher resumed only after a Workspace Review handoff. The
    /// handle is shared with the HTTP AppState graph during runtime wiring.
    pub(crate) agent_workspace_pr_fix_review_publish_resumer: Arc<
        RwLock<
            Option<
                Arc<
                    dyn crate::application::agent_workspace_pr_supervision_recovery::AgentWorkspacePrFixReviewPublishResumer,
                >,
            >,
        >,
    >,
    /// Conversation-owned primary Jira assignment/cache repository
    pub agent_conversation_jira_issue_repo: Arc<dyn AgentConversationJiraIssueRepository>,
    /// Conversation-owned primary Linear assignment/cache repository
    pub agent_conversation_linear_issue_repo: Arc<dyn AgentConversationLinearIssueRepository>,
    /// Conversation-owned primary Granola note assignment/cache repository
    pub agent_conversation_granola_note_repo: Arc<dyn AgentConversationGranolaNoteRepository>,
    /// Derived attention-state mute repository for agent conversations
    pub agent_conversation_mute_repo: Arc<dyn AgentConversationMuteRepository>,
    /// Remote-initiated conversation start requests (device-queued, backend-claimed)
    pub remote_conversation_start_request_repo: Arc<dyn RemoteConversationStartRequestRepository>,
    /// Remote-initiated agent STOP intents (device-queued, backend-claimed brake)
    pub remote_agent_stop_request_repo: Arc<dyn RemoteAgentStopRequestRepository>,
    /// Per-ticket canonical branch that all conversations for a ticket base off of
    pub ticket_canonical_branch_repo: Arc<dyn TicketCanonicalBranchRepository>,
    /// Startup orphan agent-worktree cleanup backoff markers
    pub orphan_worktree_cleanup_marker_repo: Arc<dyn OrphanWorktreeCleanupMarkerRepository>,
    /// Automation configuration repository
    pub automation_repo: Arc<dyn AutomationRepository>,
    /// Automation run repository
    pub automation_run_repo: Arc<dyn AutomationRunRepository>,
    /// In-memory PTY session manager for Agents conversation terminals
    pub agent_terminal_service: Arc<AgentTerminalService>,
    /// Agent run repository (for tracking Claude agent executions)
    pub agent_run_repo: Arc<dyn AgentRunRepository>,
    /// Activity event repository (for activity stream persistence)
    pub activity_event_repo: Arc<dyn ActivityEventRepository>,
    /// Durable notification history repository.
    pub notification_repo: Arc<dyn NotificationRepository>,
    /// Global desktop and focused-toast notification preferences.
    pub notification_settings_repo: Arc<dyn NotificationSettingsRepository>,
    /// Shared native window-focus signal used by desktop notification delivery.
    pub window_focus_state: Arc<WindowFocusState>,
    /// Shared lazily initialized desktop dispatch service for paired AppStates.
    /// Pre-AppHandle calls intentionally do not populate this cache, so later calls can use Tauri.
    pub(crate) notification_service_cache: Arc<OnceLock<Arc<NotificationService>>>,
    /// Task dependency repository
    pub task_dependency_repo: Arc<dyn TaskDependencyRepository>,
    // Extensibility repositories
    /// Workflow repository for custom workflows
    pub workflow_repo: Arc<dyn WorkflowRepository>,
    /// Artifact repository for artifact management
    pub artifact_repo: Arc<dyn ArtifactRepository>,
    /// Artifact bucket repository for organizing artifacts
    pub artifact_bucket_repo: Arc<dyn ArtifactBucketRepository>,
    /// Artifact flow repository for artifact routing
    pub artifact_flow_repo: Arc<dyn ArtifactFlowRepository>,
    /// Process repository for research processes
    pub process_repo: Arc<dyn ProcessRepository>,
    /// Methodology repository for methodology extensions
    pub methodology_repo: Arc<dyn MethodologyRepository>,
    /// Permission state for UI-based permission approval
    pub permission_state: Arc<PermissionState>,
    /// Question state for inline AskUserQuestion from agents
    pub question_state: Arc<QuestionState>,
    /// Unified message queue for all chat contexts
    pub message_queue: Arc<MessageQueue>,
    /// Durable queued message storage used to hydrate pending queue rows after restart
    pub queued_message_repo: Arc<dyn QueuedMessageRepository>,
    /// Registry for tracking running agent processes
    pub running_agent_registry: Arc<dyn RunningAgentRegistry>,
    /// Plan branch repository for feature branch tracking
    pub plan_branch_repo: Arc<dyn PlanBranchRepository>,
    /// Plan selection stats repository for tracking plan selection interactions
    pub plan_selection_stats_repo: Arc<dyn PlanSelectionStatsRepository>,
    /// App state repository for persisting active_project_id across restarts
    pub app_state_repo: Arc<dyn AppStateRepository>,
    /// Active plan repository for persisting active plan per project
    pub active_plan_repo: Arc<dyn ActivePlanRepository>,
    // Memory framework repositories
    /// Memory entry repository for storing ingested memories
    pub memory_entry_repo: Arc<dyn MemoryEntryRepository>,
    /// Memory event repository for audit trail
    pub memory_event_repo: Arc<dyn MemoryEventRepository>,
    /// Memory archive repository for snapshot generation job queue
    pub memory_archive_repo: Arc<dyn MemoryArchiveRepository>,
    /// Execution plan repository for tracking plan implementation attempts
    pub execution_plan_repo: Arc<dyn ExecutionPlanRepository>,
    /// Chat attachment repository for file uploads in chat
    pub chat_attachment_repo: Arc<dyn ChatAttachmentRepository>,
    /// Live external folders referenced by conversations.
    pub conversation_folder_reference_repo: Arc<dyn ConversationFolderReferenceRepository>,
    /// Storage path for chat attachments
    pub attachment_storage_path: PathBuf,
    /// Streaming state cache for hydrating frontend on navigation to active conversations
    pub streaming_state_cache: crate::application::chat_service::StreamingStateCache,
    /// Interactive process registry for stdin-based multi-turn messaging
    pub interactive_process_registry: Arc<crate::application::InteractiveProcessRegistry>,
    /// Tauri app handle for emitting events to frontend (None in tests)
    pub app_handle: Option<AppHandle>,
    /// Provider-neutral event sink for backend UI/runtime events.
    pub events: Arc<dyn EventSink>,
    /// Shared backend event bus used by the event sink and later internal subscribers.
    pub internal_event_bus: InternalEventBus,
    /// Process-owned app paths resolved once at the shell boundary.
    pub app_paths: AppPaths,
    /// Shared database connection for raw SQL queries (e.g. external_events table).
    /// All accesses MUST go through `db.run(|conn| { ... })` for non-blocking operation.
    pub db: crate::infrastructure::sqlite::DbConnection,
    /// Repository for external_events table — used by TaskTransitionService to dual-emit
    /// state change events for external consumers (poll/SSE endpoints).
    pub external_events_repo: Arc<dyn ExternalEventsRepository>,
    /// GitHub service for PR operations (create, poll, close). None disables PR integration.
    pub github_service: Option<Arc<dyn GithubServiceTrait>>,
    /// Registry of active GitHub PR polling tasks (AD1, AD18).
    pub pr_poller_registry: Arc<PrPollerRegistry>,
    /// Webhook registration repository for managing external webhook subscriptions
    pub webhook_registration_repo: Arc<dyn WebhookRegistrationRepository>,
    /// Optional webhook publisher for pushing events to registered external endpoints.
    /// Constructed ONCE in lib.rs and Arc-cloned into both AppState instances.
    /// None in test constructors.
    pub webhook_publisher:
        Option<Arc<dyn crate::domain::state_machine::services::WebhookPublisher>>,
    /// Shared per-session mutex map for serializing concurrent plan:delivered checks.
    /// ONE Arc, shared between both AppState instances (Tauri IPC + HTTP server) via lib.rs.
    pub session_merge_locks: Arc<dashmap::DashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// Serializes Verify Plan admission per ideation session so concurrent
    /// manual, automatic, and external requests cannot enqueue duplicate turns.
    pub plan_verification_locks: Arc<dashmap::DashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// In-process admission marker spanning the brief interval before a queued
    /// verification action becomes visible through durable queue/run storage.
    pub plan_verification_admissions: Arc<dashmap::DashMap<String, String>>,
    /// Sessions where user has enabled auto-accept for verification. Ephemeral.
    pub auto_accept_sessions: Arc<Mutex<HashSet<String>>>,
    /// Startup Git/GitHub recovery gate. Set when startup defers Git-dependent
    /// work and cleared after an explicit repair resumes that work.
    pub(crate) startup_git_auth_recovery_state: Arc<StartupGitAuthRecoveryState>,
    /// Process-local startup authority shared by Tauri and HTTP AppState graphs.
    pub startup_coordinator: Arc<StartupCoordinator>,
}

impl AppState {
    /// Installs the command-composed normal-publish continuation after the Tauri runtime state is
    /// available. Replacing an existing continuation is intentional during startup retry.
    pub(crate) fn install_agent_workspace_repair_publish_continuation(
        &self,
        continuation: Arc<
            dyn crate::application::publish_resilience::AgentWorkspaceRepairPublishContinuation,
        >,
    ) {
        *self
            .agent_workspace_repair_publish_continuation
            .write()
            .expect("repair publish continuation lock") = Some(continuation);
    }

    /// Returns the command-composed publisher only at the durable post-push boundary. Missing
    /// runtime composition fails closed before a PR effect receipt is recorded.
    pub(crate) fn agent_workspace_repair_publish_continuation(
        &self,
    ) -> AppResult<
        Arc<dyn crate::application::publish_resilience::AgentWorkspaceRepairPublishContinuation>,
    > {
        self.agent_workspace_repair_publish_continuation
            .read()
            .expect("repair publish continuation lock")
            .clone()
            .ok_or_else(|| {
                AppError::Infrastructure(
                    "workspace repair publish continuation is unavailable in this runtime"
                        .to_string(),
                )
            })
    }

    /// Installs the command-composed Workspace Review PR-fix resumer after the runtime is
    /// available. Replacing an existing resumer is intentional during startup retry.
    pub(crate) fn install_agent_workspace_pr_fix_review_publish_resumer(
        &self,
        resumer: Arc<
            dyn crate::application::agent_workspace_pr_supervision_recovery::AgentWorkspacePrFixReviewPublishResumer,
        >,
    ) {
        *self
            .agent_workspace_pr_fix_review_publish_resumer
            .write()
            .expect("PR-fix review publish resumer lock") = Some(resumer);
    }

    /// Returns the command-composed resumer only at the Workspace Review handoff boundary.
    /// Missing runtime composition fails closed before recovery can publish a PR fix.
    #[cfg(test)]
    pub(crate) fn agent_workspace_pr_fix_review_publish_resumer(
        &self,
    ) -> AppResult<
        Arc<
            dyn crate::application::agent_workspace_pr_supervision_recovery::AgentWorkspacePrFixReviewPublishResumer,
        >,
    >{
        self.agent_workspace_pr_fix_review_publish_resumer
            .read()
            .expect("PR-fix review publish resumer lock")
            .clone()
            .ok_or_else(|| {
                AppError::Infrastructure(
                    "PR-fix review publish resumer is unavailable in this runtime".to_string(),
                )
            })
    }

    fn sqlite_agent_workspace_repositories(
        shared_conn: &Arc<Mutex<rusqlite::Connection>>,
    ) -> (
        Arc<dyn AgentConversationWorkspaceRepository>,
        Arc<dyn AgentWorkspaceRepairRepository>,
    ) {
        let repository = Arc::new(SqliteAgentConversationWorkspaceRepository::from_shared(
            Arc::clone(shared_conn),
        ));
        let workspace_repository: Arc<dyn AgentConversationWorkspaceRepository> =
            repository.clone();
        let repair_repository: Arc<dyn AgentWorkspaceRepairRepository> = repository;
        (workspace_repository, repair_repository)
    }

    fn memory_agent_workspace_repositories() -> (
        Arc<dyn AgentConversationWorkspaceRepository>,
        Arc<dyn AgentWorkspaceRepairRepository>,
    ) {
        let repository = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let workspace_repository: Arc<dyn AgentConversationWorkspaceRepository> =
            repository.clone();
        let repair_repository: Arc<dyn AgentWorkspaceRepairRepository> = repository;
        (workspace_repository, repair_repository)
    }

    pub(crate) fn manual_role_default_service(
        &self,
    ) -> crate::application::manual_role_default_service::ManualRoleDefaultService {
        crate::application::manual_role_default_service::ManualRoleDefaultService::new(
            Arc::clone(&self.manual_role_default_repo),
            Arc::clone(&self.agent_lane_settings_repo),
            Arc::clone(&self.agent_provider_settings_repo),
            Arc::clone(&self.persona_repo),
            Arc::clone(&self.agent_capability_gate),
            crate::infrastructure::agents::agent_personas_enabled(),
            self.app_paths.global_router_path(),
        )
    }

    pub(crate) fn mcp_policy_service(
        &self,
    ) -> crate::application::mcp_policy_service::McpPolicyService {
        crate::application::mcp_policy_service::McpPolicyService::new(
            Arc::clone(&self.mcp_policy_repo),
            self.app_paths.global_mcp_policy_path(),
        )
        .with_provider_settings_repo(Arc::clone(&self.agent_provider_settings_repo))
    }

    pub(crate) async fn resolve_effective_manual_role_default(
        &self,
        project_id: Option<&str>,
        project_root: Option<&std::path::Path>,
        role: RoutingRole,
    ) -> AppResult<crate::application::manual_role_default_service::ResolvedManualRoleDefault> {
        use crate::application::manual_role_default_service::ManualDefaultSource;

        let mut resolved = self
            .manual_role_default_service()
            .resolve(project_id, project_root, role)
            .await?;
        if role != RoutingRole::WorkspaceReviewer
            || resolved.source != ManualDefaultSource::ProviderDefault
        {
            return Ok(resolved);
        }

        let Some(settings) = self
            .resolve_explicit_workspace_review_runtime_settings(project_id, resolved.value.harness)
            .await?
        else {
            return Ok(resolved);
        };

        resolved.value.model = settings.model;
        resolved.value.effort = settings.effort;
        resolved.source = ManualDefaultSource::LegacyWorkspaceReview;
        Ok(resolved)
    }

    pub fn agent_workflow_runner(
        &self,
    ) -> AppResult<crate::application::agent_workflow_runner::AgentWorkflowRunner> {
        Ok(
            crate::application::agent_workflow_runner::AgentWorkflowRunner::new(
                Arc::clone(&self.agent_workflow_repo),
                Arc::clone(&self.agent_capability_gate),
                self.app_paths.workflow_runner_path()?,
                self.app_paths.workflow_runtime_dir(),
            ),
        )
    }

    fn memory_agent_workflow_repo() -> Arc<dyn AgentWorkflowRepository> {
        let conn = open_connection(&std::path::PathBuf::from(":memory:"))
            .expect("Failed to open workflow test database");
        run_migrations(&conn).expect("Failed to migrate workflow test database");
        conn.execute("PRAGMA foreign_keys = OFF", [])
            .expect("Failed to configure workflow test database");
        Arc::new(SqliteAgentWorkflowRepository::new(conn))
    }
    /// Returns this AppState's shared notification service when an AppHandle is available.
    /// A pre-AppHandle call returns a transient Noop-backed service and is never cached.
    pub fn notification_service(&self) -> Arc<NotificationService> {
        if let Some(service) = self.notification_service_cache.get() {
            return Arc::clone(service);
        }

        let Some(app_handle) = self.app_handle.as_ref() else {
            return Arc::new(self.build_notification_service(None));
        };

        Arc::clone(
            self.notification_service_cache.get_or_init(|| {
                Arc::new(self.build_notification_service(Some(app_handle.clone())))
            }),
        )
    }

    fn build_notification_service(&self, app_handle: Option<AppHandle>) -> NotificationService {
        let emitter: Arc<dyn NotificationEventEmitter> = match app_handle.as_ref() {
            Some(app_handle) => Arc::new(TauriNotificationEventEmitter::new(app_handle.clone())),
            None => Arc::new(NoopNotificationEventEmitter),
        };
        let desktop_notifier: Arc<dyn crate::application::notification_service::DesktopNotifier> =
            match app_handle {
                Some(app_handle) => Arc::new(TauriDesktopNotifier::new(app_handle)),
                None => Arc::new(NoopDesktopNotifier),
            };
        NotificationService::new_with_desktop_dispatch(
            Arc::clone(&self.notification_repo),
            emitter,
            Arc::clone(&self.notification_settings_repo),
            Arc::clone(&self.window_focus_state),
            desktop_notifier,
            std::time::Duration::from_secs(
                crate::infrastructure::agents::claude::stream_timeouts()
                    .desktop_notification_coalesce_window_secs,
            ),
            Some(Arc::clone(&self.project_repo)),
        )
    }

    #[cfg(test)]
    pub(crate) fn install_notification_service_for_test(&self, service: Arc<NotificationService>) {
        assert!(
            self.notification_service_cache.set(service).is_ok(),
            "test notification service cache must be empty"
        );
    }

    #[cfg(test)]
    pub(crate) fn has_cached_notification_service_for_test(&self) -> bool {
        self.notification_service_cache.get().is_some()
    }

    fn null_event_runtime() -> (Arc<dyn EventSink>, InternalEventBus) {
        (Arc::new(NullEventSink), InternalEventBus::new())
    }

    fn production_agent_clients(
        mcp_policy_repo: Arc<dyn McpPolicyRepository>,
        project_repo: Arc<dyn ProjectRepository>,
        provider_settings_repo: Arc<dyn AgentProviderSettingsRepository>,
        global_mcp_policy_path: PathBuf,
    ) -> AgentClientBundle {
        let base = AgentClientBundle::standard_production_runtime_clients();
        let policy_service = crate::application::mcp_policy_service::McpPolicyService::new(
            mcp_policy_repo,
            global_mcp_policy_path,
        )
        .with_provider_settings_repo(provider_settings_repo);
        let wrap = |harness, client| {
            Arc::new(
                crate::application::mcp_policy_agent_client::McpPolicyAgentClient::new(
                    harness,
                    client,
                    policy_service.clone(),
                    Arc::clone(&project_repo),
                ),
            ) as Arc<dyn AgenticClient>
        };
        let default_client = wrap(base.default_harness, Arc::clone(&base.default_client));
        let harness_clients = base
            .iter_explicit_harness_clients()
            .map(|(harness, client)| (harness, wrap(harness, client)))
            .collect();
        AgentClientBundle::from_parts(base.default_harness, default_client, harness_clients)
    }

    fn mock_agent_clients() -> AgentClientBundle {
        AgentClientBundle::standard_mock_runtime_clients()
    }

    fn production_remote_environment_service(
        shared_conn: &Arc<Mutex<rusqlite::Connection>>,
        app_handle: &AppHandle,
    ) -> Arc<RemoteEnvironmentService> {
        let host_client: Arc<dyn crate::infrastructure::RemoteHostClient> =
            match crate::infrastructure::HyperRemoteHostClient::new() {
                Ok(client) => Arc::new(client),
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        "Remote host HTTP client unavailable; pairing will fail until TLS roots are available"
                    );
                    Arc::new(crate::infrastructure::UnavailableRemoteHostClient::new(
                        error,
                    ))
                }
            };
        // Constructing the relay spawns nothing (rule 17): sessions are spawned from
        // the async `connect` path only. Frames land on the webview's local bus.
        let relay = Arc::new(crate::application::RemoteEventRelay::new(
            Arc::new(crate::infrastructure::TungsteniteRemoteWsClient::new()),
            Arc::new(crate::application::TauriFrameSink(app_handle.clone())),
        ));
        Arc::new(RemoteEnvironmentService::new(
            Arc::new(
                crate::infrastructure::sqlite::SqliteRemoteEnvironmentRepository::from_shared(
                    Arc::clone(shared_conn),
                ),
            ),
            Arc::new(MacosKeychainSecretStore::new()),
            host_client,
            relay,
        ))
    }

    fn memory_remote_environment_service() -> Arc<RemoteEnvironmentService> {
        Arc::new(RemoteEnvironmentService::new(
            Arc::new(crate::infrastructure::memory::MemoryRemoteEnvironmentRepository::new()),
            Arc::new(MemorySecretStore::new()),
            Arc::new(crate::infrastructure::UnavailableRemoteHostClient::new(
                "remote host client is not wired in tests",
            )),
            Arc::new(crate::application::RemoteEventRelay::new(
                Arc::new(crate::infrastructure::remote_ws_client::MockRemoteWsClient::new()),
                Arc::new(crate::application::NoopFrameSink),
            )),
        ))
    }

    fn production_atlassian_integration_service(
        shared_conn: &Arc<Mutex<rusqlite::Connection>>,
    ) -> Arc<AtlassianIntegrationService> {
        let client: Arc<dyn crate::application::AtlassianApiClient> =
            match HyperAtlassianApiClient::new() {
                Ok(client) => Arc::new(client),
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        "Atlassian HTTP client unavailable; integration validation will fail until TLS roots are available"
                    );
                    Arc::new(UnavailableAtlassianApiClient::new(error))
                }
            };
        Arc::new(AtlassianIntegrationService::new(
            Arc::new(SqliteAtlassianIntegrationSettingsRepository::from_shared(
                Arc::clone(shared_conn),
            )),
            Arc::new(MacosKeychainSecretStore::new()),
            client,
        ))
    }

    fn memory_atlassian_integration_service() -> Arc<AtlassianIntegrationService> {
        Arc::new(AtlassianIntegrationService::new(
            Arc::new(MemoryAtlassianIntegrationSettingsRepository::new()),
            Arc::new(MemorySecretStore::new()),
            Arc::new(EmptyAtlassianApiClient),
        ))
    }

    fn production_linear_integration_service(
        shared_conn: &Arc<Mutex<rusqlite::Connection>>,
    ) -> Arc<LinearIntegrationService> {
        let client: Arc<dyn crate::application::LinearApiClient> = match HyperLinearApiClient::new()
        {
            Ok(client) => Arc::new(client),
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    "Linear HTTP client unavailable; integration validation will fail until TLS roots are available"
                );
                Arc::new(UnavailableLinearApiClient::new(error))
            }
        };
        Arc::new(LinearIntegrationService::new(
            Arc::new(SqliteLinearIntegrationSettingsRepository::from_shared(
                Arc::clone(shared_conn),
            )),
            Arc::new(MacosKeychainSecretStore::new()),
            client,
        ))
    }

    fn memory_linear_integration_service() -> Arc<LinearIntegrationService> {
        Arc::new(LinearIntegrationService::new(
            Arc::new(MemoryLinearIntegrationSettingsRepository::new()),
            Arc::new(MemorySecretStore::new()),
            Arc::new(EmptyLinearApiClient),
        ))
    }

    fn production_clickup_integration_service(
        shared_conn: &Arc<Mutex<rusqlite::Connection>>,
    ) -> Arc<ClickUpIntegrationService> {
        let client: Arc<dyn crate::application::ClickUpApiClient> = match HyperClickUpApiClient::new(
        ) {
            Ok(client) => Arc::new(client),
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    "ClickUp HTTP client unavailable; integration validation will fail until TLS roots are available"
                );
                Arc::new(UnavailableClickUpApiClient::new(error))
            }
        };
        Arc::new(ClickUpIntegrationService::new(
            Arc::new(SqliteClickUpIntegrationSettingsRepository::from_shared(
                Arc::clone(shared_conn),
            )),
            Arc::new(MacosKeychainSecretStore::new()),
            client,
        ))
    }

    fn memory_clickup_integration_service() -> Arc<ClickUpIntegrationService> {
        Arc::new(ClickUpIntegrationService::new(
            Arc::new(MemoryClickUpIntegrationSettingsRepository::new()),
            Arc::new(MemorySecretStore::new()),
            Arc::new(EmptyClickUpApiClient),
        ))
    }

    fn production_granola_integration_service(
        shared_conn: &Arc<Mutex<rusqlite::Connection>>,
    ) -> Arc<GranolaIntegrationService> {
        Arc::new(GranolaIntegrationService::new(
            Arc::new(SqliteGranolaIntegrationSettingsRepository::from_shared(
                Arc::clone(shared_conn),
            )),
            Arc::new(MacosKeychainSecretStore::new()),
            Self::production_granola_api_client(),
        ))
    }

    fn production_granola_api_client() -> Arc<dyn crate::application::GranolaApiClient> {
        match HyperGranolaApiClient::new() {
            Ok(client) => Arc::new(client),
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    "Granola HTTP client unavailable; integration validation will fail until TLS roots are available"
                );
                Arc::new(UnavailableGranolaApiClient::new(error))
            }
        }
    }

    fn memory_granola_integration_service() -> Arc<GranolaIntegrationService> {
        Arc::new(GranolaIntegrationService::new(
            Arc::new(MemoryGranolaIntegrationSettingsRepository::new()),
            Arc::new(MemorySecretStore::new()),
            Arc::new(EmptyGranolaApiClient),
        ))
    }

    fn production_external_issue_link_service(
        shared_conn: &Arc<Mutex<rusqlite::Connection>>,
    ) -> Arc<ExternalIssueLinkService> {
        Arc::new(ExternalIssueLinkService::new(Arc::new(
            SqliteExternalIssueLinkRepository::from_shared(Arc::clone(shared_conn)),
        )))
    }

    fn memory_external_issue_link_service() -> Arc<ExternalIssueLinkService> {
        Arc::new(ExternalIssueLinkService::new(Arc::new(
            MemoryExternalIssueLinkRepository::new(),
        )))
    }

    fn production_ticketing_status_catalog_service(
        shared_conn: &Arc<Mutex<rusqlite::Connection>>,
    ) -> Arc<TicketingStatusCatalogService> {
        Arc::new(TicketingStatusCatalogService::new(Arc::new(
            SqliteTicketingStatusCatalogRepository::from_shared(Arc::clone(shared_conn)),
        )))
    }

    fn memory_ticketing_status_catalog_service() -> Arc<TicketingStatusCatalogService> {
        Arc::new(TicketingStatusCatalogService::new(Arc::new(
            MemoryTicketingStatusCatalogRepository::new(),
        )))
    }

    fn enable_claude_test_mode() {
        std::env::set_var("RALPHX_TEST_MODE", "1");
    }

    fn background_agent_runtime_for_harness(
        &self,
        client: Arc<dyn AgenticClient>,
        harness: AgentHarnessKind,
        model: Option<String>,
        cli_path_override: Option<PathBuf>,
        logical_effort: Option<LogicalEffort>,
        approval_policy: Option<String>,
        sandbox_mode: Option<String>,
        service_tier: Option<String>,
        env: HashMap<String, String>,
    ) -> ResolvedBackgroundAgentRuntime {
        ResolvedBackgroundAgentRuntime {
            client,
            harness: Some(harness),
            model,
            cli_path_override,
            logical_effort,
            approval_policy: approval_policy
                .or_else(|| default_approval_policy_for_harness(harness).map(str::to_string)),
            sandbox_mode: sandbox_mode
                .or_else(|| default_sandbox_mode_for_harness(harness).map(str::to_string)),
            service_tier,
            env,
        }
    }

    fn apply_workspace_review_runtime_settings(
        runtime: ResolvedBackgroundAgentRuntime,
        settings: WorkspaceReviewRuntimeSettings,
    ) -> ResolvedBackgroundAgentRuntime {
        ResolvedBackgroundAgentRuntime {
            model: settings.model,
            logical_effort: settings.effort,
            ..runtime
        }
    }

    async fn resolve_explicit_workspace_review_runtime_settings(
        &self,
        project_id: Option<&str>,
        provider: AgentHarnessKind,
    ) -> AppResult<Option<WorkspaceReviewRuntimeSettings>> {
        let global_row = self
            .workspace_review_runtime_settings_repo
            .get_global(provider)
            .await
            .map_err(|error| AppError::Infrastructure(error.to_string()))?;
        let project_row = if let Some(project_id) = project_id {
            self.workspace_review_runtime_settings_repo
                .get_for_project(project_id, provider)
                .await
                .map_err(|error| AppError::Infrastructure(error.to_string()))?
        } else {
            None
        };

        if global_row.is_none() && project_row.is_none() {
            return Ok(None);
        }

        Ok(Some(WorkspaceReviewRuntimeSettings::resolve_effective(
            provider,
            global_row.as_ref().map(|row| &row.settings),
            project_row.as_ref().map(|row| &row.settings),
        )))
    }

    pub(crate) fn managed_cli_path_override_for_provider(
        provider_settings: &AgentProviderSettings,
        purpose: &str,
    ) -> AppResult<Option<PathBuf>> {
        let Some(launch_path) =
            crate::application::managed_provider_cli::checked_managed_provider_cli_launch_path(
                provider_settings,
                purpose,
            )
        else {
            return Ok(None);
        };

        launch_path.map(Some).map_err(AppError::Infrastructure)
    }

    async fn resolve_background_agent_client_and_cli_path_override(
        &self,
        harness: AgentHarnessKind,
        purpose: &str,
        provider_settings: &AgentProviderSettings,
    ) -> AppResult<(Arc<dyn AgenticClient>, Option<PathBuf>)> {
        let cli_path_override =
            Self::managed_cli_path_override_for_provider(provider_settings, purpose)?;

        if harness == self.agent_clients.default_harness {
            return Ok((
                Arc::clone(&self.agent_clients.default_client),
                cli_path_override,
            ));
        }

        if cli_path_override.is_some() {
            if let Some(client) = self.agent_clients.explicit_harness_client(harness) {
                return Ok((client, cli_path_override));
            }
        } else if let Some(client) = self
            .agent_clients
            .explicit_available_harness_client(harness)
            .await
        {
            return Ok((client, None));
        }

        Err(AppError::Infrastructure(format!(
            "{purpose} harness unavailable: {harness}"
        )))
    }

    pub(crate) async fn resolve_background_agent_runtime_for_harness(
        &self,
        harness: AgentHarnessKind,
        purpose: &str,
    ) -> AppResult<ResolvedBackgroundAgentRuntime> {
        crate::application::ensure_provider_spawn_enabled(
            &self.agent_provider_settings_repo,
            harness,
            purpose,
        )
        .await
        .map_err(AppError::Infrastructure)?;
        let provider_settings = self
            .agent_provider_settings_repo
            .get(harness)
            .await
            .map_err(|error| AppError::Infrastructure(error.to_string()))?
            .unwrap_or_else(|| {
                crate::domain::agents::AgentProviderSettings::disabled_defaults(harness)
            });
        let (client, cli_path_override) = self
            .resolve_background_agent_client_and_cli_path_override(
                harness,
                purpose,
                &provider_settings,
            )
            .await?;
        let provider_env = crate::application::provider_env_file::load_provider_custom_env_file(
            &provider_settings,
        )
        .map_err(AppError::Infrastructure)?;

        Ok(self.background_agent_runtime_for_harness(
            client,
            harness,
            provider_settings.model,
            cli_path_override,
            provider_settings.effort,
            provider_settings.approval_policy,
            provider_settings.sandbox_mode,
            provider_settings.service_tier,
            provider_env,
        ))
    }

    pub(crate) async fn resolve_manual_role_background_agent_runtime(
        &self,
        project_id: Option<&str>,
        project_root: Option<&std::path::Path>,
        role: RoutingRole,
        runtime_override: Option<&crate::domain::agents::ManualRoleRuntimeOverride>,
        agent_name: &str,
        purpose: &str,
        harness_override: Option<AgentHarnessKind>,
    ) -> AppResult<ResolvedBackgroundAgentRuntime> {
        let defaults = self.manual_role_default_service();
        let resolved =
            crate::application::agent_lane_resolution::resolve_manual_role_spawn_settings(
                agent_name,
                project_id,
                project_root,
                role,
                runtime_override,
                harness_override,
                None,
                &defaults,
            )
            .await?;
        crate::application::ensure_provider_spawn_enabled(
            &self.agent_provider_settings_repo,
            resolved.effective_harness,
            purpose,
        )
        .await
        .map_err(AppError::Infrastructure)?;
        let provider_settings = self
            .agent_provider_settings_repo
            .get(resolved.effective_harness)
            .await
            .map_err(|error| AppError::Infrastructure(error.to_string()))?
            .unwrap_or_else(|| {
                AgentProviderSettings::disabled_defaults(resolved.effective_harness)
            });
        let (client, cli_path_override) = self
            .resolve_background_agent_client_and_cli_path_override(
                resolved.effective_harness,
                purpose,
                &provider_settings,
            )
            .await?;
        let provider_env = crate::application::provider_env_file::load_provider_custom_env_file(
            &provider_settings,
        )
        .map_err(AppError::Infrastructure)?;

        Ok(self.background_agent_runtime_for_harness(
            client,
            resolved.effective_harness,
            Some(resolved.model),
            cli_path_override,
            resolved.logical_effort,
            resolved.approval_policy,
            resolved.sandbox_mode,
            resolved.service_tier.or(provider_settings.service_tier),
            provider_env,
        ))
    }

    pub fn build_chat_service(&self) -> AppChatService {
        self.build_chat_service_for_runtime(None, self.app_handle.clone())
    }

    /// Build chat service with the app-managed execution halt state when available.
    pub fn build_chat_service_with_managed_execution_state(&self) -> AppChatService {
        let execution_state = self
            .app_handle
            .as_ref()
            .and_then(|handle| handle.try_state::<Arc<ExecutionState>>())
            .map(|state| state.inner().clone());
        self.build_chat_service_for_runtime(execution_state, self.app_handle.clone())
    }

    pub fn build_chat_service_for_runtime<R: Runtime>(
        &self,
        execution_state: Option<Arc<ExecutionState>>,
        app_handle: Option<AppHandle<R>>,
    ) -> AppChatService<R> {
        let deps = ChatRuntimeFactoryDeps::from_app_state(self);

        build_chat_service_from_deps(app_handle, execution_state, &deps)
    }

    pub fn build_chat_service_with_execution_state(
        &self,
        execution_state: Arc<ExecutionState>,
    ) -> AppChatService {
        self.build_chat_service_for_runtime(Some(execution_state), self.app_handle.clone())
    }

    pub fn build_transition_service_with_execution_state(
        &self,
        execution_state: Arc<ExecutionState>,
    ) -> TaskTransitionService {
        self.build_transition_service_for_runtime(execution_state, self.app_handle.clone())
    }

    pub fn build_transition_service_for_runtime(
        &self,
        execution_state: Arc<ExecutionState>,
        app_handle: Option<AppHandle>,
    ) -> TaskTransitionService {
        let started_at = Instant::now();
        let deps = RuntimeFactoryDeps::from_app_state(self);
        tracing::info!(
            elapsed_ms = started_at.elapsed().as_millis(),
            "AppState transition service deps built"
        );

        let started_at = Instant::now();
        let mut service = build_transition_service_from_deps(app_handle, execution_state, &deps)
            .with_event_sink(Arc::clone(&self.events))
            .with_tasks_feature_settings_repo(Arc::clone(&self.ideation_settings_repo))
            .with_notifier(Arc::new(
                crate::application::task_notification_producer::TaskPipelineNotificationProducer::new(
                    self.notification_service(),
                ),
            ));
        tracing::info!(
            elapsed_ms = started_at.elapsed().as_millis(),
            "AppState transition service built"
        );

        let drafter =
            crate::application::plan_pr_description::build_app_state_plan_pr_description_drafter(
                Arc::clone(&self.agent_conversation_workspace_repo),
                Arc::clone(&self.chat_conversation_repo),
                Arc::clone(&self.agent_provider_settings_repo),
                Arc::new(self.manual_role_default_service()),
                self.agent_clients.clone(),
            );
        service = service.with_plan_pr_description_drafter(drafter);

        service
    }

    pub(crate) fn build_tasks_feature_toggle_service(
        &self,
        execution_state: Arc<ExecutionState>,
        app_handle: Option<AppHandle>,
    ) -> TasksFeatureToggleService<'_> {
        let transition_service = self
            .build_transition_service_for_runtime(Arc::clone(&execution_state), app_handle.clone());
        let cleanup = TaskCleanupService::new(
            Arc::clone(&self.task_repo),
            Arc::clone(&self.project_repo),
            Arc::clone(&self.running_agent_registry),
            app_handle.clone(),
        )
        .with_interactive_process_registry(Arc::clone(&self.interactive_process_registry));
        TasksFeatureToggleService::new(self, transition_service, cleanup, app_handle)
    }

    #[cfg(test)]
    pub(crate) fn build_tasks_feature_toggle_service_for_test(
        &self,
    ) -> TasksFeatureToggleService<'_> {
        self.build_tasks_feature_toggle_service(Arc::new(ExecutionState::new()), None)
    }

    #[cfg(test)]
    pub(crate) fn build_transition_service_for_test_runtime(&self) -> TaskTransitionService {
        self.build_transition_service_for_runtime(Arc::new(ExecutionState::new()), None)
    }

    pub fn build_task_scheduler_for_runtime(
        &self,
        execution_state: Arc<ExecutionState>,
        app_handle: Option<AppHandle>,
    ) -> TaskSchedulerService {
        let deps = RuntimeFactoryDeps::from_app_state(self);

        build_task_scheduler_from_deps(app_handle, execution_state, &deps)
    }

    pub(crate) async fn resolve_workspace_role_runtime_for_project(
        &self,
        project_id: &str,
        role: RoutingRole,
        agent_name: &str,
        purpose: &str,
    ) -> AppResult<ResolvedBackgroundAgentRuntime> {
        self.resolve_workspace_role_runtime_for_project_with_override(
            project_id, role, None, agent_name, purpose,
        )
        .await
    }

    pub(crate) async fn resolve_workspace_role_runtime_for_project_with_override(
        &self,
        project_id: &str,
        role: RoutingRole,
        runtime_override: Option<&crate::domain::agents::ManualRoleRuntimeOverride>,
        agent_name: &str,
        purpose: &str,
    ) -> AppResult<ResolvedBackgroundAgentRuntime> {
        let project = self
            .project_repo
            .get_by_id(&ProjectId::from_string(project_id.to_string()))
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Project not found: {project_id}")))?;
        self.resolve_manual_role_background_agent_runtime(
            Some(project_id),
            Some(std::path::Path::new(&project.working_directory)),
            role,
            runtime_override,
            agent_name,
            purpose,
            None,
        )
        .await
    }

    pub(crate) async fn resolve_workspace_reviewer_runtime_for_project(
        &self,
        project_id: &str,
    ) -> AppResult<ResolvedBackgroundAgentRuntime> {
        let project = self
            .project_repo
            .get_by_id(&ProjectId::from_string(project_id.to_string()))
            .await?;
        let project_root = project
            .as_ref()
            .map(|project| std::path::Path::new(&project.working_directory));
        let role_default = self
            .resolve_effective_manual_role_default(
                Some(project_id),
                project_root,
                RoutingRole::WorkspaceReviewer,
            )
            .await?;
        use crate::application::manual_role_default_service::ManualDefaultSource;
        if !matches!(
            role_default.source,
            ManualDefaultSource::ProviderDefault | ManualDefaultSource::LegacyWorkspaceReview
        ) {
            return self
                .resolve_workspace_role_runtime_for_project(
                    project_id,
                    RoutingRole::WorkspaceReviewer,
                    crate::infrastructure::agents::claude::agent_names::AGENT_WORKSPACE_REVIEWER,
                    "workspace reviewer provider",
                )
                .await;
        }
        let provider = role_default.value.harness;
        let runtime = self
            .resolve_background_agent_runtime_for_harness(provider, "workspace reviewer provider")
            .await?;
        if role_default.source == ManualDefaultSource::ProviderDefault {
            return Ok(runtime);
        }

        Ok(Self::apply_workspace_review_runtime_settings(
            runtime,
            WorkspaceReviewRuntimeSettings {
                model: role_default.value.model,
                effort: role_default.value.effort,
            },
        ))
    }

    /// Create AppState for production use with SQLite repositories.
    /// Opens the database at the default path and runs migrations.
    pub fn new_production(app_handle: AppHandle) -> AppResult<Self> {
        let app_paths = AppPaths::from_app_handle(&app_handle)?;
        let (events, internal_event_bus) = Self::null_event_runtime();
        Self::new_production_with_paths_and_events(
            app_handle,
            app_paths,
            events,
            internal_event_bus,
        )
    }

    pub fn new_production_with_paths_and_events(
        app_handle: AppHandle,
        app_paths: AppPaths,
        events: Arc<dyn EventSink>,
        internal_event_bus: InternalEventBus,
    ) -> AppResult<Self> {
        Self::new_production_with_paths_events_and_migration_observer(
            app_handle,
            app_paths,
            events,
            internal_event_bus,
            |_| {},
        )
    }

    /// Constructs production AppState while reporting real migration units.
    ///
    /// # Errors
    ///
    /// Returns database, migration, path, or repository-wiring failures without
    /// publishing AppState readiness.
    pub fn new_production_with_paths_events_and_migration_observer(
        app_handle: AppHandle,
        app_paths: AppPaths,
        events: Arc<dyn EventSink>,
        internal_event_bus: InternalEventBus,
        observer: impl FnMut(MigrationProgress),
    ) -> AppResult<Self> {
        let path = app_paths.database_path()?;
        let conn = open_connection(&path)?;
        run_migrations_with_observer(&conn, observer)?;
        let remove_inherited_github_cli_tokens = conn
            .query_row(
                "SELECT remove_inherited_github_cli_tokens FROM app_state WHERE id = 1",
                [],
                |row| row.get::<_, bool>(0),
            )
            .map_err(|error| AppError::Database(error.to_string()))?;
        crate::infrastructure::subprocess_env_policy::set_remove_inherited_github_cli_tokens(
            remove_inherited_github_cli_tokens,
        );

        let shared_conn = Arc::new(Mutex::new(conn));
        Self::build_from_shared_conn(
            app_handle,
            shared_conn,
            app_paths,
            events,
            internal_event_bus,
        )
    }

    /// Create AppState sharing an existing DB connection (no new connection or migrations).
    /// Used by the HTTP/MCP server to share the Tauri AppState's physical SQLite connection.
    pub fn new_production_shared(
        app_handle: AppHandle,
        shared_conn: Arc<Mutex<rusqlite::Connection>>,
    ) -> AppResult<Self> {
        let app_paths = AppPaths::from_app_handle(&app_handle)?;
        let (events, internal_event_bus) = Self::null_event_runtime();
        Self::new_production_shared_with_paths_and_events(
            app_handle,
            shared_conn,
            app_paths,
            events,
            internal_event_bus,
        )
    }

    pub fn new_production_shared_with_paths_and_events(
        app_handle: AppHandle,
        shared_conn: Arc<Mutex<rusqlite::Connection>>,
        app_paths: AppPaths,
        events: Arc<dyn EventSink>,
        internal_event_bus: InternalEventBus,
    ) -> AppResult<Self> {
        Self::build_from_shared_conn(
            app_handle,
            shared_conn,
            app_paths,
            events,
            internal_event_bus,
        )
    }

    /// Internal helper: build all SQLite repositories from a pre-existing shared connection.
    fn build_from_shared_conn(
        app_handle: AppHandle,
        shared_conn: Arc<Mutex<rusqlite::Connection>>,
        app_paths: AppPaths,
        events: Arc<dyn EventSink>,
        internal_event_bus: InternalEventBus,
    ) -> AppResult<Self> {
        // Create repositories that are used by services
        let task_repo: Arc<dyn TaskRepository> = Arc::new(
            SqliteTaskRepository::from_shared(Arc::clone(&shared_conn)).with_tasks_feature_policy(),
        );
        let project_repo: Arc<dyn ProjectRepository> = Arc::new(
            SqliteProjectRepository::from_shared(Arc::clone(&shared_conn)),
        );
        let mcp_policy_repo: Arc<dyn McpPolicyRepository> = Arc::new(
            SqliteMcpPolicyRepository::from_shared(Arc::clone(&shared_conn)),
        );
        let agent_provider_settings_repo: Arc<dyn AgentProviderSettingsRepository> = Arc::new(
            SqliteAgentProviderSettingsRepository::from_shared(Arc::clone(&shared_conn)),
        );
        let task_proposal_repo: Arc<dyn TaskProposalRepository> = Arc::new(
            SqliteTaskProposalRepository::from_shared(Arc::clone(&shared_conn)),
        );
        let artifact_repo: Arc<dyn ArtifactRepository> = Arc::new(
            SqliteArtifactRepository::from_shared(Arc::clone(&shared_conn)),
        );

        // Chat attachment repository
        let chat_attachment_repo: Arc<dyn ChatAttachmentRepository> = Arc::new(
            SqliteChatAttachmentRepository::from_shared(Arc::clone(&shared_conn)),
        );
        let attachment_storage_path = app_paths.attachment_storage_path();

        let gh_svc: Arc<dyn GithubServiceTrait> = Arc::new(GhCliGithubService::new());
        let (agent_conversation_workspace_repo, agent_workspace_repair_repo) =
            Self::sqlite_agent_workspace_repositories(&shared_conn);

        let state = Self {
            task_repo: Arc::clone(&task_repo),
            branch_update_repo: Arc::new(
                SqliteBranchUpdateRepository::from_shared(Arc::clone(&shared_conn))
                    .with_tasks_feature_policy(),
            ),
            task_step_repo: Arc::new(
                SqliteTaskStepRepository::from_shared(Arc::clone(&shared_conn))
                    .with_tasks_feature_policy(),
            ),
            project_repo: Arc::clone(&project_repo),
            api_key_repo: Arc::new(SqliteApiKeyRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            remote_environment_service: Self::production_remote_environment_service(
                &shared_conn,
                &app_handle,
            ),
            atlassian_integration_service: Self::production_atlassian_integration_service(
                &shared_conn,
            ),
            linear_integration_service: Self::production_linear_integration_service(&shared_conn),
            clickup_integration_service: Self::production_clickup_integration_service(&shared_conn),
            granola_integration_service: Self::production_granola_integration_service(&shared_conn),
            external_issue_link_service: Self::production_external_issue_link_service(&shared_conn),
            ticketing_status_catalog_service: Self::production_ticketing_status_catalog_service(
                &shared_conn,
            ),
            agent_profile_repo: Arc::new(SqliteAgentProfileRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            task_qa_repo: Arc::new(SqliteTaskQARepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            review_repo: Arc::new(SqliteReviewRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            review_settings_repo: Arc::new(SqliteReviewSettingsRepository::from_shared(
                Arc::clone(&shared_conn),
            )),
            ui_feature_flag_overrides_repo: Arc::new(
                SqliteUiFeatureFlagOverridesRepository::from_shared(Arc::clone(&shared_conn)),
            ),
            agent_capability_gate: Arc::new(AgentCapabilityGate::default()),
            notification_settings_repo: Arc::new(
                SqliteNotificationSettingsRepository::from_shared(Arc::clone(&shared_conn)),
            ),
            window_focus_state: Arc::new(WindowFocusState::default()),
            notification_service_cache: Arc::new(OnceLock::new()),
            validation_run_repo: Arc::new(SqliteValidationRunRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            workspace_review_runtime_settings_repo: Arc::new(
                SqliteWorkspaceReviewRuntimeSettingsRepository::from_shared(Arc::clone(
                    &shared_conn,
                )),
            ),
            review_issue_repo: Arc::new(SqliteReviewIssueRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            agent_clients: Self::production_agent_clients(
                Arc::clone(&mcp_policy_repo),
                Arc::clone(&project_repo),
                Arc::clone(&agent_provider_settings_repo),
                app_paths.global_mcp_policy_path(),
            ),
            qa_settings: Arc::new(tokio::sync::RwLock::new(QASettings::default())),
            execution_settings_repo: Arc::new(SqliteExecutionSettingsRepository::from_shared(
                Arc::clone(&shared_conn),
            )),
            global_execution_settings_repo: Arc::new(
                SqliteGlobalExecutionSettingsRepository::from_shared(Arc::clone(&shared_conn)),
            ),
            ideation_session_repo: Arc::new(SqliteIdeationSessionRepository::from_shared(
                Arc::clone(&shared_conn),
            )),
            plan_approval_repo: Arc::new(SqlitePlanArtifactApprovalRepository::new(
                crate::infrastructure::sqlite::DbConnection::from_shared(Arc::clone(&shared_conn)),
            )),
            delegated_session_repo: Arc::new(SqliteDelegatedSessionRepository::from_shared(
                Arc::clone(&shared_conn),
            )),
            agent_task_repo: Arc::new(SqliteAgentTaskRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            agent_workflow_repo: Arc::new(SqliteAgentWorkflowRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            agent_conversation_issue_repo: Arc::new(
                SqliteAgentConversationIssueRepository::from_shared(Arc::clone(&shared_conn)),
            ),
            ideation_settings_repo: Arc::new(SqliteIdeationSettingsRepository::from_shared(
                Arc::clone(&shared_conn),
            )),
            ideation_effort_settings_repo: Arc::new(
                SqliteIdeationEffortSettingsRepository::from_shared(Arc::clone(&shared_conn)),
            ),
            ideation_model_settings_repo: Arc::new(
                SqliteIdeationModelSettingsRepository::from_shared(Arc::clone(&shared_conn)),
            ),
            agent_lane_settings_repo: Arc::new(SqliteAgentLaneSettingsRepository::from_shared(
                Arc::clone(&shared_conn),
            )),
            manual_role_default_repo: Arc::new(SqliteManualRoleDefaultRepository::from_shared(
                Arc::clone(&shared_conn),
            )),
            mcp_policy_repo,
            agent_model_registry_repo: Arc::new(SqliteAgentModelRegistryRepository::from_shared(
                Arc::clone(&shared_conn),
            )),
            agent_provider_settings_repo,
            session_link_repo: Arc::new(SqliteSessionLinkRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            task_proposal_repo: Arc::clone(&task_proposal_repo),
            proposal_dependency_repo: Arc::new(SqliteProposalDependencyRepository::from_shared(
                Arc::clone(&shared_conn),
            )),
            chat_message_repo: Arc::new(SqliteChatMessageRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            chat_timeline_repo: Arc::new(SqliteChatTimelineRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            chat_conversation_repo: Arc::new(SqliteChatConversationRepository::from_shared(
                Arc::clone(&shared_conn),
            )),
            persona_repo: Arc::new(SqlitePersonaRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            agent_conversation_workspace_repo,
            agent_workspace_repair_repo,
            agent_workspace_repair_publish_continuation: Arc::new(RwLock::new(None)),
            agent_workspace_pr_fix_review_publish_resumer: Arc::new(RwLock::new(None)),
            agent_conversation_jira_issue_repo: Arc::new(
                SqliteAgentConversationJiraIssueRepository::from_shared(Arc::clone(&shared_conn)),
            ),
            agent_conversation_linear_issue_repo: Arc::new(
                SqliteAgentConversationLinearIssueRepository::from_shared(Arc::clone(&shared_conn)),
            ),
            agent_conversation_granola_note_repo: Arc::new(
                SqliteAgentConversationGranolaNoteRepository::from_shared(Arc::clone(&shared_conn)),
            ),
            agent_conversation_mute_repo: Arc::new(
                SqliteAgentConversationMuteRepository::from_shared(Arc::clone(&shared_conn)),
            ),
            remote_conversation_start_request_repo: Arc::new(
                SqliteRemoteConversationStartRequestRepository::from_shared(Arc::clone(
                    &shared_conn,
                )),
            ),
            remote_agent_stop_request_repo: Arc::new(
                SqliteRemoteAgentStopRequestRepository::from_shared(Arc::clone(&shared_conn)),
            ),
            ticket_canonical_branch_repo: Arc::new(
                SqliteTicketCanonicalBranchRepository::from_shared(Arc::clone(&shared_conn)),
            ),
            orphan_worktree_cleanup_marker_repo: Arc::new(
                SqliteOrphanWorktreeCleanupMarkerRepository::from_shared(Arc::clone(&shared_conn)),
            ),
            automation_repo: Arc::new(SqliteAutomationRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            automation_run_repo: Arc::new(SqliteAutomationRunRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            agent_terminal_service: Arc::new(AgentTerminalService::new()),
            agent_run_repo: Arc::new(SqliteAgentRunRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            activity_event_repo: Arc::new(SqliteActivityEventRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            notification_repo: Arc::new(SqliteNotificationRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            task_dependency_repo: Arc::new(SqliteTaskDependencyRepository::from_shared(
                Arc::clone(&shared_conn),
            )),
            // Extensibility repositories
            workflow_repo: Arc::new(SqliteWorkflowRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            artifact_repo: Arc::clone(&artifact_repo),
            artifact_bucket_repo: Arc::new(SqliteArtifactBucketRepository::from_shared(
                Arc::clone(&shared_conn),
            )),
            artifact_flow_repo: Arc::new(SqliteArtifactFlowRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            process_repo: Arc::new(SqliteProcessRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            methodology_repo: Arc::new(SqliteMethodologyRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            plan_branch_repo: Arc::new(SqlitePlanBranchRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            plan_selection_stats_repo: Arc::new(SqlitePlanSelectionStatsRepository::from_shared(
                Arc::clone(&shared_conn),
            )),
            app_state_repo: Arc::new(SqliteAppStateRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            active_plan_repo: Arc::new(SqliteActivePlanRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            memory_entry_repo: Arc::new(SqliteMemoryEntryRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            memory_event_repo: Arc::new(SqliteMemoryEventRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            memory_archive_repo: Arc::new(SqliteMemoryArchiveRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            execution_plan_repo: Arc::new(SqliteExecutionPlanRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            chat_attachment_repo,
            conversation_folder_reference_repo: Arc::new(
                SqliteConversationFolderReferenceRepository::from_shared(Arc::clone(&shared_conn)),
            ),
            attachment_storage_path,
            permission_state: Arc::new(PermissionState::with_repo(Arc::new(
                SqlitePermissionRepository::from_shared(Arc::clone(&shared_conn)),
            ))),
            question_state: Arc::new(QuestionState::with_repo(Arc::new(
                SqliteQuestionRepository::from_shared(Arc::clone(&shared_conn)),
            ))),
            message_queue: Arc::new(MessageQueue::new()),
            queued_message_repo: Arc::new(SqliteQueuedMessageRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            db: crate::infrastructure::sqlite::DbConnection::from_shared(Arc::clone(&shared_conn)),
            external_events_repo: Arc::new(SqliteExternalEventsRepository::from_shared(
                Arc::clone(&shared_conn),
            )),
            github_service: Some(Arc::clone(&gh_svc)),
            pr_poller_registry: Arc::new(PrPollerRegistry::new(
                Some(gh_svc),
                Arc::new(
                    crate::infrastructure::sqlite::SqlitePlanBranchRepository::from_shared(
                        Arc::clone(&shared_conn),
                    ),
                ),
            )),
            running_agent_registry: Arc::new(SqliteRunningAgentRegistry::new(Arc::clone(
                &shared_conn,
            ))),
            webhook_registration_repo: Arc::new(SqliteWebhookRegistrationRepository::from_shared(
                Arc::clone(&shared_conn),
            )),
            webhook_publisher: None,
            session_merge_locks: Arc::new(dashmap::DashMap::new()),
            plan_verification_locks: Arc::new(dashmap::DashMap::new()),
            plan_verification_admissions: Arc::new(dashmap::DashMap::new()),
            auto_accept_sessions: Arc::new(Mutex::new(HashSet::new())),
            startup_git_auth_recovery_state: Arc::new(StartupGitAuthRecoveryState::default()),
            startup_coordinator: Arc::new(StartupCoordinator::new()),

            streaming_state_cache: crate::application::chat_service::StreamingStateCache::new(),
            interactive_process_registry: Arc::new(
                crate::application::InteractiveProcessRegistry::new(),
            ),
            app_handle: Some(app_handle),
            events,
            internal_event_bus,
            app_paths,
        };
        state
            .pr_poller_registry
            .set_notification_service(state.notification_service());
        state
            .pr_poller_registry
            .set_branch_update_repo(Arc::clone(&state.branch_update_repo));
        Ok(state)
    }

    /// Create AppState with a specific database path
    pub fn with_db_path(db_path: &str, app_handle: AppHandle) -> AppResult<Self> {
        let app_paths = AppPaths::from_app_handle(&app_handle)?;
        let (events, internal_event_bus) = Self::null_event_runtime();
        let path = PathBuf::from(db_path);
        let conn = open_connection(&path)?;
        run_migrations(&conn)?;
        let shared_conn = Arc::new(Mutex::new(conn));
        Self::build_from_shared_conn(
            app_handle,
            shared_conn,
            app_paths,
            events,
            internal_event_bus,
        )
    }

    /// Create AppState for testing with in-memory repositories
    /// No AppHandle is provided - event emission is disabled in tests
    pub fn new_test() -> Self {
        Self::enable_claude_test_mode();
        Self::with_repos(
            Arc::new(MemoryTaskRepository::new()),
            Arc::new(MemoryProjectRepository::new()),
        )
    }

    /// Create AppState for handler tests that need SQLite-backed artifact/session/proposal repos.
    ///
    /// The artifact, ideation_session, and task_proposal repositories share one in-memory
    /// SQLite connection with `db`, so handlers calling `db.run_transaction()` with sync helpers
    /// see the same rows that the test inserts via the repo trait methods. All other repos use
    /// in-memory implementations as in `new_test()`.
    #[doc(hidden)]
    pub fn new_sqlite_test() -> Self {
        Self::enable_claude_test_mode();
        let conn = open_connection(&std::path::PathBuf::from(":memory:"))
            .expect("Failed to open in-memory SQLite for handler tests");
        run_migrations(&conn).expect("Failed to run migrations on in-memory test DB");
        conn.execute(
            "UPDATE ideation_settings SET tasks_enabled = 1, tasks_feature_state = 'enabled' WHERE id = 1",
            [],
        )
        .expect("Failed to enable Tasks for legacy handler tests");
        // Migrations may leave foreign_keys = ON. Disable for tests: we test handler logic,
        // not FK enforcement. Sessions reference projects that don't exist in the test DB.
        conn.execute("PRAGMA foreign_keys = OFF", [])
            .expect("Failed to disable foreign_keys for test DB");
        let shared_conn = Arc::new(tokio::sync::Mutex::new(conn));

        let chat_attachment_repo: Arc<dyn ChatAttachmentRepository> =
            Arc::new(MemoryChatAttachmentRepository::new());
        let app_paths = AppPaths::for_tests();
        let attachment_storage_path = app_paths.attachment_storage_path();
        let (events, internal_event_bus) = Self::null_event_runtime();
        let automation_state = MemoryAutomationRepository::new_shared_state();
        let (agent_conversation_workspace_repo, agent_workspace_repair_repo) =
            Self::memory_agent_workspace_repositories();

        Self {
            task_repo: Arc::new(MemoryTaskRepository::new()),
            branch_update_repo: Arc::new(SqliteBranchUpdateRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            task_step_repo: Arc::new(MemoryTaskStepRepository::new()),
            project_repo: Arc::new(MemoryProjectRepository::new()),
            api_key_repo: Arc::new(MemoryApiKeyRepository::new()),
            remote_environment_service: Self::memory_remote_environment_service(),
            atlassian_integration_service: Self::memory_atlassian_integration_service(),
            linear_integration_service: Self::memory_linear_integration_service(),
            clickup_integration_service: Self::memory_clickup_integration_service(),
            granola_integration_service: Self::memory_granola_integration_service(),
            external_issue_link_service: Self::production_external_issue_link_service(&shared_conn),
            ticketing_status_catalog_service: Self::production_ticketing_status_catalog_service(
                &shared_conn,
            ),
            agent_profile_repo: Arc::new(MemoryAgentProfileRepository::new()),
            task_qa_repo: Arc::new(MemoryTaskQARepository::new()),
            review_repo: Arc::new(MemoryReviewRepository::new()),
            review_settings_repo: Arc::new(MemoryReviewSettingsRepository::new()),
            ui_feature_flag_overrides_repo: Arc::new(MemoryUiFeatureFlagOverridesRepository::new()),
            agent_capability_gate: Arc::new(AgentCapabilityGate::default()),
            notification_settings_repo: Arc::new(MemoryNotificationSettingsRepository::new()),
            window_focus_state: Arc::new(WindowFocusState::default()),
            notification_service_cache: Arc::new(OnceLock::new()),
            validation_run_repo: Arc::new(MemoryValidationRunRepository::new()),
            workspace_review_runtime_settings_repo: Arc::new(
                MemoryWorkspaceReviewRuntimeSettingsRepository::new(),
            ),
            review_issue_repo: Arc::new(MemoryReviewIssueRepository::new()),
            agent_clients: Self::mock_agent_clients(),
            qa_settings: Arc::new(tokio::sync::RwLock::new(QASettings::default())),
            execution_settings_repo: Arc::new(MemoryExecutionSettingsRepository::new()),
            global_execution_settings_repo: Arc::new(MemoryGlobalExecutionSettingsRepository::new()),
            ideation_session_repo: Arc::new(SqliteIdeationSessionRepository::from_shared(
                Arc::clone(&shared_conn),
            )),
            plan_approval_repo: Arc::new(SqlitePlanArtifactApprovalRepository::new(
                crate::infrastructure::sqlite::DbConnection::from_shared(Arc::clone(&shared_conn)),
            )),
            delegated_session_repo: Arc::new(SqliteDelegatedSessionRepository::from_shared(
                Arc::clone(&shared_conn),
            )),
            agent_task_repo: Arc::new(SqliteAgentTaskRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            agent_workflow_repo: Arc::new(SqliteAgentWorkflowRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            agent_conversation_issue_repo: Arc::new(
                SqliteAgentConversationIssueRepository::from_shared(Arc::clone(&shared_conn)),
            ),
            ideation_settings_repo: Arc::new(MemoryIdeationSettingsRepository::with_settings(
                IdeationSettings {
                    tasks_enabled: true,
                    tasks_feature_state: TasksFeatureState::Enabled,
                    ..Default::default()
                },
            )),
            ideation_effort_settings_repo: Arc::new(MemoryIdeationEffortSettingsRepository::new()),
            ideation_model_settings_repo: Arc::new(MemoryIdeationModelSettingsRepository::new()),
            agent_lane_settings_repo: Arc::new(MemoryAgentLaneSettingsRepository::new()),
            manual_role_default_repo: Arc::new(MemoryManualRoleDefaultRepository::new()),
            mcp_policy_repo: Arc::new(MemoryMcpPolicyRepository::new()),
            agent_model_registry_repo: Arc::new(MemoryAgentModelRegistryRepository::new()),
            agent_provider_settings_repo: Arc::new(
                MemoryAgentProviderSettingsRepository::with_all_providers_enabled(
                    DEFAULT_AGENT_HARNESS,
                ),
            ),
            session_link_repo: Arc::new(MemorySessionLinkRepository::new()),
            task_proposal_repo: Arc::new(SqliteTaskProposalRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            proposal_dependency_repo: Arc::new(MemoryProposalDependencyRepository::new()),
            chat_message_repo: Arc::new(MemoryChatMessageRepository::new()),
            chat_timeline_repo: Arc::new(MemoryChatTimelineRepository::new()),
            chat_conversation_repo: Arc::new(MemoryChatConversationRepository::new()),
            persona_repo: Arc::new(MemoryPersonaRepository::new()),
            agent_conversation_workspace_repo,
            agent_workspace_repair_repo,
            agent_workspace_repair_publish_continuation: Arc::new(RwLock::new(None)),
            agent_workspace_pr_fix_review_publish_resumer: Arc::new(RwLock::new(None)),
            agent_conversation_jira_issue_repo: Arc::new(
                MemoryAgentConversationJiraIssueRepository::new(),
            ),
            agent_conversation_linear_issue_repo: Arc::new(
                MemoryAgentConversationLinearIssueRepository::new(),
            ),
            agent_conversation_granola_note_repo: Arc::new(
                MemoryAgentConversationGranolaNoteRepository::new(),
            ),
            agent_conversation_mute_repo: Arc::new(MemoryAgentConversationMuteRepository::new()),
            remote_conversation_start_request_repo: Arc::new(
                MemoryRemoteConversationStartRequestRepository::new(),
            ),
            remote_agent_stop_request_repo: Arc::new(MemoryRemoteAgentStopRequestRepository::new()),
            ticket_canonical_branch_repo: Arc::new(MemoryTicketCanonicalBranchRepository::new()),
            orphan_worktree_cleanup_marker_repo: Arc::new(
                MemoryOrphanWorktreeCleanupMarkerRepository::new(),
            ),
            automation_repo: Arc::new(MemoryAutomationRepository::with_shared_state(Arc::clone(
                &automation_state,
            ))),
            automation_run_repo: Arc::new(MemoryAutomationRunRepository::new(automation_state)),
            agent_terminal_service: Arc::new(AgentTerminalService::new()),
            agent_run_repo: Arc::new(MemoryAgentRunRepository::new()),
            activity_event_repo: Arc::new(MemoryActivityEventRepository::new()),
            notification_repo: Arc::new(MemoryNotificationRepository::new()),
            task_dependency_repo: Arc::new(MemoryTaskDependencyRepository::new()),
            workflow_repo: Arc::new(MemoryWorkflowRepository::new()),
            artifact_repo: Arc::new(SqliteArtifactRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            artifact_bucket_repo: Arc::new(MemoryArtifactBucketRepository::new()),
            artifact_flow_repo: Arc::new(MemoryArtifactFlowRepository::new()),
            process_repo: Arc::new(MemoryProcessRepository::new()),
            methodology_repo: Arc::new(MemoryMethodologyRepository::new()),
            plan_branch_repo: Arc::new(MemoryPlanBranchRepository::new()),
            plan_selection_stats_repo: Arc::new(MemoryPlanSelectionStatsRepository::new()),
            app_state_repo: Arc::new(MemoryAppStateRepository::new()),
            active_plan_repo: Arc::new(MemoryActivePlanRepository::new()),
            memory_entry_repo: Arc::new(InMemoryMemoryEntryRepository::new()),
            memory_event_repo: Arc::new(InMemoryMemoryEventRepository::new()),
            memory_archive_repo: Arc::new(SqliteMemoryArchiveRepository::new(
                open_connection(&std::path::PathBuf::from(":memory:"))
                    .expect("Failed to create in-memory connection for memory_archive"),
            )),
            execution_plan_repo: Arc::new(MemoryExecutionPlanRepository::new()),
            chat_attachment_repo,
            conversation_folder_reference_repo: Arc::new(
                MemoryConversationFolderReferenceRepository::new(),
            ),
            attachment_storage_path,
            permission_state: Arc::new(PermissionState::with_repo(Arc::new(
                MemoryPermissionRepository::new(),
            ))),
            question_state: Arc::new(QuestionState::with_repo(Arc::new(
                MemoryQuestionRepository::new(),
            ))),
            message_queue: Arc::new(MessageQueue::new()),
            queued_message_repo: Arc::new(SqliteQueuedMessageRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            db: crate::infrastructure::sqlite::DbConnection::from_shared(Arc::clone(&shared_conn)),
            external_events_repo: Arc::new(MemoryExternalEventsRepository::new()),
            running_agent_registry: Arc::new(MemoryRunningAgentRegistry::new()),
            webhook_registration_repo: Arc::new(MemoryWebhookRegistrationRepository::new()),
            webhook_publisher: None,
            session_merge_locks: Arc::new(dashmap::DashMap::new()),
            plan_verification_locks: Arc::new(dashmap::DashMap::new()),
            plan_verification_admissions: Arc::new(dashmap::DashMap::new()),
            auto_accept_sessions: Arc::new(Mutex::new(HashSet::new())),
            startup_git_auth_recovery_state: Arc::new(StartupGitAuthRecoveryState::default()),
            startup_coordinator: Arc::new(StartupCoordinator::new()),

            streaming_state_cache: crate::application::chat_service::StreamingStateCache::new(),
            interactive_process_registry: Arc::new(
                crate::application::InteractiveProcessRegistry::new(),
            ),
            app_handle: None,
            events,
            internal_event_bus,
            app_paths,
            github_service: None,
            pr_poller_registry: Arc::new(PrPollerRegistry::new(
                None,
                Arc::new(MemoryPlanBranchRepository::new()),
            )),
        }
    }

    /// Create AppState for handler tests that need a pre-seeded RunningAgentRegistry.
    ///
    /// Identical to `new_sqlite_test()` except the `running_agent_registry` is injected
    /// by the caller. Use `MemoryRunningAgentRegistry::set_running()` to seed it before
    /// passing it here, so freeze-check tests can control the registry state.
    #[doc(hidden)]
    pub fn new_sqlite_test_with_registry(registry: Arc<MemoryRunningAgentRegistry>) -> Self {
        Self::enable_claude_test_mode();
        let conn = open_connection(&std::path::PathBuf::from(":memory:"))
            .expect("Failed to open in-memory SQLite for handler tests");
        run_migrations(&conn).expect("Failed to run migrations on in-memory test DB");
        conn.execute(
            "UPDATE ideation_settings SET tasks_enabled = 1, tasks_feature_state = 'enabled' WHERE id = 1",
            [],
        )
        .expect("Failed to enable Tasks for legacy handler tests");
        conn.execute("PRAGMA foreign_keys = OFF", [])
            .expect("Failed to disable foreign_keys for test DB");
        let shared_conn = Arc::new(tokio::sync::Mutex::new(conn));

        let chat_attachment_repo: Arc<dyn ChatAttachmentRepository> =
            Arc::new(MemoryChatAttachmentRepository::new());
        let app_paths = AppPaths::for_tests();
        let attachment_storage_path = app_paths.attachment_storage_path();
        let (events, internal_event_bus) = Self::null_event_runtime();
        let automation_state = MemoryAutomationRepository::new_shared_state();
        let (agent_conversation_workspace_repo, agent_workspace_repair_repo) =
            Self::memory_agent_workspace_repositories();

        Self {
            task_repo: Arc::new(MemoryTaskRepository::new()),
            branch_update_repo: Arc::new(SqliteBranchUpdateRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            task_step_repo: Arc::new(MemoryTaskStepRepository::new()),
            project_repo: Arc::new(MemoryProjectRepository::new()),
            api_key_repo: Arc::new(MemoryApiKeyRepository::new()),
            remote_environment_service: Self::memory_remote_environment_service(),
            atlassian_integration_service: Self::memory_atlassian_integration_service(),
            linear_integration_service: Self::memory_linear_integration_service(),
            clickup_integration_service: Self::memory_clickup_integration_service(),
            granola_integration_service: Self::memory_granola_integration_service(),
            external_issue_link_service: Self::production_external_issue_link_service(&shared_conn),
            ticketing_status_catalog_service: Self::production_ticketing_status_catalog_service(
                &shared_conn,
            ),
            agent_profile_repo: Arc::new(MemoryAgentProfileRepository::new()),
            task_qa_repo: Arc::new(MemoryTaskQARepository::new()),
            review_repo: Arc::new(MemoryReviewRepository::new()),
            review_settings_repo: Arc::new(MemoryReviewSettingsRepository::new()),
            ui_feature_flag_overrides_repo: Arc::new(MemoryUiFeatureFlagOverridesRepository::new()),
            agent_capability_gate: Arc::new(AgentCapabilityGate::default()),
            notification_settings_repo: Arc::new(MemoryNotificationSettingsRepository::new()),
            window_focus_state: Arc::new(WindowFocusState::default()),
            notification_service_cache: Arc::new(OnceLock::new()),
            validation_run_repo: Arc::new(MemoryValidationRunRepository::new()),
            workspace_review_runtime_settings_repo: Arc::new(
                MemoryWorkspaceReviewRuntimeSettingsRepository::new(),
            ),
            review_issue_repo: Arc::new(MemoryReviewIssueRepository::new()),
            agent_clients: Self::mock_agent_clients(),
            qa_settings: Arc::new(tokio::sync::RwLock::new(QASettings::default())),
            execution_settings_repo: Arc::new(MemoryExecutionSettingsRepository::new()),
            global_execution_settings_repo: Arc::new(MemoryGlobalExecutionSettingsRepository::new()),
            ideation_session_repo: Arc::new(SqliteIdeationSessionRepository::from_shared(
                Arc::clone(&shared_conn),
            )),
            plan_approval_repo: Arc::new(SqlitePlanArtifactApprovalRepository::new(
                crate::infrastructure::sqlite::DbConnection::from_shared(Arc::clone(&shared_conn)),
            )),
            delegated_session_repo: Arc::new(SqliteDelegatedSessionRepository::from_shared(
                Arc::clone(&shared_conn),
            )),
            agent_task_repo: Arc::new(SqliteAgentTaskRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            agent_workflow_repo: Arc::new(SqliteAgentWorkflowRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            agent_conversation_issue_repo: Arc::new(
                SqliteAgentConversationIssueRepository::from_shared(Arc::clone(&shared_conn)),
            ),
            ideation_settings_repo: Arc::new(MemoryIdeationSettingsRepository::with_settings(
                IdeationSettings {
                    tasks_enabled: true,
                    tasks_feature_state: TasksFeatureState::Enabled,
                    ..Default::default()
                },
            )),
            ideation_effort_settings_repo: Arc::new(MemoryIdeationEffortSettingsRepository::new()),
            ideation_model_settings_repo: Arc::new(MemoryIdeationModelSettingsRepository::new()),
            agent_lane_settings_repo: Arc::new(MemoryAgentLaneSettingsRepository::new()),
            manual_role_default_repo: Arc::new(MemoryManualRoleDefaultRepository::new()),
            mcp_policy_repo: Arc::new(MemoryMcpPolicyRepository::new()),
            agent_model_registry_repo: Arc::new(MemoryAgentModelRegistryRepository::new()),
            agent_provider_settings_repo: Arc::new(
                MemoryAgentProviderSettingsRepository::with_all_providers_enabled(
                    DEFAULT_AGENT_HARNESS,
                ),
            ),
            session_link_repo: Arc::new(MemorySessionLinkRepository::new()),
            task_proposal_repo: Arc::new(SqliteTaskProposalRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            proposal_dependency_repo: Arc::new(MemoryProposalDependencyRepository::new()),
            chat_message_repo: Arc::new(MemoryChatMessageRepository::new()),
            chat_timeline_repo: Arc::new(MemoryChatTimelineRepository::new()),
            chat_conversation_repo: Arc::new(MemoryChatConversationRepository::new()),
            persona_repo: Arc::new(MemoryPersonaRepository::new()),
            agent_conversation_workspace_repo,
            agent_workspace_repair_repo,
            agent_workspace_repair_publish_continuation: Arc::new(RwLock::new(None)),
            agent_workspace_pr_fix_review_publish_resumer: Arc::new(RwLock::new(None)),
            agent_conversation_jira_issue_repo: Arc::new(
                MemoryAgentConversationJiraIssueRepository::new(),
            ),
            agent_conversation_linear_issue_repo: Arc::new(
                MemoryAgentConversationLinearIssueRepository::new(),
            ),
            agent_conversation_granola_note_repo: Arc::new(
                MemoryAgentConversationGranolaNoteRepository::new(),
            ),
            agent_conversation_mute_repo: Arc::new(MemoryAgentConversationMuteRepository::new()),
            remote_conversation_start_request_repo: Arc::new(
                MemoryRemoteConversationStartRequestRepository::new(),
            ),
            remote_agent_stop_request_repo: Arc::new(MemoryRemoteAgentStopRequestRepository::new()),
            ticket_canonical_branch_repo: Arc::new(MemoryTicketCanonicalBranchRepository::new()),
            orphan_worktree_cleanup_marker_repo: Arc::new(
                MemoryOrphanWorktreeCleanupMarkerRepository::new(),
            ),
            automation_repo: Arc::new(MemoryAutomationRepository::with_shared_state(Arc::clone(
                &automation_state,
            ))),
            automation_run_repo: Arc::new(MemoryAutomationRunRepository::new(automation_state)),
            agent_terminal_service: Arc::new(AgentTerminalService::new()),
            agent_run_repo: Arc::new(MemoryAgentRunRepository::new()),
            activity_event_repo: Arc::new(MemoryActivityEventRepository::new()),
            notification_repo: Arc::new(MemoryNotificationRepository::new()),
            task_dependency_repo: Arc::new(MemoryTaskDependencyRepository::new()),
            workflow_repo: Arc::new(MemoryWorkflowRepository::new()),
            artifact_repo: Arc::new(SqliteArtifactRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            artifact_bucket_repo: Arc::new(MemoryArtifactBucketRepository::new()),
            artifact_flow_repo: Arc::new(MemoryArtifactFlowRepository::new()),
            process_repo: Arc::new(MemoryProcessRepository::new()),
            methodology_repo: Arc::new(MemoryMethodologyRepository::new()),
            plan_branch_repo: Arc::new(MemoryPlanBranchRepository::new()),
            plan_selection_stats_repo: Arc::new(MemoryPlanSelectionStatsRepository::new()),
            app_state_repo: Arc::new(MemoryAppStateRepository::new()),
            active_plan_repo: Arc::new(MemoryActivePlanRepository::new()),
            memory_entry_repo: Arc::new(InMemoryMemoryEntryRepository::new()),
            memory_event_repo: Arc::new(InMemoryMemoryEventRepository::new()),
            memory_archive_repo: Arc::new(SqliteMemoryArchiveRepository::new(
                open_connection(&std::path::PathBuf::from(":memory:"))
                    .expect("Failed to create in-memory connection for memory_archive"),
            )),
            execution_plan_repo: Arc::new(MemoryExecutionPlanRepository::new()),
            chat_attachment_repo,
            conversation_folder_reference_repo: Arc::new(
                MemoryConversationFolderReferenceRepository::new(),
            ),
            attachment_storage_path,
            permission_state: Arc::new(PermissionState::with_repo(Arc::new(
                MemoryPermissionRepository::new(),
            ))),
            question_state: Arc::new(QuestionState::with_repo(Arc::new(
                MemoryQuestionRepository::new(),
            ))),
            message_queue: Arc::new(MessageQueue::new()),
            queued_message_repo: Arc::new(SqliteQueuedMessageRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            db: crate::infrastructure::sqlite::DbConnection::from_shared(Arc::clone(&shared_conn)),
            external_events_repo: Arc::new(MemoryExternalEventsRepository::new()),
            running_agent_registry: registry,
            webhook_registration_repo: Arc::new(MemoryWebhookRegistrationRepository::new()),
            webhook_publisher: None,
            session_merge_locks: Arc::new(dashmap::DashMap::new()),
            plan_verification_locks: Arc::new(dashmap::DashMap::new()),
            plan_verification_admissions: Arc::new(dashmap::DashMap::new()),
            auto_accept_sessions: Arc::new(Mutex::new(HashSet::new())),
            startup_git_auth_recovery_state: Arc::new(StartupGitAuthRecoveryState::default()),
            startup_coordinator: Arc::new(StartupCoordinator::new()),

            streaming_state_cache: crate::application::chat_service::StreamingStateCache::new(),
            interactive_process_registry: Arc::new(
                crate::application::InteractiveProcessRegistry::new(),
            ),
            app_handle: None,
            events,
            internal_event_bus,
            app_paths,
            github_service: None,
            pr_poller_registry: Arc::new(PrPollerRegistry::new(
                None,
                Arc::new(MemoryPlanBranchRepository::new()),
            )),
        }
    }

    /// Create AppState for `apply_proposals_core` tests.
    ///
    /// Uses a single shared in-memory SQLite connection (with full migrations) for all
    /// repositories that `apply_proposals_core` touches — both via async repo methods AND
    /// via `db.run_transaction()`. This ensures that rows written inside the transaction
    /// are immediately visible to subsequent async repo reads in the same test.
    ///
    /// Repositories backed by the shared connection:
    /// - `ideation_session_repo`, `task_proposal_repo`, `proposal_dependency_repo`
    /// - `execution_plan_repo`, `task_repo`, `task_step_repo`, `task_dependency_repo`
    /// - `plan_branch_repo`, `project_repo`, `active_plan_repo`, `db`
    #[doc(hidden)]
    pub fn new_sqlite_for_apply_test() -> Self {
        Self::enable_claude_test_mode();
        let conn = open_connection(&std::path::PathBuf::from(":memory:"))
            .expect("Failed to open in-memory SQLite for apply_proposals_core tests");
        run_migrations(&conn).expect("Failed to run migrations on in-memory test DB");
        conn.execute(
            "UPDATE ideation_settings SET tasks_enabled = 1, tasks_feature_state = 'enabled' WHERE id = 1",
            [],
        )
        .expect("Failed to enable Tasks for legacy apply tests");
        conn.execute("PRAGMA foreign_keys = OFF", [])
            .expect("Failed to disable foreign_keys for test DB");
        let shared_conn = Arc::new(tokio::sync::Mutex::new(conn));

        let chat_attachment_repo: Arc<dyn ChatAttachmentRepository> =
            Arc::new(MemoryChatAttachmentRepository::new());
        let app_paths = AppPaths::for_tests();
        let attachment_storage_path = app_paths.attachment_storage_path();
        let (events, internal_event_bus) = Self::null_event_runtime();
        let (agent_conversation_workspace_repo, agent_workspace_repair_repo) =
            Self::sqlite_agent_workspace_repositories(&shared_conn);

        Self {
            task_repo: Arc::new(SqliteTaskRepository::from_shared(Arc::clone(&shared_conn))),
            branch_update_repo: Arc::new(SqliteBranchUpdateRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            task_step_repo: Arc::new(
                SqliteTaskStepRepository::from_shared(Arc::clone(&shared_conn))
                    .with_tasks_feature_policy(),
            ),
            project_repo: Arc::new(SqliteProjectRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            api_key_repo: Arc::new(MemoryApiKeyRepository::new()),
            remote_environment_service: Self::memory_remote_environment_service(),
            atlassian_integration_service: Self::memory_atlassian_integration_service(),
            linear_integration_service: Self::memory_linear_integration_service(),
            clickup_integration_service: Self::memory_clickup_integration_service(),
            granola_integration_service: Self::memory_granola_integration_service(),
            external_issue_link_service: Self::production_external_issue_link_service(&shared_conn),
            ticketing_status_catalog_service: Self::production_ticketing_status_catalog_service(
                &shared_conn,
            ),
            agent_profile_repo: Arc::new(MemoryAgentProfileRepository::new()),
            task_qa_repo: Arc::new(MemoryTaskQARepository::new()),
            review_repo: Arc::new(MemoryReviewRepository::new()),
            review_settings_repo: Arc::new(MemoryReviewSettingsRepository::new()),
            ui_feature_flag_overrides_repo: Arc::new(MemoryUiFeatureFlagOverridesRepository::new()),
            agent_capability_gate: Arc::new(AgentCapabilityGate::default()),
            notification_settings_repo: Arc::new(MemoryNotificationSettingsRepository::new()),
            window_focus_state: Arc::new(WindowFocusState::default()),
            notification_service_cache: Arc::new(OnceLock::new()),
            validation_run_repo: Arc::new(MemoryValidationRunRepository::new()),
            workspace_review_runtime_settings_repo: Arc::new(
                MemoryWorkspaceReviewRuntimeSettingsRepository::new(),
            ),
            review_issue_repo: Arc::new(MemoryReviewIssueRepository::new()),
            agent_clients: Self::mock_agent_clients(),
            qa_settings: Arc::new(tokio::sync::RwLock::new(QASettings::default())),
            execution_settings_repo: Arc::new(MemoryExecutionSettingsRepository::new()),
            global_execution_settings_repo: Arc::new(MemoryGlobalExecutionSettingsRepository::new()),
            ideation_session_repo: Arc::new(SqliteIdeationSessionRepository::from_shared(
                Arc::clone(&shared_conn),
            )),
            plan_approval_repo: Arc::new(SqlitePlanArtifactApprovalRepository::new(
                crate::infrastructure::sqlite::DbConnection::from_shared(Arc::clone(&shared_conn)),
            )),
            delegated_session_repo: Arc::new(SqliteDelegatedSessionRepository::from_shared(
                Arc::clone(&shared_conn),
            )),
            agent_task_repo: Arc::new(SqliteAgentTaskRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            agent_workflow_repo: Arc::new(SqliteAgentWorkflowRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            agent_conversation_issue_repo: Arc::new(
                SqliteAgentConversationIssueRepository::from_shared(Arc::clone(&shared_conn)),
            ),
            ideation_settings_repo: Arc::new(MemoryIdeationSettingsRepository::with_settings(
                IdeationSettings {
                    tasks_enabled: true,
                    tasks_feature_state: TasksFeatureState::Enabled,
                    ..Default::default()
                },
            )),
            ideation_effort_settings_repo: Arc::new(MemoryIdeationEffortSettingsRepository::new()),
            ideation_model_settings_repo: Arc::new(MemoryIdeationModelSettingsRepository::new()),
            agent_lane_settings_repo: Arc::new(MemoryAgentLaneSettingsRepository::new()),
            manual_role_default_repo: Arc::new(MemoryManualRoleDefaultRepository::new()),
            mcp_policy_repo: Arc::new(MemoryMcpPolicyRepository::new()),
            agent_model_registry_repo: Arc::new(MemoryAgentModelRegistryRepository::new()),
            agent_provider_settings_repo: Arc::new(
                MemoryAgentProviderSettingsRepository::with_all_providers_enabled(
                    DEFAULT_AGENT_HARNESS,
                ),
            ),
            session_link_repo: Arc::new(MemorySessionLinkRepository::new()),
            task_proposal_repo: Arc::new(SqliteTaskProposalRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            proposal_dependency_repo: Arc::new(SqliteProposalDependencyRepository::from_shared(
                Arc::clone(&shared_conn),
            )),
            chat_message_repo: Arc::new(MemoryChatMessageRepository::new()),
            chat_timeline_repo: Arc::new(MemoryChatTimelineRepository::new()),
            chat_conversation_repo: Arc::new(MemoryChatConversationRepository::new()),
            persona_repo: Arc::new(MemoryPersonaRepository::new()),
            agent_conversation_workspace_repo,
            agent_workspace_repair_repo,
            agent_workspace_repair_publish_continuation: Arc::new(RwLock::new(None)),
            agent_workspace_pr_fix_review_publish_resumer: Arc::new(RwLock::new(None)),
            agent_conversation_jira_issue_repo: Arc::new(
                SqliteAgentConversationJiraIssueRepository::from_shared(Arc::clone(&shared_conn)),
            ),
            agent_conversation_linear_issue_repo: Arc::new(
                SqliteAgentConversationLinearIssueRepository::from_shared(Arc::clone(&shared_conn)),
            ),
            agent_conversation_granola_note_repo: Arc::new(
                SqliteAgentConversationGranolaNoteRepository::from_shared(Arc::clone(&shared_conn)),
            ),
            agent_conversation_mute_repo: Arc::new(
                SqliteAgentConversationMuteRepository::from_shared(Arc::clone(&shared_conn)),
            ),
            remote_conversation_start_request_repo: Arc::new(
                SqliteRemoteConversationStartRequestRepository::from_shared(Arc::clone(
                    &shared_conn,
                )),
            ),
            remote_agent_stop_request_repo: Arc::new(
                SqliteRemoteAgentStopRequestRepository::from_shared(Arc::clone(&shared_conn)),
            ),
            ticket_canonical_branch_repo: Arc::new(
                SqliteTicketCanonicalBranchRepository::from_shared(Arc::clone(&shared_conn)),
            ),
            orphan_worktree_cleanup_marker_repo: Arc::new(
                SqliteOrphanWorktreeCleanupMarkerRepository::from_shared(Arc::clone(&shared_conn)),
            ),
            automation_repo: Arc::new(SqliteAutomationRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            automation_run_repo: Arc::new(SqliteAutomationRunRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            agent_terminal_service: Arc::new(AgentTerminalService::new()),
            agent_run_repo: Arc::new(MemoryAgentRunRepository::new()),
            activity_event_repo: Arc::new(MemoryActivityEventRepository::new()),
            notification_repo: Arc::new(MemoryNotificationRepository::new()),
            task_dependency_repo: Arc::new(SqliteTaskDependencyRepository::from_shared(
                Arc::clone(&shared_conn),
            )),
            workflow_repo: Arc::new(MemoryWorkflowRepository::new()),
            artifact_repo: Arc::new(SqliteArtifactRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            artifact_bucket_repo: Arc::new(MemoryArtifactBucketRepository::new()),
            artifact_flow_repo: Arc::new(MemoryArtifactFlowRepository::new()),
            process_repo: Arc::new(MemoryProcessRepository::new()),
            methodology_repo: Arc::new(MemoryMethodologyRepository::new()),
            plan_branch_repo: Arc::new(SqlitePlanBranchRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            plan_selection_stats_repo: Arc::new(MemoryPlanSelectionStatsRepository::new()),
            app_state_repo: Arc::new(MemoryAppStateRepository::new()),
            active_plan_repo: Arc::new(SqliteActivePlanRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            memory_entry_repo: Arc::new(InMemoryMemoryEntryRepository::new()),
            memory_event_repo: Arc::new(InMemoryMemoryEventRepository::new()),
            memory_archive_repo: Arc::new(SqliteMemoryArchiveRepository::new(
                open_connection(&std::path::PathBuf::from(":memory:"))
                    .expect("Failed to create in-memory connection for memory_archive"),
            )),
            execution_plan_repo: Arc::new(SqliteExecutionPlanRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            chat_attachment_repo,
            conversation_folder_reference_repo: Arc::new(
                MemoryConversationFolderReferenceRepository::new(),
            ),
            attachment_storage_path,
            permission_state: Arc::new(PermissionState::with_repo(Arc::new(
                MemoryPermissionRepository::new(),
            ))),
            question_state: Arc::new(QuestionState::with_repo(Arc::new(
                MemoryQuestionRepository::new(),
            ))),
            message_queue: Arc::new(MessageQueue::new()),
            queued_message_repo: Arc::new(SqliteQueuedMessageRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            db: crate::infrastructure::sqlite::DbConnection::from_shared(Arc::clone(&shared_conn)),
            external_events_repo: Arc::new(MemoryExternalEventsRepository::new()),
            running_agent_registry: Arc::new(MemoryRunningAgentRegistry::new()),
            webhook_registration_repo: Arc::new(MemoryWebhookRegistrationRepository::new()),
            webhook_publisher: None,
            session_merge_locks: Arc::new(dashmap::DashMap::new()),
            plan_verification_locks: Arc::new(dashmap::DashMap::new()),
            plan_verification_admissions: Arc::new(dashmap::DashMap::new()),
            auto_accept_sessions: Arc::new(Mutex::new(HashSet::new())),
            startup_git_auth_recovery_state: Arc::new(StartupGitAuthRecoveryState::default()),
            startup_coordinator: Arc::new(StartupCoordinator::new()),

            streaming_state_cache: crate::application::chat_service::StreamingStateCache::new(),
            interactive_process_registry: Arc::new(
                crate::application::InteractiveProcessRegistry::new(),
            ),
            app_handle: None,
            events,
            internal_event_bus,
            app_paths,
            github_service: None,
            pr_poller_registry: Arc::new(PrPollerRegistry::new(
                None,
                Arc::new(MemoryPlanBranchRepository::new()),
            )),
        }
    }

    /// Create AppState with custom repositories (for dependency injection)
    /// No AppHandle is provided - event emission is disabled
    pub fn with_repos(
        task_repo: Arc<dyn TaskRepository>,
        project_repo: Arc<dyn ProjectRepository>,
    ) -> Self {
        let task_proposal_repo: Arc<dyn TaskProposalRepository> =
            Arc::new(MemoryTaskProposalRepository::new());
        let artifact_repo: Arc<dyn ArtifactRepository> = Arc::new(MemoryArtifactRepository::new());

        // Chat attachment repository for tests
        let chat_attachment_repo: Arc<dyn ChatAttachmentRepository> =
            Arc::new(MemoryChatAttachmentRepository::new());
        let app_paths = AppPaths::for_tests();
        let attachment_storage_path = app_paths.attachment_storage_path();
        let (events, internal_event_bus) = Self::null_event_runtime();
        let automation_state = MemoryAutomationRepository::new_shared_state();
        let (agent_conversation_workspace_repo, agent_workspace_repair_repo) =
            Self::memory_agent_workspace_repositories();

        Self {
            task_repo: Arc::clone(&task_repo),
            branch_update_repo: Arc::new(MemoryBranchUpdateRepository::new()),
            task_step_repo: Arc::new(MemoryTaskStepRepository::new()),
            project_repo,
            api_key_repo: Arc::new(MemoryApiKeyRepository::new()),
            remote_environment_service: Self::memory_remote_environment_service(),
            atlassian_integration_service: Self::memory_atlassian_integration_service(),
            linear_integration_service: Self::memory_linear_integration_service(),
            clickup_integration_service: Self::memory_clickup_integration_service(),
            granola_integration_service: Self::memory_granola_integration_service(),
            external_issue_link_service: Self::memory_external_issue_link_service(),
            ticketing_status_catalog_service: Self::memory_ticketing_status_catalog_service(),
            agent_profile_repo: Arc::new(MemoryAgentProfileRepository::new()),
            task_qa_repo: Arc::new(MemoryTaskQARepository::new()),
            review_repo: Arc::new(MemoryReviewRepository::new()),
            review_settings_repo: Arc::new(MemoryReviewSettingsRepository::new()),
            ui_feature_flag_overrides_repo: Arc::new(MemoryUiFeatureFlagOverridesRepository::new()),
            agent_capability_gate: Arc::new(AgentCapabilityGate::default()),
            notification_settings_repo: Arc::new(MemoryNotificationSettingsRepository::new()),
            window_focus_state: Arc::new(WindowFocusState::default()),
            notification_service_cache: Arc::new(OnceLock::new()),
            validation_run_repo: Arc::new(MemoryValidationRunRepository::new()),
            workspace_review_runtime_settings_repo: Arc::new(
                MemoryWorkspaceReviewRuntimeSettingsRepository::new(),
            ),
            review_issue_repo: Arc::new(MemoryReviewIssueRepository::new()),
            agent_clients: Self::mock_agent_clients(),
            qa_settings: Arc::new(tokio::sync::RwLock::new(QASettings::default())),
            execution_settings_repo: Arc::new(MemoryExecutionSettingsRepository::new()),
            global_execution_settings_repo: Arc::new(MemoryGlobalExecutionSettingsRepository::new()),
            ideation_session_repo: Arc::new(MemoryIdeationSessionRepository::new()),
            plan_approval_repo: Arc::new(MemoryPlanArtifactApprovalRepository::new()),
            delegated_session_repo: Arc::new(MemoryDelegatedSessionRepository::new()),
            agent_task_repo: Arc::new(MemoryAgentTaskRepository::new()),
            agent_workflow_repo: Self::memory_agent_workflow_repo(),
            agent_conversation_issue_repo: Arc::new(MemoryAgentConversationIssueRepository::new()),
            ideation_settings_repo: Arc::new(MemoryIdeationSettingsRepository::with_settings(
                IdeationSettings {
                    tasks_enabled: true,
                    tasks_feature_state: TasksFeatureState::Enabled,
                    ..Default::default()
                },
            )),
            ideation_effort_settings_repo: Arc::new(MemoryIdeationEffortSettingsRepository::new()),
            ideation_model_settings_repo: Arc::new(MemoryIdeationModelSettingsRepository::new()),
            agent_lane_settings_repo: Arc::new(MemoryAgentLaneSettingsRepository::new()),
            manual_role_default_repo: Arc::new(MemoryManualRoleDefaultRepository::new()),
            mcp_policy_repo: Arc::new(MemoryMcpPolicyRepository::new()),
            agent_model_registry_repo: Arc::new(MemoryAgentModelRegistryRepository::new()),
            agent_provider_settings_repo: Arc::new(
                MemoryAgentProviderSettingsRepository::with_all_providers_enabled(
                    DEFAULT_AGENT_HARNESS,
                ),
            ),
            session_link_repo: Arc::new(MemorySessionLinkRepository::new()),
            task_proposal_repo: Arc::clone(&task_proposal_repo),
            proposal_dependency_repo: Arc::new(MemoryProposalDependencyRepository::new()),
            chat_message_repo: Arc::new(MemoryChatMessageRepository::new()),
            chat_timeline_repo: Arc::new(MemoryChatTimelineRepository::new()),
            chat_conversation_repo: Arc::new(MemoryChatConversationRepository::new()),
            persona_repo: Arc::new(MemoryPersonaRepository::new()),
            agent_conversation_workspace_repo,
            agent_workspace_repair_repo,
            agent_workspace_repair_publish_continuation: Arc::new(RwLock::new(None)),
            agent_workspace_pr_fix_review_publish_resumer: Arc::new(RwLock::new(None)),
            agent_conversation_jira_issue_repo: Arc::new(
                MemoryAgentConversationJiraIssueRepository::new(),
            ),
            agent_conversation_linear_issue_repo: Arc::new(
                MemoryAgentConversationLinearIssueRepository::new(),
            ),
            agent_conversation_granola_note_repo: Arc::new(
                MemoryAgentConversationGranolaNoteRepository::new(),
            ),
            agent_conversation_mute_repo: Arc::new(MemoryAgentConversationMuteRepository::new()),
            remote_conversation_start_request_repo: Arc::new(
                MemoryRemoteConversationStartRequestRepository::new(),
            ),
            remote_agent_stop_request_repo: Arc::new(MemoryRemoteAgentStopRequestRepository::new()),
            ticket_canonical_branch_repo: Arc::new(MemoryTicketCanonicalBranchRepository::new()),
            orphan_worktree_cleanup_marker_repo: Arc::new(
                MemoryOrphanWorktreeCleanupMarkerRepository::new(),
            ),
            automation_repo: Arc::new(MemoryAutomationRepository::with_shared_state(Arc::clone(
                &automation_state,
            ))),
            automation_run_repo: Arc::new(MemoryAutomationRunRepository::new(automation_state)),
            agent_terminal_service: Arc::new(AgentTerminalService::new()),
            agent_run_repo: Arc::new(MemoryAgentRunRepository::new()),
            activity_event_repo: Arc::new(MemoryActivityEventRepository::new()),
            notification_repo: Arc::new(MemoryNotificationRepository::new()),
            task_dependency_repo: Arc::new(MemoryTaskDependencyRepository::new()),
            // Extensibility repositories
            workflow_repo: Arc::new(MemoryWorkflowRepository::new()),
            artifact_repo: Arc::clone(&artifact_repo),
            artifact_bucket_repo: Arc::new(MemoryArtifactBucketRepository::new()),
            artifact_flow_repo: Arc::new(MemoryArtifactFlowRepository::new()),
            process_repo: Arc::new(MemoryProcessRepository::new()),
            methodology_repo: Arc::new(MemoryMethodologyRepository::new()),
            plan_branch_repo: Arc::new(MemoryPlanBranchRepository::new()),
            plan_selection_stats_repo: Arc::new(MemoryPlanSelectionStatsRepository::new()),
            app_state_repo: Arc::new(MemoryAppStateRepository::new()),
            active_plan_repo: Arc::new(MemoryActivePlanRepository::new()),
            memory_entry_repo: Arc::new(InMemoryMemoryEntryRepository::new()),
            memory_event_repo: Arc::new(InMemoryMemoryEventRepository::new()),
            memory_archive_repo: Arc::new(SqliteMemoryArchiveRepository::new(
                open_connection(&PathBuf::from(":memory:"))
                    .expect("Failed to create in-memory connection"),
            )),
            execution_plan_repo: Arc::new(MemoryExecutionPlanRepository::new()),
            chat_attachment_repo,
            conversation_folder_reference_repo: Arc::new(
                MemoryConversationFolderReferenceRepository::new(),
            ),
            attachment_storage_path,
            permission_state: Arc::new(PermissionState::with_repo(Arc::new(
                MemoryPermissionRepository::new(),
            ))),
            question_state: Arc::new(QuestionState::with_repo(Arc::new(
                MemoryQuestionRepository::new(),
            ))),
            message_queue: Arc::new(MessageQueue::new()),
            queued_message_repo: Arc::new(MemoryQueuedMessageRepository::new()),
            db: {
                let conn = open_connection(&std::path::PathBuf::from(":memory:"))
                    .expect("Failed to create in-memory connection for db field");
                conn.execute_batch(
                    "CREATE TABLE deferred_plan_approval_notifications (
                        session_id TEXT PRIMARY KEY NOT NULL,
                        artifact_id TEXT NOT NULL,
                        plan_target_id TEXT,
                        created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                    );",
                )
                .expect("Failed to create deferred plan approval marker table for tests");
                crate::infrastructure::sqlite::DbConnection::new(conn)
            },
            external_events_repo: Arc::new(MemoryExternalEventsRepository::new()),
            running_agent_registry: Arc::new(MemoryRunningAgentRegistry::new()),
            webhook_registration_repo: Arc::new(MemoryWebhookRegistrationRepository::new()),
            webhook_publisher: None,
            session_merge_locks: Arc::new(dashmap::DashMap::new()),
            plan_verification_locks: Arc::new(dashmap::DashMap::new()),
            plan_verification_admissions: Arc::new(dashmap::DashMap::new()),
            auto_accept_sessions: Arc::new(Mutex::new(HashSet::new())),
            startup_git_auth_recovery_state: Arc::new(StartupGitAuthRecoveryState::default()),
            startup_coordinator: Arc::new(StartupCoordinator::new()),

            streaming_state_cache: crate::application::chat_service::StreamingStateCache::new(),
            interactive_process_registry: Arc::new(
                crate::application::InteractiveProcessRegistry::new(),
            ),
            app_handle: None,
            events,
            internal_event_bus,
            app_paths,
            github_service: None,
            pr_poller_registry: Arc::new(PrPollerRegistry::new(
                None,
                Arc::new(MemoryPlanBranchRepository::new()),
            )),
        }
    }

    /// Swap the agent client to a different implementation
    pub fn with_agent_client(mut self, client: Arc<dyn AgenticClient>) -> Self {
        self.agent_clients.default_client = client;
        self
    }

    pub fn agent_client_bundle(&self) -> AgentClientBundle {
        self.agent_clients.clone()
    }

    /// Resolve the client for a specific harness, falling back to the default client.
    pub fn resolve_harness_agent_client(
        &self,
        harness: AgentHarnessKind,
    ) -> Arc<dyn AgenticClient> {
        self.agent_client_bundle().resolve(harness)
    }

    /// Swap the agent client used for a specific harness.
    pub fn with_harness_agent_client(
        mut self,
        harness: AgentHarnessKind,
        client: Arc<dyn AgenticClient>,
    ) -> Self {
        self.agent_clients = self.agent_clients.with_harness_client(harness, client);
        self
    }

    /// Swap the QA settings to custom settings
    pub fn with_qa_settings(mut self, settings: QASettings) -> Self {
        self.qa_settings = Arc::new(tokio::sync::RwLock::new(settings));
        self
    }

    /// Create a ResumeValidator for task resume validation
    pub fn resume_validator(&self) -> ResumeValidator {
        ResumeValidator::new(Arc::clone(&self.running_agent_registry))
            .with_interactive_process_registry(Arc::clone(&self.interactive_process_registry))
    }
}

#[cfg(test)]
#[path = "app_state_tests.rs"]
mod tests;
