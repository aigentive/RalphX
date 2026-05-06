// Application state container for dependency injection
// Holds repository trait objects that can be swapped for testing

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Manager, Runtime};
use tokio::sync::Mutex;

use super::services::PrPollerRegistry;
use crate::application::chat_service::AppChatService;
use crate::application::runtime_factory::{
    build_chat_service_from_deps, build_task_scheduler_from_deps,
    build_transition_service_from_deps, ChatRuntimeFactoryDeps, RuntimeFactoryDeps,
};
use crate::application::startup_git_auth_preflight::StartupGitAuthRecoveryState;
use crate::application::AgentClientBundle;
use crate::application::AgentTerminalService;
use crate::application::PermissionState;
use crate::application::QuestionState;
use crate::application::ResumeValidator;
use crate::application::TaskSchedulerService;
use crate::application::TaskTransitionService;
use crate::commands::ExecutionState;
use crate::domain::agents::{AgentHarnessKind, AgenticClient, LogicalEffort};
use crate::domain::entities::ChatContextType;
use crate::domain::qa::QASettings;
use crate::domain::repositories::{
    ActivePlanRepository, ActivityEventRepository, AgentConversationWorkspaceRepository,
    AgentLaneSettingsRepository, AgentModelRegistryRepository, AgentProfileRepository,
    AgentRunRepository, ApiKeyRepository, AppStateRepository, ArtifactBucketRepository,
    ArtifactFlowRepository, ArtifactRepository, ChatAttachmentRepository,
    ChatConversationRepository, ChatMessageRepository, DelegatedSessionRepository,
    ExecutionPlanRepository, ExecutionSettingsRepository, ExternalEventsRepository,
    GlobalExecutionSettingsRepository, IdeationEffortSettingsRepository,
    IdeationModelSettingsRepository, IdeationSessionRepository, IdeationSettingsRepository,
    MemoryArchiveRepository, MemoryEntryRepository, MemoryEventRepository, MethodologyRepository,
    PlanBranchRepository, PlanSelectionStatsRepository, ProcessRepository, ProjectRepository,
    ProposalDependencyRepository, ReviewRepository, ReviewSettingsRepository,
    SessionLinkRepository, TaskDependencyRepository, TaskProposalRepository, TaskQARepository,
    TaskRepository, TaskStepRepository, TeamMessageRepository, TeamSessionRepository,
    WebhookRegistrationRepository, WorkflowRepository,
};
use crate::domain::services::{
    GithubServiceTrait, MemoryRunningAgentRegistry, MessageQueue, RunningAgentRegistry,
};
use crate::error::AppResult;
use crate::infrastructure::memory::{
    InMemoryMemoryEntryRepository, InMemoryMemoryEventRepository, MemoryActivePlanRepository,
    MemoryActivityEventRepository, MemoryAgentConversationWorkspaceRepository,
    MemoryAgentLaneSettingsRepository, MemoryAgentModelRegistryRepository,
    MemoryAgentProfileRepository, MemoryAgentRunRepository, MemoryApiKeyRepository,
    MemoryAppStateRepository, MemoryArtifactBucketRepository, MemoryArtifactFlowRepository,
    MemoryArtifactRepository, MemoryChatAttachmentRepository, MemoryChatConversationRepository,
    MemoryChatMessageRepository, MemoryDelegatedSessionRepository, MemoryExecutionPlanRepository,
    MemoryExecutionSettingsRepository, MemoryExternalEventsRepository,
    MemoryGlobalExecutionSettingsRepository, MemoryIdeationEffortSettingsRepository,
    MemoryIdeationModelSettingsRepository, MemoryIdeationSessionRepository,
    MemoryIdeationSettingsRepository, MemoryMethodologyRepository, MemoryPermissionRepository,
    MemoryPlanBranchRepository, MemoryPlanSelectionStatsRepository, MemoryProcessRepository,
    MemoryProjectRepository, MemoryProposalDependencyRepository, MemoryQuestionRepository,
    MemoryReviewIssueRepository, MemoryReviewRepository, MemoryReviewSettingsRepository,
    MemorySessionLinkRepository, MemoryTaskDependencyRepository, MemoryTaskProposalRepository,
    MemoryTaskQARepository, MemoryTaskRepository, MemoryTaskStepRepository,
    MemoryTeamMessageRepository, MemoryTeamSessionRepository, MemoryWebhookRegistrationRepository,
    MemoryWorkflowRepository,
};
use crate::infrastructure::sqlite::ReviewIssueRepository;
use crate::infrastructure::sqlite::{
    get_app_data_db_path, get_default_db_path, open_connection, run_migrations,
    SqliteActivePlanRepository, SqliteActivityEventRepository,
    SqliteAgentConversationWorkspaceRepository, SqliteAgentLaneSettingsRepository,
    SqliteAgentModelRegistryRepository, SqliteAgentProfileRepository, SqliteAgentRunRepository,
    SqliteApiKeyRepository, SqliteAppStateRepository, SqliteArtifactBucketRepository,
    SqliteArtifactFlowRepository, SqliteArtifactRepository, SqliteChatAttachmentRepository,
    SqliteChatConversationRepository, SqliteChatMessageRepository,
    SqliteDelegatedSessionRepository, SqliteExecutionPlanRepository,
    SqliteExecutionSettingsRepository, SqliteExternalEventsRepository,
    SqliteGlobalExecutionSettingsRepository, SqliteIdeationEffortSettingsRepository,
    SqliteIdeationModelSettingsRepository, SqliteIdeationSessionRepository,
    SqliteIdeationSettingsRepository, SqliteMemoryArchiveRepository, SqliteMemoryEntryRepository,
    SqliteMemoryEventRepository, SqliteMethodologyRepository, SqlitePermissionRepository,
    SqlitePlanBranchRepository, SqlitePlanSelectionStatsRepository, SqliteProcessRepository,
    SqliteProjectRepository, SqliteProposalDependencyRepository, SqliteQuestionRepository,
    SqliteReviewIssueRepository, SqliteReviewRepository, SqliteReviewSettingsRepository,
    SqliteRunningAgentRegistry, SqliteSessionLinkRepository, SqliteTaskDependencyRepository,
    SqliteTaskProposalRepository, SqliteTaskQARepository, SqliteTaskRepository,
    SqliteTaskStepRepository, SqliteTeamMessageRepository, SqliteTeamSessionRepository,
    SqliteWebhookRegistrationRepository, SqliteWorkflowRepository,
};
use crate::infrastructure::GhCliGithubService;

pub(crate) struct ResolvedBackgroundAgentRuntime {
    pub client: Arc<dyn AgenticClient>,
    pub harness: Option<AgentHarnessKind>,
    pub model: Option<String>,
    pub logical_effort: Option<LogicalEffort>,
    pub approval_policy: Option<String>,
    pub sandbox_mode: Option<String>,
}

/// Application state container for dependency injection
/// Holds repository trait objects that can be swapped for testing vs production
pub struct AppState {
    /// Task repository (SQLite in production, in-memory for tests)
    pub task_repo: Arc<dyn TaskRepository>,
    /// Task step repository for tracking execution progress
    pub task_step_repo: Arc<dyn TaskStepRepository>,
    /// Project repository (SQLite in production, in-memory for tests)
    pub project_repo: Arc<dyn ProjectRepository>,
    /// API key repository for external API authentication
    pub api_key_repo: Arc<dyn ApiKeyRepository>,
    /// Agent profile repository (SQLite in production)
    pub agent_profile_repo: Arc<dyn AgentProfileRepository>,
    /// TaskQA repository for QA artifacts
    pub task_qa_repo: Arc<dyn TaskQARepository>,
    /// Review repository for code reviews
    pub review_repo: Arc<dyn ReviewRepository>,
    /// Review settings repository
    pub review_settings_repo: Arc<dyn ReviewSettingsRepository>,
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
    /// Ideation settings repository
    pub ideation_settings_repo: Arc<dyn IdeationSettingsRepository>,
    /// Delegated specialist session repository
    pub delegated_session_repo: Arc<dyn DelegatedSessionRepository>,
    /// Ideation effort settings repository (global and per-project effort overrides)
    pub ideation_effort_settings_repo: Arc<dyn IdeationEffortSettingsRepository>,
    /// Ideation model settings repository (global and per-project model overrides)
    pub ideation_model_settings_repo: Arc<dyn IdeationModelSettingsRepository>,
    /// Provider-neutral lane settings repository for multi-harness routing
    pub agent_lane_settings_repo: Arc<dyn AgentLaneSettingsRepository>,
    /// Provider/model compatibility and custom model registry
    pub agent_model_registry_repo: Arc<dyn AgentModelRegistryRepository>,
    /// Session link repository for managing parent-child session relationships
    pub session_link_repo: Arc<dyn SessionLinkRepository>,
    /// Task proposal repository
    pub task_proposal_repo: Arc<dyn TaskProposalRepository>,
    /// Proposal dependency repository
    pub proposal_dependency_repo: Arc<dyn ProposalDependencyRepository>,
    /// Chat message repository
    pub chat_message_repo: Arc<dyn ChatMessageRepository>,
    /// Chat conversation repository (for context-aware chat)
    pub chat_conversation_repo: Arc<dyn ChatConversationRepository>,
    /// Conversation-owned branch/worktree repository for Agents starter workspaces
    pub agent_conversation_workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    /// In-memory PTY session manager for Agents conversation terminals
    pub agent_terminal_service: Arc<AgentTerminalService>,
    /// Agent run repository (for tracking Claude agent executions)
    pub agent_run_repo: Arc<dyn AgentRunRepository>,
    /// Activity event repository (for activity stream persistence)
    pub activity_event_repo: Arc<dyn ActivityEventRepository>,
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
    /// Team session repository for agent team history
    pub team_session_repo: Arc<dyn TeamSessionRepository>,
    /// Team message repository for agent team messages
    pub team_message_repo: Arc<dyn TeamMessageRepository>,
    /// Execution plan repository for tracking plan implementation attempts
    pub execution_plan_repo: Arc<dyn ExecutionPlanRepository>,
    /// Chat attachment repository for file uploads in chat
    pub chat_attachment_repo: Arc<dyn ChatAttachmentRepository>,
    /// Storage path for chat attachments
    pub attachment_storage_path: PathBuf,
    /// Streaming state cache for hydrating frontend on navigation to active conversations
    pub streaming_state_cache: crate::application::chat_service::StreamingStateCache,
    /// Interactive process registry for stdin-based multi-turn messaging
    pub interactive_process_registry: Arc<crate::application::InteractiveProcessRegistry>,
    /// Tauri app handle for emitting events to frontend (None in tests)
    pub app_handle: Option<AppHandle>,
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
    /// Sessions where user has enabled auto-accept for verification. Ephemeral.
    pub auto_accept_sessions: Arc<Mutex<HashSet<String>>>,
    /// Startup Git/GitHub recovery gate. Set when startup defers Git-dependent
    /// work and cleared after an explicit repair resumes that work.
    pub(crate) startup_git_auth_recovery_state: Arc<StartupGitAuthRecoveryState>,
}

impl AppState {
    fn production_agent_clients() -> AgentClientBundle {
        AgentClientBundle::standard_production_runtime_clients()
    }

    fn mock_agent_clients() -> AgentClientBundle {
        AgentClientBundle::standard_mock_runtime_clients()
    }

    fn enable_claude_test_mode() {
        std::env::set_var("RALPHX_TEST_MODE", "1");
    }

    pub fn build_chat_service(&self) -> AppChatService {
        self.build_chat_service_for_runtime(None, self.app_handle.clone())
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

    pub fn build_transition_service_for_runtime<R: Runtime>(
        &self,
        execution_state: Arc<ExecutionState>,
        app_handle: Option<AppHandle<R>>,
    ) -> TaskTransitionService<R> {
        let deps = RuntimeFactoryDeps::from_app_state(self);

        build_transition_service_from_deps(app_handle, execution_state, &deps)
    }

    pub fn build_task_scheduler_for_runtime<R: Runtime>(
        &self,
        execution_state: Arc<ExecutionState>,
        app_handle: Option<AppHandle<R>>,
    ) -> TaskSchedulerService<R> {
        let deps = RuntimeFactoryDeps::from_app_state(self);

        build_task_scheduler_from_deps(app_handle, execution_state, &deps)
    }

    pub(crate) async fn resolve_ideation_background_agent_runtime(
        &self,
        project_id: Option<&str>,
    ) -> AppResult<ResolvedBackgroundAgentRuntime> {
        let resolved = crate::application::agent_lane_resolution::resolve_agent_spawn_settings(
            crate::infrastructure::agents::claude::agent_names::AGENT_ORCHESTRATOR_IDEATION,
            project_id,
            ChatContextType::Ideation,
            None,
            None,
            None,
            Some(&self.agent_lane_settings_repo),
        )
        .await;

        if let Some(client) = self
            .agent_clients
            .explicit_available_harness_client(resolved.effective_harness)
            .await
        {
            return Ok(ResolvedBackgroundAgentRuntime {
                client,
                harness: Some(resolved.effective_harness),
                model: Some(resolved.model),
                logical_effort: resolved.logical_effort,
                approval_policy: resolved.approval_policy,
                sandbox_mode: resolved.sandbox_mode,
            });
        }

        if resolved.effective_harness != self.agent_clients.default_harness {
            return Err(crate::error::AppError::Infrastructure(format!(
                "Configured ideation sidecar harness unavailable for project {}: {}",
                project_id.unwrap_or(""),
                resolved.effective_harness
            )));
        }

        Ok(ResolvedBackgroundAgentRuntime {
            client: Arc::clone(&self.agent_clients.default_client),
            harness: None,
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        })
    }

    pub(crate) async fn resolve_session_namer_runtime(&self) -> ResolvedBackgroundAgentRuntime {
        ResolvedBackgroundAgentRuntime {
            client: Arc::clone(&self.agent_clients.default_client),
            harness: None,
            model: None,
            logical_effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }
    }

    /// Create AppState for production use with SQLite repositories.
    /// Opens the database at the default path and runs migrations.
    pub fn new_production(app_handle: AppHandle) -> AppResult<Self> {
        let path = if cfg!(debug_assertions) {
            get_default_db_path()
        } else {
            get_app_data_db_path(&app_handle)?
        };
        let conn = open_connection(&path)?;
        run_migrations(&conn)?;

        let shared_conn = Arc::new(Mutex::new(conn));
        Self::build_from_shared_conn(app_handle, shared_conn)
    }

    /// Create AppState sharing an existing DB connection (no new connection or migrations).
    /// Used by the HTTP/MCP server to share the Tauri AppState's physical SQLite connection.
    pub fn new_production_shared(
        app_handle: AppHandle,
        shared_conn: Arc<Mutex<rusqlite::Connection>>,
    ) -> AppResult<Self> {
        Self::build_from_shared_conn(app_handle, shared_conn)
    }

    /// Internal helper: build all SQLite repositories from a pre-existing shared connection.
    fn build_from_shared_conn(
        app_handle: AppHandle,
        shared_conn: Arc<Mutex<rusqlite::Connection>>,
    ) -> AppResult<Self> {
        // Create repositories that are used by services
        let task_repo: Arc<dyn TaskRepository> =
            Arc::new(SqliteTaskRepository::from_shared(Arc::clone(&shared_conn)));
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
        let attachment_storage_path = if cfg!(debug_assertions) {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        } else {
            app_handle
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
        };

        let gh_svc: Arc<dyn GithubServiceTrait> = Arc::new(GhCliGithubService::new());

        Ok(Self {
            task_repo: Arc::clone(&task_repo),
            task_step_repo: Arc::new(SqliteTaskStepRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            project_repo: Arc::new(SqliteProjectRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            api_key_repo: Arc::new(SqliteApiKeyRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
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
            review_issue_repo: Arc::new(SqliteReviewIssueRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            agent_clients: Self::production_agent_clients(),
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
            delegated_session_repo: Arc::new(SqliteDelegatedSessionRepository::from_shared(
                Arc::clone(&shared_conn),
            )),
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
            agent_model_registry_repo: Arc::new(SqliteAgentModelRegistryRepository::from_shared(
                Arc::clone(&shared_conn),
            )),
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
            chat_conversation_repo: Arc::new(SqliteChatConversationRepository::from_shared(
                Arc::clone(&shared_conn),
            )),
            agent_conversation_workspace_repo: Arc::new(
                SqliteAgentConversationWorkspaceRepository::from_shared(Arc::clone(&shared_conn)),
            ),
            agent_terminal_service: Arc::new(AgentTerminalService::new()),
            agent_run_repo: Arc::new(SqliteAgentRunRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            activity_event_repo: Arc::new(SqliteActivityEventRepository::from_shared(Arc::clone(
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
            team_session_repo: Arc::new(SqliteTeamSessionRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            team_message_repo: Arc::new(SqliteTeamMessageRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            execution_plan_repo: Arc::new(SqliteExecutionPlanRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            chat_attachment_repo,
            attachment_storage_path,
            permission_state: Arc::new(PermissionState::with_repo(Arc::new(
                SqlitePermissionRepository::from_shared(Arc::clone(&shared_conn)),
            ))),
            question_state: Arc::new(QuestionState::with_repo(Arc::new(
                SqliteQuestionRepository::from_shared(Arc::clone(&shared_conn)),
            ))),
            message_queue: Arc::new(MessageQueue::new()),
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
            auto_accept_sessions: Arc::new(Mutex::new(HashSet::new())),
            startup_git_auth_recovery_state: Arc::new(StartupGitAuthRecoveryState::default()),

            streaming_state_cache: crate::application::chat_service::StreamingStateCache::new(),
            interactive_process_registry: Arc::new(
                crate::application::InteractiveProcessRegistry::new(),
            ),
            app_handle: Some(app_handle),
        })
    }

    /// Create AppState with a specific database path
    pub fn with_db_path(db_path: &str, app_handle: AppHandle) -> AppResult<Self> {
        let path = PathBuf::from(db_path);
        let conn = open_connection(&path)?;
        run_migrations(&conn)?;
        let shared_conn = Arc::new(Mutex::new(conn));
        Self::build_from_shared_conn(app_handle, shared_conn)
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
        // Migrations may leave foreign_keys = ON. Disable for tests: we test handler logic,
        // not FK enforcement. Sessions reference projects that don't exist in the test DB.
        conn.execute("PRAGMA foreign_keys = OFF", [])
            .expect("Failed to disable foreign_keys for test DB");
        let shared_conn = Arc::new(tokio::sync::Mutex::new(conn));

        let chat_attachment_repo: Arc<dyn ChatAttachmentRepository> =
            Arc::new(MemoryChatAttachmentRepository::new());
        let attachment_storage_path = std::env::temp_dir();

        Self {
            task_repo: Arc::new(MemoryTaskRepository::new()),
            task_step_repo: Arc::new(MemoryTaskStepRepository::new()),
            project_repo: Arc::new(MemoryProjectRepository::new()),
            api_key_repo: Arc::new(MemoryApiKeyRepository::new()),
            agent_profile_repo: Arc::new(MemoryAgentProfileRepository::new()),
            task_qa_repo: Arc::new(MemoryTaskQARepository::new()),
            review_repo: Arc::new(MemoryReviewRepository::new()),
            review_settings_repo: Arc::new(MemoryReviewSettingsRepository::new()),
            review_issue_repo: Arc::new(MemoryReviewIssueRepository::new()),
            agent_clients: Self::mock_agent_clients(),
            qa_settings: Arc::new(tokio::sync::RwLock::new(QASettings::default())),
            execution_settings_repo: Arc::new(MemoryExecutionSettingsRepository::new()),
            global_execution_settings_repo: Arc::new(MemoryGlobalExecutionSettingsRepository::new()),
            ideation_session_repo: Arc::new(SqliteIdeationSessionRepository::from_shared(
                Arc::clone(&shared_conn),
            )),
            delegated_session_repo: Arc::new(SqliteDelegatedSessionRepository::from_shared(
                Arc::clone(&shared_conn),
            )),
            ideation_settings_repo: Arc::new(MemoryIdeationSettingsRepository::new()),
            ideation_effort_settings_repo: Arc::new(MemoryIdeationEffortSettingsRepository::new()),
            ideation_model_settings_repo: Arc::new(MemoryIdeationModelSettingsRepository::new()),
            agent_lane_settings_repo: Arc::new(MemoryAgentLaneSettingsRepository::new()),
            agent_model_registry_repo: Arc::new(MemoryAgentModelRegistryRepository::new()),
            session_link_repo: Arc::new(MemorySessionLinkRepository::new()),
            task_proposal_repo: Arc::new(SqliteTaskProposalRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            proposal_dependency_repo: Arc::new(MemoryProposalDependencyRepository::new()),
            chat_message_repo: Arc::new(MemoryChatMessageRepository::new()),
            chat_conversation_repo: Arc::new(MemoryChatConversationRepository::new()),
            agent_conversation_workspace_repo: Arc::new(
                MemoryAgentConversationWorkspaceRepository::new(),
            ),
            agent_terminal_service: Arc::new(AgentTerminalService::new()),
            agent_run_repo: Arc::new(MemoryAgentRunRepository::new()),
            activity_event_repo: Arc::new(MemoryActivityEventRepository::new()),
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
            team_session_repo: Arc::new(MemoryTeamSessionRepository::new()),
            team_message_repo: Arc::new(MemoryTeamMessageRepository::new()),
            execution_plan_repo: Arc::new(MemoryExecutionPlanRepository::new()),
            chat_attachment_repo,
            attachment_storage_path,
            permission_state: Arc::new(PermissionState::with_repo(Arc::new(
                MemoryPermissionRepository::new(),
            ))),
            question_state: Arc::new(QuestionState::with_repo(Arc::new(
                MemoryQuestionRepository::new(),
            ))),
            message_queue: Arc::new(MessageQueue::new()),
            db: crate::infrastructure::sqlite::DbConnection::from_shared(Arc::clone(&shared_conn)),
            external_events_repo: Arc::new(MemoryExternalEventsRepository::new()),
            running_agent_registry: Arc::new(MemoryRunningAgentRegistry::new()),
            webhook_registration_repo: Arc::new(MemoryWebhookRegistrationRepository::new()),
            webhook_publisher: None,
            session_merge_locks: Arc::new(dashmap::DashMap::new()),
            auto_accept_sessions: Arc::new(Mutex::new(HashSet::new())),
            startup_git_auth_recovery_state: Arc::new(StartupGitAuthRecoveryState::default()),

            streaming_state_cache: crate::application::chat_service::StreamingStateCache::new(),
            interactive_process_registry: Arc::new(
                crate::application::InteractiveProcessRegistry::new(),
            ),
            app_handle: None,
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
        conn.execute("PRAGMA foreign_keys = OFF", [])
            .expect("Failed to disable foreign_keys for test DB");
        let shared_conn = Arc::new(tokio::sync::Mutex::new(conn));

        let chat_attachment_repo: Arc<dyn ChatAttachmentRepository> =
            Arc::new(MemoryChatAttachmentRepository::new());
        let attachment_storage_path = std::env::temp_dir();

        Self {
            task_repo: Arc::new(MemoryTaskRepository::new()),
            task_step_repo: Arc::new(MemoryTaskStepRepository::new()),
            project_repo: Arc::new(MemoryProjectRepository::new()),
            api_key_repo: Arc::new(MemoryApiKeyRepository::new()),
            agent_profile_repo: Arc::new(MemoryAgentProfileRepository::new()),
            task_qa_repo: Arc::new(MemoryTaskQARepository::new()),
            review_repo: Arc::new(MemoryReviewRepository::new()),
            review_settings_repo: Arc::new(MemoryReviewSettingsRepository::new()),
            review_issue_repo: Arc::new(MemoryReviewIssueRepository::new()),
            agent_clients: Self::mock_agent_clients(),
            qa_settings: Arc::new(tokio::sync::RwLock::new(QASettings::default())),
            execution_settings_repo: Arc::new(MemoryExecutionSettingsRepository::new()),
            global_execution_settings_repo: Arc::new(MemoryGlobalExecutionSettingsRepository::new()),
            ideation_session_repo: Arc::new(SqliteIdeationSessionRepository::from_shared(
                Arc::clone(&shared_conn),
            )),
            delegated_session_repo: Arc::new(SqliteDelegatedSessionRepository::from_shared(
                Arc::clone(&shared_conn),
            )),
            ideation_settings_repo: Arc::new(MemoryIdeationSettingsRepository::new()),
            ideation_effort_settings_repo: Arc::new(MemoryIdeationEffortSettingsRepository::new()),
            ideation_model_settings_repo: Arc::new(MemoryIdeationModelSettingsRepository::new()),
            agent_lane_settings_repo: Arc::new(MemoryAgentLaneSettingsRepository::new()),
            agent_model_registry_repo: Arc::new(MemoryAgentModelRegistryRepository::new()),
            session_link_repo: Arc::new(MemorySessionLinkRepository::new()),
            task_proposal_repo: Arc::new(SqliteTaskProposalRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            proposal_dependency_repo: Arc::new(MemoryProposalDependencyRepository::new()),
            chat_message_repo: Arc::new(MemoryChatMessageRepository::new()),
            chat_conversation_repo: Arc::new(MemoryChatConversationRepository::new()),
            agent_conversation_workspace_repo: Arc::new(
                MemoryAgentConversationWorkspaceRepository::new(),
            ),
            agent_terminal_service: Arc::new(AgentTerminalService::new()),
            agent_run_repo: Arc::new(MemoryAgentRunRepository::new()),
            activity_event_repo: Arc::new(MemoryActivityEventRepository::new()),
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
            team_session_repo: Arc::new(MemoryTeamSessionRepository::new()),
            team_message_repo: Arc::new(MemoryTeamMessageRepository::new()),
            execution_plan_repo: Arc::new(MemoryExecutionPlanRepository::new()),
            chat_attachment_repo,
            attachment_storage_path,
            permission_state: Arc::new(PermissionState::with_repo(Arc::new(
                MemoryPermissionRepository::new(),
            ))),
            question_state: Arc::new(QuestionState::with_repo(Arc::new(
                MemoryQuestionRepository::new(),
            ))),
            message_queue: Arc::new(MessageQueue::new()),
            db: crate::infrastructure::sqlite::DbConnection::from_shared(Arc::clone(&shared_conn)),
            external_events_repo: Arc::new(MemoryExternalEventsRepository::new()),
            running_agent_registry: registry,
            webhook_registration_repo: Arc::new(MemoryWebhookRegistrationRepository::new()),
            webhook_publisher: None,
            session_merge_locks: Arc::new(dashmap::DashMap::new()),
            auto_accept_sessions: Arc::new(Mutex::new(HashSet::new())),
            startup_git_auth_recovery_state: Arc::new(StartupGitAuthRecoveryState::default()),

            streaming_state_cache: crate::application::chat_service::StreamingStateCache::new(),
            interactive_process_registry: Arc::new(
                crate::application::InteractiveProcessRegistry::new(),
            ),
            app_handle: None,
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
    /// - `plan_branch_repo`, `project_repo`, `db`
    #[doc(hidden)]
    pub fn new_sqlite_for_apply_test() -> Self {
        Self::enable_claude_test_mode();
        let conn = open_connection(&std::path::PathBuf::from(":memory:"))
            .expect("Failed to open in-memory SQLite for apply_proposals_core tests");
        run_migrations(&conn).expect("Failed to run migrations on in-memory test DB");
        conn.execute("PRAGMA foreign_keys = OFF", [])
            .expect("Failed to disable foreign_keys for test DB");
        let shared_conn = Arc::new(tokio::sync::Mutex::new(conn));

        let chat_attachment_repo: Arc<dyn ChatAttachmentRepository> =
            Arc::new(MemoryChatAttachmentRepository::new());
        let attachment_storage_path = std::env::temp_dir();

        Self {
            task_repo: Arc::new(SqliteTaskRepository::from_shared(Arc::clone(&shared_conn))),
            task_step_repo: Arc::new(SqliteTaskStepRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            project_repo: Arc::new(SqliteProjectRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            api_key_repo: Arc::new(MemoryApiKeyRepository::new()),
            agent_profile_repo: Arc::new(MemoryAgentProfileRepository::new()),
            task_qa_repo: Arc::new(MemoryTaskQARepository::new()),
            review_repo: Arc::new(MemoryReviewRepository::new()),
            review_settings_repo: Arc::new(MemoryReviewSettingsRepository::new()),
            review_issue_repo: Arc::new(MemoryReviewIssueRepository::new()),
            agent_clients: Self::mock_agent_clients(),
            qa_settings: Arc::new(tokio::sync::RwLock::new(QASettings::default())),
            execution_settings_repo: Arc::new(MemoryExecutionSettingsRepository::new()),
            global_execution_settings_repo: Arc::new(MemoryGlobalExecutionSettingsRepository::new()),
            ideation_session_repo: Arc::new(SqliteIdeationSessionRepository::from_shared(
                Arc::clone(&shared_conn),
            )),
            delegated_session_repo: Arc::new(SqliteDelegatedSessionRepository::from_shared(
                Arc::clone(&shared_conn),
            )),
            ideation_settings_repo: Arc::new(MemoryIdeationSettingsRepository::new()),
            ideation_effort_settings_repo: Arc::new(MemoryIdeationEffortSettingsRepository::new()),
            ideation_model_settings_repo: Arc::new(MemoryIdeationModelSettingsRepository::new()),
            agent_lane_settings_repo: Arc::new(MemoryAgentLaneSettingsRepository::new()),
            agent_model_registry_repo: Arc::new(MemoryAgentModelRegistryRepository::new()),
            session_link_repo: Arc::new(MemorySessionLinkRepository::new()),
            task_proposal_repo: Arc::new(SqliteTaskProposalRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            proposal_dependency_repo: Arc::new(SqliteProposalDependencyRepository::from_shared(
                Arc::clone(&shared_conn),
            )),
            chat_message_repo: Arc::new(MemoryChatMessageRepository::new()),
            chat_conversation_repo: Arc::new(MemoryChatConversationRepository::new()),
            agent_conversation_workspace_repo: Arc::new(
                SqliteAgentConversationWorkspaceRepository::from_shared(Arc::clone(&shared_conn)),
            ),
            agent_terminal_service: Arc::new(AgentTerminalService::new()),
            agent_run_repo: Arc::new(MemoryAgentRunRepository::new()),
            activity_event_repo: Arc::new(MemoryActivityEventRepository::new()),
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
            active_plan_repo: Arc::new(MemoryActivePlanRepository::new()),
            memory_entry_repo: Arc::new(InMemoryMemoryEntryRepository::new()),
            memory_event_repo: Arc::new(InMemoryMemoryEventRepository::new()),
            memory_archive_repo: Arc::new(SqliteMemoryArchiveRepository::new(
                open_connection(&std::path::PathBuf::from(":memory:"))
                    .expect("Failed to create in-memory connection for memory_archive"),
            )),
            team_session_repo: Arc::new(MemoryTeamSessionRepository::new()),
            team_message_repo: Arc::new(MemoryTeamMessageRepository::new()),
            execution_plan_repo: Arc::new(SqliteExecutionPlanRepository::from_shared(Arc::clone(
                &shared_conn,
            ))),
            chat_attachment_repo,
            attachment_storage_path,
            permission_state: Arc::new(PermissionState::with_repo(Arc::new(
                MemoryPermissionRepository::new(),
            ))),
            question_state: Arc::new(QuestionState::with_repo(Arc::new(
                MemoryQuestionRepository::new(),
            ))),
            message_queue: Arc::new(MessageQueue::new()),
            db: crate::infrastructure::sqlite::DbConnection::from_shared(Arc::clone(&shared_conn)),
            external_events_repo: Arc::new(MemoryExternalEventsRepository::new()),
            running_agent_registry: Arc::new(MemoryRunningAgentRegistry::new()),
            webhook_registration_repo: Arc::new(MemoryWebhookRegistrationRepository::new()),
            webhook_publisher: None,
            session_merge_locks: Arc::new(dashmap::DashMap::new()),
            auto_accept_sessions: Arc::new(Mutex::new(HashSet::new())),
            startup_git_auth_recovery_state: Arc::new(StartupGitAuthRecoveryState::default()),

            streaming_state_cache: crate::application::chat_service::StreamingStateCache::new(),
            interactive_process_registry: Arc::new(
                crate::application::InteractiveProcessRegistry::new(),
            ),
            app_handle: None,
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
        let attachment_storage_path = std::env::temp_dir();

        Self {
            task_repo: Arc::clone(&task_repo),
            task_step_repo: Arc::new(MemoryTaskStepRepository::new()),
            project_repo,
            api_key_repo: Arc::new(MemoryApiKeyRepository::new()),
            agent_profile_repo: Arc::new(MemoryAgentProfileRepository::new()),
            task_qa_repo: Arc::new(MemoryTaskQARepository::new()),
            review_repo: Arc::new(MemoryReviewRepository::new()),
            review_settings_repo: Arc::new(MemoryReviewSettingsRepository::new()),
            review_issue_repo: Arc::new(MemoryReviewIssueRepository::new()),
            agent_clients: Self::mock_agent_clients(),
            qa_settings: Arc::new(tokio::sync::RwLock::new(QASettings::default())),
            execution_settings_repo: Arc::new(MemoryExecutionSettingsRepository::new()),
            global_execution_settings_repo: Arc::new(MemoryGlobalExecutionSettingsRepository::new()),
            ideation_session_repo: Arc::new(MemoryIdeationSessionRepository::new()),
            delegated_session_repo: Arc::new(MemoryDelegatedSessionRepository::new()),
            ideation_settings_repo: Arc::new(MemoryIdeationSettingsRepository::new()),
            ideation_effort_settings_repo: Arc::new(MemoryIdeationEffortSettingsRepository::new()),
            ideation_model_settings_repo: Arc::new(MemoryIdeationModelSettingsRepository::new()),
            agent_lane_settings_repo: Arc::new(MemoryAgentLaneSettingsRepository::new()),
            agent_model_registry_repo: Arc::new(MemoryAgentModelRegistryRepository::new()),
            session_link_repo: Arc::new(MemorySessionLinkRepository::new()),
            task_proposal_repo: Arc::clone(&task_proposal_repo),
            proposal_dependency_repo: Arc::new(MemoryProposalDependencyRepository::new()),
            chat_message_repo: Arc::new(MemoryChatMessageRepository::new()),
            chat_conversation_repo: Arc::new(MemoryChatConversationRepository::new()),
            agent_conversation_workspace_repo: Arc::new(
                MemoryAgentConversationWorkspaceRepository::new(),
            ),
            agent_terminal_service: Arc::new(AgentTerminalService::new()),
            agent_run_repo: Arc::new(MemoryAgentRunRepository::new()),
            activity_event_repo: Arc::new(MemoryActivityEventRepository::new()),
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
            team_session_repo: Arc::new(MemoryTeamSessionRepository::new()),
            team_message_repo: Arc::new(MemoryTeamMessageRepository::new()),
            execution_plan_repo: Arc::new(MemoryExecutionPlanRepository::new()),
            chat_attachment_repo,
            attachment_storage_path,
            permission_state: Arc::new(PermissionState::with_repo(Arc::new(
                MemoryPermissionRepository::new(),
            ))),
            question_state: Arc::new(QuestionState::with_repo(Arc::new(
                MemoryQuestionRepository::new(),
            ))),
            message_queue: Arc::new(MessageQueue::new()),
            db: crate::infrastructure::sqlite::DbConnection::new(
                open_connection(&std::path::PathBuf::from(":memory:"))
                    .expect("Failed to create in-memory connection for db field"),
            ),
            external_events_repo: Arc::new(MemoryExternalEventsRepository::new()),
            running_agent_registry: Arc::new(MemoryRunningAgentRegistry::new()),
            webhook_registration_repo: Arc::new(MemoryWebhookRegistrationRepository::new()),
            webhook_publisher: None,
            session_merge_locks: Arc::new(dashmap::DashMap::new()),
            auto_accept_sessions: Arc::new(Mutex::new(HashSet::new())),
            startup_git_auth_recovery_state: Arc::new(StartupGitAuthRecoveryState::default()),

            streaming_state_cache: crate::application::chat_service::StreamingStateCache::new(),
            interactive_process_registry: Arc::new(
                crate::application::InteractiveProcessRegistry::new(),
            ),
            app_handle: None,
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
