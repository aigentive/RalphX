// Unified Chat Service
//
// Consolidates OrchestratorService and ExecutionChatService into a single service
// with consistent patterns:
// - Background spawn pattern for ALL contexts (returns immediately, processes in background)
// - Unified event namespace: agent:* instead of chat:*/execution:*
// - Backend message queue with context-aware routing
// - Task state transitions only for TaskExecution context
//
// This service replaces both:
// - OrchestratorService (ideation, task, project contexts)
// - ExecutionChatService (task_execution context)

pub(crate) mod chat_service_context;
mod chat_service_errors;
mod chat_service_handlers;
mod chat_service_helpers;
mod chat_service_merge;
mod chat_service_mock;
mod chat_service_queue;
mod chat_service_recovery;
mod chat_service_replay;
mod chat_service_repository;
mod chat_service_send_background;
mod chat_service_streaming;
mod chat_service_types;
pub mod freshness_routing;
mod streaming_state_cache;
pub(crate) mod verification_child_process_registry;

use crate::application::agent_conversation_workspace::{
    is_terminal_agent_conversation_publication_status,
    resolve_valid_agent_conversation_workspace_path, rollover_agent_conversation_workspace,
    AGENT_CONVERSATION_WORKSPACE_CONTINUATION_MESSAGE,
};
use crate::application::harness_runtime_registry::{
    default_harness_runtime_available, resolve_chat_service_bootstrap,
    resolve_default_chat_service_bootstrap, resolve_harness_plugin_dir,
};
use crate::application::interactive_process_registry::{
    InteractiveProcessKey, InteractiveProcessMetadata, InteractiveProcessRegistry,
};
use crate::application::question_state::QuestionState;
use crate::domain::agents::{AgentHarnessKind, LogicalEffort, DEFAULT_AGENT_HARNESS};
use crate::domain::entities::ideation::SessionPurpose;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentConversationWorkspaceStatus,
    AgentRun, AgentRunId, AgentRunStatus, ChatContextType, ChatConversation, ChatConversationId,
    ChatMessage, ChatMessageId, IdeationSessionId, InternalStatus, MessageRole, ProjectId, TaskId,
};
use crate::domain::repositories::{
    ActivityEventRepository, AgentConversationWorkspaceRepository, AgentLaneSettingsRepository,
    AgentRunRepository, ArtifactRepository, ChatAttachmentRepository, ChatConversationRepository,
    ChatMessageRepository, DelegatedSessionRepository, ExecutionSettingsRepository,
    IdeationEffortSettingsRepository, IdeationModelSettingsRepository, IdeationSessionRepository,
    MemoryEventRepository, PlanBranchRepository, ProjectRepository, ReviewRepository,
    StateHistoryMetadata, TaskDependencyRepository, TaskProposalRepository, TaskRepository,
    TaskStepRepository,
};
use crate::domain::services::{
    is_process_alive, kill_process, MessageQueue, QueuedMessage, RunningAgentInfo, RunningAgentKey,
    RunningAgentRegistry,
};
use crate::infrastructure::agents::claude::agent_names::{
    AGENT_CHAT_PROJECT, AGENT_GENERAL_EXPLORER, AGENT_GENERAL_WORKER,
};
use async_trait::async_trait;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Runtime};
use tokio_util::sync::CancellationToken;

/// Prefix used when formatting agent errors into chat messages.
/// Both the write site (chat_service_handlers) and read site (chat_service_replay)
/// must use this constant to stay in sync.
pub const AGENT_ERROR_PREFIX: &str = "[Agent error:";
const REGISTRY_PID_ZERO_GRACE_SECONDS: i64 = 30;

// Re-exports from extracted modules
#[doc(hidden)]
pub use chat_service_context::create_assistant_message;
pub use chat_service_context::{
    build_command, build_initial_prompt, build_resume_command, build_resume_command_for_harness,
    build_resume_initial_prompt, format_attachments_for_agent, format_session_history,
    get_entity_status_for_resume, is_text_file, provider_resume_mode_for_session_under,
    resolve_working_directory, ProviderResumeMode,
};
pub use chat_service_errors::{
    classify_agent_error, classify_provider_error, parse_retry_after_from_message,
    truncate_error_message, PauseReason, ProviderErrorCategory, ProviderErrorMetadata, StreamError,
    STALE_SESSION_ERROR,
};
pub use chat_service_helpers::harness_supports_team_mode;
pub use chat_service_helpers::{
    context_type_to_process, get_agent_name, get_assistant_role, resolve_agent_with_team_mode,
};
pub use chat_service_merge::{
    merge_completion_watcher_loop, resolve_watcher_context, verify_merge_on_target,
    AutoCompleteGuard, MergeVerification,
};
pub(crate) use chat_service_merge::{reconcile_merge_auto_complete, MergeAutoCompleteContext};
pub use chat_service_mock::{MockChatResponse, MockChatService};
pub use chat_service_replay::{build_rehydration_prompt, ConversationReplay, ReplayBuilder, Turn};
#[doc(hidden)]
pub use chat_service_send_background::finalize_assistant_message_for_test;
#[doc(hidden)]
pub use chat_service_send_background::finalize_structured_assistant_message_for_test;
pub use chat_service_streaming::process_stream_background;
pub use chat_service_streaming::{
    is_completion_tool_name, should_kill_on_timeout, ActiveTaskTracker, CompletionSignalTracker,
    StreamOutcome, StreamTimeoutConfig,
};
pub use chat_service_types::events::AGENT_MESSAGE_QUEUED;
pub use chat_service_types::{
    events, AgentChunkPayload, AgentConversationCreatedPayload, AgentErrorPayload,
    AgentHookPayload, AgentMessageCreatedPayload, AgentMessageQueuedPayload, AgentQueueSentPayload,
    AgentRunCompletedPayload, AgentRunStartedPayload, AgentTaskCompletedPayload,
    AgentTaskStartedPayload, AgentToolCallPayload, ChatConversationWithMessages, ChatServiceError,
    SendCallerContext, SendResult, TeamArtifactCreatedPayload, TeamCostUpdatePayload,
    TeamCreatedPayload, TeamDisbandedPayload, TeamMessagePayload, TeamTeammateIdlePayload,
    TeamTeammateShutdownPayload, TeamTeammateSpawnedPayload,
};
pub use streaming_state_cache::{
    CachedStreamingTask, CachedToolCall, ConversationStreamingState, StreamingStateCache,
};

// Types and errors are now in chat_service_types.rs

/// Shared definition for "meaningful" agent output used by streaming and
/// background completion logic.
pub(crate) fn has_meaningful_output(
    response_text: &str,
    tool_call_count: usize,
    stderr_text: &str,
) -> bool {
    if tool_call_count > 0 {
        return true;
    }
    if chat_service_errors::classify_provider_error(response_text).is_some() {
        return false;
    }
    if !response_text.trim().is_empty() {
        return true;
    }
    // If stderr has content and no response/tool calls, the agent did not
    // produce meaningful work for the UI to show.
    if !stderr_text.trim().is_empty() {
        return false;
    }
    false
}

fn registry_entry_blocks_send_but_is_stale(
    info: &RunningAgentInfo,
    now: chrono::DateTime<chrono::Utc>,
) -> bool {
    if info.pid == 0 {
        let age = now.signed_duration_since(info.started_at);
        return age >= chrono::Duration::seconds(REGISTRY_PID_ZERO_GRACE_SECONDS);
    }

    !is_process_alive(info.pid)
}

fn registry_entry_blocks_send_because_run_inactive(
    info: &RunningAgentInfo,
    run_status: Option<AgentRunStatus>,
    now: chrono::DateTime<chrono::Utc>,
) -> bool {
    if info.agent_run_id.is_empty() {
        return false;
    }

    match run_status {
        Some(AgentRunStatus::Running) => false,
        Some(_) => true,
        None => {
            let age = now.signed_duration_since(info.started_at);
            age >= chrono::Duration::seconds(REGISTRY_PID_ZERO_GRACE_SECONDS)
        }
    }
}

fn resume_in_place_requested(metadata: Option<&str>) -> bool {
    metadata
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .and_then(|value| value.get("resume_in_place").and_then(|v| v.as_bool()))
        .unwrap_or(false)
}

fn strip_resume_in_place_metadata(metadata: Option<String>) -> Option<String> {
    let raw = metadata?;
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Some(raw);
    };
    let Some(obj) = value.as_object_mut() else {
        return Some(raw);
    };
    obj.remove("resume_in_place");
    if obj.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn runtime_context_id_for_send(
    context_type: ChatContextType,
    context_id: &str,
    conversation_id_override: Option<&ChatConversationId>,
) -> String {
    if context_type == ChatContextType::Project {
        if let Some(conversation_id) = conversation_id_override {
            return conversation_id.as_str().to_string();
        }
    }

    context_id.to_string()
}

/// Returns true for context types that consume execution slots (running count).
/// TaskExecution, Review, Merge, and Ideation are tracked against max_concurrent.
#[doc(hidden)]
pub fn uses_execution_slot(context_type: ChatContextType) -> bool {
    matches!(
        context_type,
        ChatContextType::TaskExecution
            | ChatContextType::Review
            | ChatContextType::Merge
            | ChatContextType::Ideation
    )
}

fn claude_launches_paused(
    context_type: ChatContextType,
    execution_state: Option<&Arc<crate::commands::ExecutionState>>,
) -> bool {
    matches!(
        context_type,
        ChatContextType::TaskExecution
            | ChatContextType::Review
            | ChatContextType::Merge
            | ChatContextType::Ideation
            | ChatContextType::Task
            | ChatContextType::Project
    ) && execution_state.is_some_and(|exec| exec.is_paused())
}

fn is_ideation_registry_context(context_type: &str) -> bool {
    context_type == "ideation" || context_type == "session"
}

/// Shared event payload context used by background and streaming modules.
#[derive(Debug, Clone)]
pub(crate) struct EventContextPayload {
    pub conversation_id: String,
    pub context_type: String,
    pub context_id: String,
}

pub(crate) fn event_context(
    conversation_id: &ChatConversationId,
    context_type: &ChatContextType,
    context_id: &str,
) -> EventContextPayload {
    EventContextPayload {
        conversation_id: conversation_id.as_str().to_string(),
        context_type: context_type.to_string(),
        context_id: context_id.to_string(),
    }
}

fn interactive_run_started_provider_session(
    conversation: &ChatConversation,
    process_metadata: Option<&InteractiveProcessMetadata>,
) -> (AgentHarnessKind, Option<String>) {
    let conversation_session_ref = conversation.provider_session_ref();
    let harness = process_metadata
        .and_then(|metadata| metadata.harness)
        .or_else(|| {
            conversation_session_ref
                .as_ref()
                .map(|session_ref| session_ref.harness)
        })
        .unwrap_or(DEFAULT_AGENT_HARNESS);
    let provider_session_id = process_metadata
        .and_then(|metadata| metadata.provider_session_id.clone())
        .or_else(|| {
            conversation_session_ref
                .as_ref()
                .map(|session_ref| session_ref.provider_session_id.clone())
        });

    (harness, provider_session_id)
}

fn agent_name_for_conversation_mode(mode: AgentConversationWorkspaceMode) -> &'static str {
    match mode {
        AgentConversationWorkspaceMode::Chat => AGENT_GENERAL_EXPLORER,
        AgentConversationWorkspaceMode::Edit => AGENT_GENERAL_WORKER,
        AgentConversationWorkspaceMode::Ideation => AGENT_CHAT_PROJECT,
    }
}

fn resolve_agent_name_for_send<'a>(
    context_type: &ChatContextType,
    entity_status: Option<&'a str>,
    team_mode: bool,
    agent_name_override: Option<&'a str>,
    agent_conversation_mode: Option<AgentConversationWorkspaceMode>,
) -> &'a str {
    agent_name_override
        .or_else(|| agent_conversation_mode.map(agent_name_for_conversation_mode))
        .unwrap_or_else(|| {
            chat_service_helpers::resolve_agent_with_team_mode(
                context_type,
                entity_status,
                team_mode,
            )
        })
}

fn continuation_metadata_requests_lineage(task_metadata: Option<&str>) -> bool {
    let metadata = task_metadata
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    let trigger_origin = metadata
        .get("trigger_origin")
        .and_then(|value| value.as_str());

    matches!(trigger_origin, Some("recovery" | "resume"))
        || metadata
            .get("startup_recovery_attempts")
            .and_then(|value| value.as_u64())
            .unwrap_or(0)
            > 0
}

fn should_inherit_parent_harness_for_fresh_spawn(
    context_type: ChatContextType,
    task_metadata: Option<&str>,
) -> bool {
    matches!(
        context_type,
        ChatContextType::TaskExecution | ChatContextType::Merge
    ) && continuation_metadata_requests_lineage(task_metadata)
}

fn spawn_settings_require_task_metadata(context_type: ChatContextType) -> bool {
    matches!(
        context_type,
        ChatContextType::TaskExecution | ChatContextType::Review | ChatContextType::Merge
    )
}

fn conversation_spawn_harness_override(
    agent_name: &str,
    context_type: ChatContextType,
    task_metadata: Option<&str>,
    conversation: &ChatConversation,
    parent_conversation: Option<&ChatConversation>,
) -> Option<AgentHarnessKind> {
    let review_reviewer_agent = context_type == ChatContextType::Review
        && agent_name == get_agent_name(&ChatContextType::Review);

    conversation
        .provider_session_ref()
        .and_then(|session_ref| {
            if review_reviewer_agent && !continuation_metadata_requests_lineage(task_metadata) {
                None
            } else {
                Some(session_ref.harness)
            }
        })
        .or_else(|| {
            if should_inherit_parent_harness_for_fresh_spawn(context_type, task_metadata) {
                parent_conversation.and_then(|parent| {
                    parent
                        .provider_session_ref()
                        .map(|session_ref| session_ref.harness)
                })
            } else {
                None
            }
        })
}

fn apply_send_message_overrides(
    resolved: &mut crate::application::agent_lane_resolution::ResolvedAgentSpawnSettings,
    options: &SendMessageOptions,
) {
    if let Some(model_override) = options.model_override.as_ref() {
        resolved.configured_model = Some(model_override.clone());
        resolved.model = model_override.clone();
    }

    if let Some(logical_effort_override) = options.logical_effort_override {
        resolved.configured_logical_effort = Some(logical_effort_override);
        resolved.logical_effort = Some(logical_effort_override);
        resolved.claude_effort = Some(
            logical_effort_override
                .to_legacy_claude_effort()
                .to_string(),
        );
    }

    if let Some(approval_policy_override) = options.approval_policy_override.as_ref() {
        resolved.configured_approval_policy = Some(approval_policy_override.clone());
        resolved.approval_policy = Some(approval_policy_override.clone());
    }

    if let Some(sandbox_mode_override) = options.sandbox_mode_override.as_ref() {
        resolved.configured_sandbox_mode = Some(sandbox_mode_override.clone());
        resolved.sandbox_mode = Some(sandbox_mode_override.clone());
    }
}

// ============================================================================
// ChatService trait
// ============================================================================

/// Options for customizing message sending behavior.
#[derive(Debug, Default, Clone)]
pub struct SendMessageOptions {
    /// Optional JSON metadata string to attach to the user message.
    pub metadata: Option<String>,
    /// Optional timestamp override for the user message. If None, uses Utc::now().
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Optional provider harness override for relaunch/recovery flows that must preserve an
    /// existing provider session's runtime instead of re-resolving only from current lane config.
    pub harness_override: Option<AgentHarnessKind>,
    /// Optional explicit canonical agent override for this send.
    pub agent_name_override: Option<String>,
    /// Optional explicit model override for this send.
    pub model_override: Option<String>,
    /// Optional conversation override for surfaces that own explicit session selection.
    pub conversation_id_override: Option<ChatConversationId>,
    /// Optional internal working-directory override for orchestrated maintenance
    /// flows that must run in a resolved publish target instead of the
    /// conversation's default workspace path.
    pub working_directory_override: Option<PathBuf>,
    /// Optional explicit logical-effort override for this send.
    pub logical_effort_override: Option<LogicalEffort>,
    /// Optional explicit approval-policy override for this send.
    pub approval_policy_override: Option<String>,
    /// Optional explicit sandbox-mode override for this send.
    pub sandbox_mode_override: Option<String>,
    /// Start a fresh provider-native session even when the conversation has a stored
    /// provider session or an idle interactive process.
    pub force_new_provider_session: bool,
    /// Keep the conversation-level provider session ref unchanged after this run. The
    /// assistant message and run still retain their own provider-session attribution.
    pub preserve_conversation_provider_session_ref: bool,
    /// When true, the agent was spawned from an external MCP request (e.g. ReefBot).
    /// Filters interactive-only tools (e.g. `ask_user_question`) from the allowed tool list
    /// to prevent deadlocks where the agent waits for human input that will never arrive.
    pub is_external_mcp: bool,
    /// Who initiated this send.  Controls the SpawnFailed catch-and-persist behaviour for
    /// ideation contexts (see `SendCallerContext`).  Defaults to `UserInitiated`.
    pub caller_context: SendCallerContext,
}

/// Unified chat service for all context types
///
/// Key features:
/// - Background spawn pattern: send_message returns immediately
/// - Unified event namespace: all events use agent:* prefix
/// - Backend message queue: messages can be queued while agent is running
/// - Context-aware: routes to appropriate agent based on context type
/// - Task transitions: only TaskExecution context triggers state changes
#[async_trait]
pub trait ChatService: Send + Sync {
    /// Send a message in a context-aware conversation
    ///
    /// Returns immediately with conversation_id and agent_run_id.
    /// Processing happens in background, with events emitted via Tauri.
    ///
    /// Event flow:
    /// 1. agent:run_started
    /// 2. agent:message_created (user message)
    /// 3. agent:chunk (streaming text)
    /// 4. agent:tool_call (tool invocations)
    /// 5. agent:message_created (assistant message)
    /// 6. agent:run_completed or agent:turn_completed (interactive) or agent:error
    async fn send_message(
        &self,
        context_type: ChatContextType,
        context_id: &str,
        message: &str,
        options: SendMessageOptions,
    ) -> Result<SendResult, ChatServiceError>;

    /// Queue a message to be sent when the current agent run completes
    ///
    /// The message is held in the backend queue and automatically sent
    /// via --resume when the current run finishes.
    ///
    /// If `client_id` is provided, that ID will be used for the message,
    /// allowing frontend and backend to use the same ID for tracking.
    async fn queue_message(
        &self,
        context_type: ChatContextType,
        context_id: &str,
        content: &str,
        client_id: Option<&str>,
    ) -> Result<QueuedMessage, ChatServiceError>;

    /// Get all queued messages for a context
    async fn get_queued_messages(
        &self,
        context_type: ChatContextType,
        context_id: &str,
    ) -> Result<Vec<QueuedMessage>, ChatServiceError>;

    /// Delete a queued message before it's sent
    async fn delete_queued_message(
        &self,
        context_type: ChatContextType,
        context_id: &str,
        message_id: &str,
    ) -> Result<bool, ChatServiceError>;

    /// Get or create a conversation for a context.
    /// Returns `(conversation, is_new)` where `is_new` is `true` when a new conversation was created.
    async fn get_or_create_conversation(
        &self,
        context_type: ChatContextType,
        context_id: &str,
    ) -> Result<(ChatConversation, bool), ChatServiceError>;

    /// Get a conversation by ID with all its messages
    async fn get_conversation_with_messages(
        &self,
        conversation_id: &ChatConversationId,
    ) -> Result<Option<ChatConversationWithMessages>, ChatServiceError>;

    /// List all conversations for a context
    async fn list_conversations(
        &self,
        context_type: ChatContextType,
        context_id: &str,
    ) -> Result<Vec<ChatConversation>, ChatServiceError>;

    /// Get the active agent run for a conversation
    async fn get_active_run(
        &self,
        conversation_id: &ChatConversationId,
    ) -> Result<Option<AgentRun>, ChatServiceError>;

    /// Check if the chat service runtime is available
    async fn is_available(&self) -> bool;

    /// Stop a running agent for a context
    ///
    /// Sends SIGTERM to the running agent process and emits an agent:stopped event.
    /// Returns true if an agent was stopped, false if no agent was running.
    async fn stop_agent(
        &self,
        context_type: ChatContextType,
        context_id: &str,
    ) -> Result<bool, ChatServiceError>;

    /// Check if an agent is running for a context
    async fn is_agent_running(&self, context_type: ChatContextType, context_id: &str) -> bool;

    /// Override team mode at runtime (interior mutability).
    /// Default is a no-op; AppChatService uses AtomicBool.
    fn set_team_mode(&self, _mode: bool) {}

    /// Override plan branch repo at runtime (interior mutability).
    /// Default is a no-op; AppChatService uses std::sync::Mutex.
    fn set_plan_branch_repo(&self, _repo: Arc<dyn PlanBranchRepository>) {}

    /// Override the InteractiveProcessRegistry at runtime (interior mutability).
    /// Default is a no-op; AppChatService uses std::sync::Mutex.
    fn set_interactive_process_registry(&self, _registry: Arc<InteractiveProcessRegistry>) {}
}

// ============================================================================
// AppChatService - Production implementation
// ============================================================================

// Helper functions are now in chat_service_helpers.rs

/// Preferred app-layer surface for the unified multi-harness chat runtime.
pub struct AppChatService<R: Runtime = tauri::Wry> {
    cli_path: PathBuf,
    plugin_dir: PathBuf,
    default_working_directory: PathBuf,
    chat_message_repo: Arc<dyn ChatMessageRepository>,
    chat_attachment_repo: Arc<dyn ChatAttachmentRepository>,
    artifact_repo: Arc<dyn ArtifactRepository>,
    conversation_repo: Arc<dyn ChatConversationRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    project_repo: Arc<dyn ProjectRepository>,
    task_repo: Arc<dyn TaskRepository>,
    task_dependency_repo: Arc<dyn TaskDependencyRepository>,
    delegated_session_repo: Arc<dyn DelegatedSessionRepository>,
    execution_settings_repo: Option<Arc<dyn ExecutionSettingsRepository>>,
    agent_lane_settings_repo: Option<Arc<dyn AgentLaneSettingsRepository>>,
    ideation_effort_settings_repo: Option<Arc<dyn IdeationEffortSettingsRepository>>,
    ideation_model_settings_repo: Option<Arc<dyn IdeationModelSettingsRepository>>,
    ideation_session_repo: Arc<dyn IdeationSessionRepository>,
    activity_event_repo: Arc<dyn ActivityEventRepository>,
    message_queue: Arc<MessageQueue>,
    running_agent_registry: Arc<dyn RunningAgentRegistry>,
    memory_event_repo: Arc<dyn MemoryEventRepository>,
    app_handle: Option<AppHandle<R>>,
    execution_state: Option<Arc<crate::commands::ExecutionState>>,
    question_state: Option<Arc<QuestionState>>,
    plan_branch_repo: std::sync::Mutex<Option<Arc<dyn PlanBranchRepository>>>,
    agent_conversation_workspace_repo:
        std::sync::Mutex<Option<Arc<dyn AgentConversationWorkspaceRepository>>>,
    task_proposal_repo: Option<Arc<dyn TaskProposalRepository>>,
    task_step_repo: Option<Arc<dyn TaskStepRepository>>,
    review_repo: Option<Arc<dyn ReviewRepository>>,
    model: String,
    /// When true, agent resolution uses team-lead variants if configured.
    /// Uses AtomicBool for interior mutability so team_mode can be set
    /// after Arc-wrapping (e.g., per-task metadata override).
    team_mode: AtomicBool,
    /// Team service for managing agent teams lifecycle (persistence + events).
    team_service: Option<std::sync::Arc<crate::application::TeamService>>,
    /// Cache for streaming state, used to hydrate frontend on navigation.
    streaming_state_cache: StreamingStateCache,
    /// Registry of interactive processes with open stdin handles for multi-turn messaging.
    /// Wrapped in Mutex for interior mutability so TaskTransitionService can inject the
    /// shared AppState registry after construction (same pattern as plan_branch_repo).
    interactive_process_registry: std::sync::Mutex<Arc<InteractiveProcessRegistry>>,
    /// Registry of verification child process PIDs for explicit cleanup after reconciliation.
    /// Prevents idle verification processes from lingering until the 600s timeout fires.
    verification_child_registry:
        Arc<verification_child_process_registry::VerificationChildProcessRegistry>,
}

/// Compatibility alias for older callsites/tests that still use the legacy concrete name.
pub type ClaudeChatService<R = tauri::Wry> = AppChatService<R>;

impl<R: Runtime> AppChatService<R> {
    pub fn new(
        chat_message_repo: Arc<dyn ChatMessageRepository>,
        chat_attachment_repo: Arc<dyn ChatAttachmentRepository>,
        artifact_repo: Arc<dyn ArtifactRepository>,
        conversation_repo: Arc<dyn ChatConversationRepository>,
        agent_run_repo: Arc<dyn AgentRunRepository>,
        project_repo: Arc<dyn ProjectRepository>,
        task_repo: Arc<dyn TaskRepository>,
        task_dependency_repo: Arc<dyn TaskDependencyRepository>,
        ideation_session_repo: Arc<dyn IdeationSessionRepository>,
        delegated_session_repo: Arc<dyn DelegatedSessionRepository>,
        activity_event_repo: Arc<dyn ActivityEventRepository>,
        message_queue: Arc<MessageQueue>,
        running_agent_registry: Arc<dyn RunningAgentRegistry>,
        memory_event_repo: Arc<dyn MemoryEventRepository>,
    ) -> Self {
        let bootstrap = resolve_default_chat_service_bootstrap();

        Self {
            cli_path: bootstrap.cli_path,
            plugin_dir: bootstrap.plugin_dir,
            default_working_directory: bootstrap.default_working_directory,
            chat_message_repo,
            chat_attachment_repo,
            artifact_repo,
            conversation_repo,
            agent_run_repo,
            project_repo,
            task_repo,
            task_dependency_repo,
            delegated_session_repo,
            execution_settings_repo: None,
            agent_lane_settings_repo: None,
            ideation_effort_settings_repo: None,
            ideation_model_settings_repo: None,
            ideation_session_repo,
            activity_event_repo,
            message_queue,
            running_agent_registry,
            memory_event_repo,
            app_handle: None,
            execution_state: None,
            question_state: None,
            plan_branch_repo: std::sync::Mutex::new(None),
            agent_conversation_workspace_repo: std::sync::Mutex::new(None),
            task_proposal_repo: None,
            task_step_repo: None,
            review_repo: None,
            model: "sonnet".to_string(),
            team_mode: AtomicBool::new(false),
            team_service: None,
            streaming_state_cache: StreamingStateCache::new(),
            interactive_process_registry: std::sync::Mutex::new(Arc::new(
                InteractiveProcessRegistry::new(),
            )),
            verification_child_registry: Arc::new(
                verification_child_process_registry::VerificationChildProcessRegistry::new(),
            ),
        }
    }

    pub fn with_execution_state(mut self, state: Arc<crate::commands::ExecutionState>) -> Self {
        self.execution_state = Some(state);
        self
    }

    pub fn with_execution_settings_repo(
        mut self,
        repo: Arc<dyn ExecutionSettingsRepository>,
    ) -> Self {
        self.execution_settings_repo = Some(repo);
        self
    }

    pub fn with_agent_lane_settings_repo(
        mut self,
        repo: Arc<dyn AgentLaneSettingsRepository>,
    ) -> Self {
        self.agent_lane_settings_repo = Some(repo);
        self
    }

    pub fn with_ideation_effort_settings_repo(
        mut self,
        repo: Arc<dyn IdeationEffortSettingsRepository>,
    ) -> Self {
        self.ideation_effort_settings_repo = Some(repo);
        self
    }

    pub fn with_ideation_model_settings_repo(
        mut self,
        repo: Arc<dyn IdeationModelSettingsRepository>,
    ) -> Self {
        self.ideation_model_settings_repo = Some(repo);
        self
    }

    fn enqueue_pending_send(
        &self,
        context_type: ChatContextType,
        context_id: &str,
        message: &str,
        options: &SendMessageOptions,
        conversation_id: Option<String>,
    ) -> QueuedMessage {
        let queued = self.message_queue.queue_with_overrides(
            context_type,
            context_id,
            message.to_string(),
            options.metadata.clone(),
            options.created_at.map(|ts| ts.to_rfc3339()),
            options.harness_override,
        );
        self.emit_event(
            "agent:message_queued",
            AgentMessageQueuedPayload {
                message_id: queued.id.clone(),
                content: queued.content.clone(),
                context_type: context_type.to_string(),
                context_id: context_id.to_string(),
                conversation_id,
                created_at: queued.created_at.clone(),
            },
        );
        queued
    }

    async fn get_or_create_conversation_for_send(
        &self,
        context_type: ChatContextType,
        context_id: &str,
        options: &SendMessageOptions,
    ) -> Result<(ChatConversation, bool), ChatServiceError> {
        let Some(conversation_id) = options.conversation_id_override else {
            return chat_service_repository::get_or_create_conversation(
                Arc::clone(&self.conversation_repo),
                context_type,
                context_id,
            )
            .await;
        };

        let conversation = self
            .conversation_repo
            .get_by_id(&conversation_id)
            .await
            .map_err(|e| ChatServiceError::RepositoryError(e.to_string()))?
            .ok_or_else(|| {
                ChatServiceError::ConversationNotFound(format!(
                    "Conversation not found: {}",
                    conversation_id
                ))
            })?;

        if conversation.context_type != context_type || conversation.context_id != context_id {
            return Err(ChatServiceError::ContextNotFound(format!(
                "Conversation {} belongs to {}/{} not {}/{}",
                conversation_id,
                conversation.context_type,
                conversation.context_id,
                context_type,
                context_id
            )));
        }

        Ok((conversation, false))
    }

    pub fn with_question_state(mut self, state: Arc<QuestionState>) -> Self {
        self.question_state = Some(state);
        self
    }

    pub fn with_plan_branch_repo(self, repo: Arc<dyn PlanBranchRepository>) -> Self {
        *self.plan_branch_repo.lock().unwrap() = Some(repo);
        self
    }

    pub fn with_agent_conversation_workspace_repo(
        self,
        repo: Arc<dyn AgentConversationWorkspaceRepository>,
    ) -> Self {
        *self.agent_conversation_workspace_repo.lock().unwrap() = Some(repo);
        self
    }

    pub fn with_task_proposal_repo(mut self, repo: Arc<dyn TaskProposalRepository>) -> Self {
        self.task_proposal_repo = Some(repo);
        self
    }

    pub fn with_task_step_repo(mut self, repo: Arc<dyn TaskStepRepository>) -> Self {
        self.task_step_repo = Some(repo);
        self
    }

    pub fn with_review_repo(mut self, repo: Arc<dyn ReviewRepository>) -> Self {
        self.review_repo = Some(repo);
        self
    }

    pub fn with_cli_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.cli_path = path.into();
        self
    }

    pub fn with_plugin_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.plugin_dir = path.into();
        self
    }

    pub fn with_working_directory(mut self, path: impl Into<PathBuf>) -> Self {
        self.default_working_directory = path.into();
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    pub fn with_team_mode(mut self, team_mode: bool) -> Self {
        self.team_mode = AtomicBool::new(team_mode);
        self
    }

    pub fn with_team_service(
        mut self,
        service: std::sync::Arc<crate::application::TeamService>,
    ) -> Self {
        self.team_service = Some(service);
        self
    }

    pub fn with_streaming_state_cache(mut self, cache: StreamingStateCache) -> Self {
        self.streaming_state_cache = cache;
        self
    }

    pub fn with_interactive_process_registry(
        mut self,
        registry: Arc<InteractiveProcessRegistry>,
    ) -> Self {
        self.interactive_process_registry = std::sync::Mutex::new(registry);
        self
    }

    /// Returns a clone of the current InteractiveProcessRegistry Arc.
    fn ipr(&self) -> Arc<InteractiveProcessRegistry> {
        Arc::clone(&*self.interactive_process_registry.lock().unwrap())
    }

    async fn cleanup_stale_registry_block(
        &self,
        registry_key: &RunningAgentKey,
        existing: &RunningAgentInfo,
        context_type: ChatContextType,
        context_id: &str,
        source: &'static str,
    ) -> bool {
        if !registry_entry_blocks_send_but_is_stale(existing, chrono::Utc::now()) {
            return false;
        }

        match self
            .running_agent_registry
            .cleanup_stale_entry(registry_key)
            .await
        {
            Ok(Some(info)) => {
                tracing::warn!(
                    %context_type,
                    context_id,
                    stale_pid = info.pid,
                    stale_run_id = %info.agent_run_id,
                    source,
                    "Cleaned stale running-agent registry entry before chat send"
                );
                true
            }
            Ok(None) => {
                tracing::debug!(
                    %context_type,
                    context_id,
                    existing_pid = existing.pid,
                    existing_run_id = %existing.agent_run_id,
                    source,
                    "Registry entry looked stale but cleanup kept it"
                );
                false
            }
            Err(error) => {
                tracing::warn!(
                    %context_type,
                    context_id,
                    error = %error,
                    source,
                    "Failed to clean stale running-agent registry entry before chat send"
                );
                false
            }
        }
    }

    async fn cleanup_inactive_registry_block(
        &self,
        registry_key: &RunningAgentKey,
        existing: &RunningAgentInfo,
        context_type: ChatContextType,
        context_id: &str,
        source: &'static str,
    ) -> bool {
        let run = match self
            .agent_run_repo
            .get_by_id(&AgentRunId::from_string(&existing.agent_run_id))
            .await
        {
            Ok(run) => run,
            Err(error) => {
                tracing::warn!(
                    %context_type,
                    context_id,
                    existing_pid = existing.pid,
                    existing_run_id = %existing.agent_run_id,
                    error = %error,
                    source,
                    "Failed to load blocking agent run before chat send; keeping registry entry"
                );
                return false;
            }
        };
        let run_status = run.as_ref().map(|run| run.status);

        if !registry_entry_blocks_send_because_run_inactive(
            existing,
            run_status,
            chrono::Utc::now(),
        ) {
            return false;
        }

        let reason = match run_status {
            Some(status) => status.to_string(),
            None => "run_missing".to_string(),
        };

        let Some(info) = self
            .running_agent_registry
            .unregister(registry_key, &existing.agent_run_id)
            .await
        else {
            tracing::debug!(
                %context_type,
                context_id,
                existing_pid = existing.pid,
                existing_run_id = %existing.agent_run_id,
                source,
                reason = %reason,
                "Inactive registry entry was already replaced before cleanup"
            );
            return false;
        };

        if is_process_alive(info.pid) {
            if let Some(token) = info.cancellation_token.as_ref() {
                token.cancel();
            }
            if info.pid == std::process::id() {
                tracing::warn!(
                    %context_type,
                    context_id,
                    pid = info.pid,
                    source,
                    "Refusing to kill current process while cleaning inactive registry entry"
                );
            } else {
                kill_process(info.pid);
            }
        }

        tracing::warn!(
            %context_type,
            context_id,
            stale_pid = info.pid,
            stale_run_id = %info.agent_run_id,
            source,
            reason = %reason,
            "Cleaned inactive running-agent registry entry before chat send"
        );
        true
    }

    async fn count_active_ideation_slots(&self) -> Result<u32, ChatServiceError> {
        let registry_entries = self.running_agent_registry.list_all().await;
        let mut count = 0u32;

        for (key, info) in registry_entries {
            if info.pid == 0 || !is_ideation_registry_context(&key.context_type) {
                continue;
            }

            if key.context_type == "session" {
                let session_id = IdeationSessionId::from_string(key.context_id.clone());
                match self.ideation_session_repo.get_by_id(&session_id).await {
                    Ok(Some(_)) => {}
                    Ok(None) => continue,
                    Err(e) => return Err(ChatServiceError::RepositoryError(e.to_string())),
                }
            }

            if let Some(ref exec) = self.execution_state {
                let slot_key = format!("{}/{}", key.context_type, key.context_id);
                if exec.is_interactive_idle(&slot_key) {
                    continue;
                }
            }

            count += 1;
        }

        Ok(count)
    }

    async fn count_active_ideation_slots_for_project(
        &self,
        project_id: &ProjectId,
    ) -> Result<u32, ChatServiceError> {
        let registry_entries = self.running_agent_registry.list_all().await;
        let mut count = 0u32;

        for (key, info) in registry_entries {
            if info.pid == 0 || !is_ideation_registry_context(&key.context_type) {
                continue;
            }

            let session_id = IdeationSessionId::from_string(key.context_id.clone());
            let session = match self.ideation_session_repo.get_by_id(&session_id).await {
                Ok(Some(session)) => session,
                Ok(None) => continue,
                Err(e) => return Err(ChatServiceError::RepositoryError(e.to_string())),
            };

            if session.project_id != *project_id {
                continue;
            }

            if let Some(ref exec) = self.execution_state {
                let slot_key = format!("{}/{}", key.context_type, key.context_id);
                if exec.is_interactive_idle(&slot_key) {
                    continue;
                }
            }

            count += 1;
        }

        Ok(count)
    }

    async fn count_active_slot_consuming_contexts_for_project(
        &self,
        project_id: &ProjectId,
    ) -> Result<u32, ChatServiceError> {
        let registry_entries = self.running_agent_registry.list_all().await;
        let mut count = 0u32;

        for (key, info) in registry_entries {
            if info.pid == 0 {
                continue;
            }

            if is_ideation_registry_context(&key.context_type) {
                let session_id = IdeationSessionId::from_string(key.context_id.clone());
                let session = match self.ideation_session_repo.get_by_id(&session_id).await {
                    Ok(Some(session)) => session,
                    Ok(None) => continue,
                    Err(e) => return Err(ChatServiceError::RepositoryError(e.to_string())),
                };

                if session.project_id != *project_id {
                    continue;
                }

                if let Some(ref exec) = self.execution_state {
                    let slot_key = format!("{}/{}", key.context_type, key.context_id);
                    if exec.is_interactive_idle(&slot_key) {
                        continue;
                    }
                }

                count += 1;
                continue;
            }

            let context_type = match key.context_type.parse::<ChatContextType>() {
                Ok(value) => value,
                Err(_) => continue,
            };

            if !uses_execution_slot(context_type) {
                continue;
            }

            let task_id = TaskId::from_string(key.context_id.clone());
            let task = match self.task_repo.get_by_id(&task_id).await {
                Ok(Some(task)) => task,
                Ok(None) => continue,
                Err(e) => return Err(ChatServiceError::RepositoryError(e.to_string())),
            };

            if task.project_id != *project_id
                || !crate::commands::execution_commands::context_matches_running_status_for_gc(
                    context_type,
                    task.internal_status,
                )
            {
                continue;
            }

            count += 1;
        }

        Ok(count)
    }

    async fn has_runnable_execution_waiting(
        &self,
        project_filter: Option<&ProjectId>,
    ) -> Result<bool, ChatServiceError> {
        if let Some(project_id) = project_filter {
            let tasks = self
                .task_repo
                .get_by_project(project_id)
                .await
                .map_err(|e| ChatServiceError::RepositoryError(e.to_string()))?;
            if tasks
                .iter()
                .any(|task| task.internal_status == InternalStatus::Ready)
            {
                return Ok(true);
            }
        } else {
            let projects = self
                .project_repo
                .get_all()
                .await
                .map_err(|e| ChatServiceError::RepositoryError(e.to_string()))?;
            for project in projects {
                let tasks = self
                    .task_repo
                    .get_by_project(&project.id)
                    .await
                    .map_err(|e| ChatServiceError::RepositoryError(e.to_string()))?;
                if tasks
                    .iter()
                    .any(|task| task.internal_status == InternalStatus::Ready)
                {
                    return Ok(true);
                }
            }
        }

        for key in self.message_queue.list_keys() {
            if !matches!(
                key.context_type,
                ChatContextType::TaskExecution | ChatContextType::Review | ChatContextType::Merge
            ) {
                continue;
            }

            let task_id = TaskId::from_string(key.context_id.clone());
            let Some(task) = self
                .task_repo
                .get_by_id(&task_id)
                .await
                .map_err(|e| ChatServiceError::RepositoryError(e.to_string()))?
            else {
                continue;
            };

            if project_filter.is_none_or(|project_id| task.project_id == *project_id) {
                return Ok(true);
            }
        }

        Ok(false)
    }

    pub fn with_app_handle(mut self, app_handle: AppHandle<R>) -> Self {
        self.app_handle = Some(app_handle);
        self
    }

    /// Get a reference to the streaming state cache.
    ///
    /// Used by HTTP handlers to fetch current streaming state for hydration.
    pub fn streaming_state_cache(&self) -> &StreamingStateCache {
        &self.streaming_state_cache
    }

    /// Emit a Tauri event if app_handle is available
    fn emit_event(&self, event: &str, payload: impl Serialize + Clone) {
        if let Some(ref handle) = self.app_handle {
            let _ = handle.emit(event, payload);
        }
    }

    /// Resolve the project's working directory from a context.
    ///
    /// Returns `Err` for Merge contexts that resolve to the primary repo
    /// (hard error to prevent fixer agent from corrupting user's checkout).
    async fn resolve_working_directory(
        &self,
        context_type: ChatContextType,
        context_id: &str,
    ) -> Result<PathBuf, String> {
        chat_service_context::resolve_working_directory(
            context_type,
            context_id,
            Arc::clone(&self.project_repo),
            Arc::clone(&self.task_repo),
            Arc::clone(&self.ideation_session_repo),
            Arc::clone(&self.delegated_session_repo),
            &self.default_working_directory,
        )
        .await
    }

    async fn load_agent_conversation_workspace(
        &self,
        context_type: ChatContextType,
        conversation_id: &ChatConversationId,
    ) -> Result<Option<AgentConversationWorkspace>, ChatServiceError> {
        if context_type != ChatContextType::Project {
            return Ok(None);
        }

        let repo = self
            .agent_conversation_workspace_repo
            .lock()
            .unwrap()
            .clone();
        let Some(repo) = repo else {
            return Ok(None);
        };

        repo.get_by_conversation_id(conversation_id)
            .await
            .map_err(|error| ChatServiceError::RepositoryError(error.to_string()))
    }

    async fn resolve_agent_workspace_working_directory(
        &self,
        workspace: &AgentConversationWorkspace,
    ) -> Result<PathBuf, ChatServiceError> {
        let project = self
            .project_repo
            .get_by_id(&workspace.project_id)
            .await
            .map_err(|error| ChatServiceError::RepositoryError(error.to_string()))?
            .ok_or_else(|| {
                ChatServiceError::SpawnFailed(format!(
                    "Project not found for agent conversation workspace: {}",
                    workspace.project_id
                ))
            })?;

        match resolve_valid_agent_conversation_workspace_path(&project, workspace).await {
            Ok(path) => Ok(path),
            Err(error) => {
                if error
                    .to_string()
                    .contains("Agent conversation workspace is missing")
                {
                    self.mark_agent_conversation_workspace_missing(workspace)
                        .await;
                }
                Err(ChatServiceError::SpawnFailed(error.to_string()))
            }
        }
    }

    async fn mark_agent_conversation_workspace_missing(
        &self,
        workspace: &AgentConversationWorkspace,
    ) {
        let repo = self
            .agent_conversation_workspace_repo
            .lock()
            .unwrap()
            .clone();
        let Some(repo) = repo else {
            return;
        };

        if let Err(error) = repo
            .update_status(
                &workspace.conversation_id,
                AgentConversationWorkspaceStatus::Missing,
            )
            .await
        {
            tracing::warn!(
                conversation_id = workspace.conversation_id.as_str(),
                error = %error,
                "Failed to mark missing agent conversation workspace"
            );
        }
    }

    async fn prepare_agent_workspace_continuation_for_send(
        &self,
        context_type: ChatContextType,
        context_id: &str,
        runtime_context_id: &str,
        conversation_id_override: Option<&ChatConversationId>,
    ) -> Result<(), ChatServiceError> {
        if context_type != ChatContextType::Project {
            return Ok(());
        }

        let Some(conversation_id) = conversation_id_override.copied() else {
            return Ok(());
        };

        let repo = self
            .agent_conversation_workspace_repo
            .lock()
            .unwrap()
            .clone();
        let Some(repo) = repo else {
            return Ok(());
        };

        let Some(workspace) = repo
            .get_by_conversation_id(&conversation_id)
            .await
            .map_err(|error| ChatServiceError::RepositoryError(error.to_string()))?
        else {
            return Ok(());
        };

        if !is_terminal_agent_conversation_publication_status(
            workspace.publication_pr_status.as_deref(),
        ) {
            return Ok(());
        }

        let registry_key = RunningAgentKey::new(context_type.to_string(), runtime_context_id);
        if self.running_agent_registry.is_running(&registry_key).await {
            return Err(ChatServiceError::SpawnFailed(
                "Cannot continue this workspace while the previous agent turn is still running"
                    .to_string(),
            ));
        }

        let interactive_key =
            InteractiveProcessKey::new(context_type.to_string(), runtime_context_id);
        if self.ipr().has_process(&interactive_key).await {
            self.ipr().remove(&interactive_key).await;
            tracing::info!(
                %context_type,
                context_id,
                runtime_context_id,
                conversation_id = %conversation_id,
                "Dropped stale interactive process before agent workspace branch rollover"
            );
        }

        let project = self
            .project_repo
            .get_by_id(&workspace.project_id)
            .await
            .map_err(|error| ChatServiceError::RepositoryError(error.to_string()))?
            .ok_or_else(|| {
                ChatServiceError::SpawnFailed(format!(
                    "Project not found for agent conversation workspace: {}",
                    workspace.project_id
                ))
            })?;

        if project.id.as_str() != context_id {
            return Err(ChatServiceError::ContextNotFound(format!(
                "Agent conversation workspace {} belongs to project {} instead of {}",
                workspace.conversation_id, project.id, context_id
            )));
        }

        let updated_workspace = rollover_agent_conversation_workspace(&project, &workspace)
            .await
            .map_err(|error| ChatServiceError::SpawnFailed(error.to_string()))?;
        let updated_workspace = repo
            .create_or_update(updated_workspace)
            .await
            .map_err(|error| ChatServiceError::RepositoryError(error.to_string()))?;

        self.conversation_repo
            .clear_provider_session_ref(&conversation_id)
            .await
            .map_err(|error| ChatServiceError::RepositoryError(error.to_string()))?;

        self.persist_agent_workspace_continuation_message(
            context_id,
            conversation_id,
            &updated_workspace,
        )
        .await?;

        Ok(())
    }

    async fn persist_agent_workspace_continuation_message(
        &self,
        context_id: &str,
        conversation_id: ChatConversationId,
        workspace: &AgentConversationWorkspace,
    ) -> Result<(), ChatServiceError> {
        let metadata = serde_json::json!({
            "kind": "agent_workspace_branch_rollover",
            "branch_name": &workspace.branch_name,
            "base_ref": &workspace.base_ref,
        })
        .to_string();
        let mut message = ChatMessage::user_in_project(
            ProjectId::from_string(context_id.to_string()),
            AGENT_CONVERSATION_WORKSPACE_CONTINUATION_MESSAGE,
        )
        .with_metadata(metadata.clone());
        message.role = MessageRole::System;
        message.conversation_id = Some(conversation_id);

        let message_id = message.id.as_str().to_string();
        let created_at = message.created_at.to_rfc3339();
        self.chat_message_repo
            .create(message)
            .await
            .map_err(|error| ChatServiceError::RepositoryError(error.to_string()))?;

        self.emit_event(
            "agent:message_created",
            AgentMessageCreatedPayload {
                message_id,
                conversation_id: conversation_id.as_str().to_string(),
                context_type: ChatContextType::Project.to_string(),
                context_id: context_id.to_string(),
                role: "system".to_string(),
                content: AGENT_CONVERSATION_WORKSPACE_CONTINUATION_MESSAGE.to_string(),
                created_at: Some(created_at),
                metadata: Some(metadata),
            },
        );

        Ok(())
    }

    /// Create a spawnable Claude CLI command (one-shot mode with `-p`).
    /// Kept for fallback/non-interactive spawn paths (queue resume, retry).
    #[allow(dead_code)]
    async fn build_command(
        &self,
        conversation: &ChatConversation,
        user_message: &str,
        working_directory: &Path,
        entity_status: Option<&str>,
        project_id: Option<&str>,
        session_messages: &[crate::domain::entities::ChatMessage],
        total_available: usize,
    ) -> Result<crate::infrastructure::agents::claude::SpawnableCommand, ChatServiceError> {
        chat_service_context::build_command(
            &self.cli_path,
            &self.plugin_dir,
            conversation,
            user_message,
            working_directory,
            entity_status,
            project_id,
            self.team_mode.load(Ordering::Relaxed),
            Arc::clone(&self.chat_attachment_repo),
            Arc::clone(&self.artifact_repo),
            self.agent_lane_settings_repo.clone(),
            self.ideation_effort_settings_repo.clone(),
            self.ideation_model_settings_repo.clone(),
            session_messages,
            total_available,
            None, // effort_override: callers pre-resolve if needed
            None, // model_override: callers pre-resolve if needed
        )
        .await
        .map_err(ChatServiceError::SpawnFailed)
    }

    #[allow(clippy::too_many_arguments)]
    async fn spawn_process_for_harness(
        &self,
        conversation: &ChatConversation,
        message: &str,
        agent_name_override: Option<&str>,
        context_type: ChatContextType,
        context_id: &str,
        runtime_context_id: &str,
        working_directory: &Path,
        entity_status: Option<&str>,
        project_id: Option<&str>,
        session_messages: &[crate::domain::entities::ChatMessage],
        session_total: usize,
        is_external_mcp: bool,
        runtime_team_mode: bool,
        stored_session_id: Option<&str>,
        resolved_spawn_settings: &crate::application::agent_lane_resolution::ResolvedAgentSpawnSettings,
    ) -> Result<
        (
            PathBuf,
            tokio::process::Child,
            Option<Arc<InteractiveProcessRegistry>>,
        ),
        ChatServiceError,
    > {
        let effective_harness = resolved_spawn_settings.effective_harness;
        let cli_path = if effective_harness == DEFAULT_AGENT_HARNESS {
            self.cli_path.clone()
        } else {
            resolve_chat_service_bootstrap(effective_harness).cli_path
        };
        let plugin_dir = if effective_harness == DEFAULT_AGENT_HARNESS {
            self.plugin_dir.clone()
        } else {
            resolve_harness_plugin_dir(effective_harness, working_directory)
        };

        let launch_plan = chat_service_context::build_launch_plan_for_harness(
            effective_harness,
            &cli_path,
            &plugin_dir,
            conversation,
            message,
            agent_name_override,
            context_type,
            context_id,
            working_directory,
            entity_status,
            project_id,
            runtime_team_mode,
            Arc::clone(&self.chat_attachment_repo),
            Arc::clone(&self.artifact_repo),
            Arc::clone(&self.ideation_session_repo),
            Arc::clone(&self.delegated_session_repo),
            Arc::clone(&self.task_repo),
            session_messages,
            session_total,
            is_external_mcp,
            stored_session_id.clone(),
            resolved_spawn_settings,
        )
        .await
        .map_err(|error| {
            tracing::warn!(
                harness = %effective_harness,
                cli_path = %cli_path.display(),
                %error,
                "chat_service.send_message missing harness runtime"
            );
            ChatServiceError::SpawnFailed(error)
        })?;

        let launch_mode = launch_plan.launch_mode();
        tracing::info!(mode = ?launch_mode, plan = ?launch_plan, "Spawning chat harness agent");
        let launched = launch_plan.spawn().await.map_err(|error| {
            tracing::error!(mode = ?launch_mode, error = %error, "chat_service.send_message harness spawn failed");
            ChatServiceError::SpawnFailed(error.to_string())
        })?;
        tracing::debug!(
            mode = ?launch_mode,
            pid = ?launched.child.id(),
            "chat_service.send_message harness spawn ok"
        );

        if let Some(child_stdin) = launched.child_stdin {
            let interactive_key_for_register =
                InteractiveProcessKey::new(context_type.to_string(), runtime_context_id);
            tracing::info!(
                context_type = %context_type,
                context_id,
                runtime_context_id = %runtime_context_id,
                "[IPR_REGISTER] Registering lead stdin in InteractiveProcessRegistry"
            );
            self.ipr()
                .register_with_metadata(
                    interactive_key_for_register,
                    child_stdin,
                    InteractiveProcessMetadata {
                        harness: Some(resolved_spawn_settings.effective_harness),
                        provider_session_id: stored_session_id.map(str::to_string),
                    },
                )
                .await;

            Ok((launched.cli_path, launched.child, Some(self.ipr())))
        } else {
            Ok((launched.cli_path, launched.child, None))
        }
    }

    /// Fetch entity status for context types that support it
    /// Used for dynamic agent resolution based on entity state
    async fn get_entity_status(
        &self,
        context_type: ChatContextType,
        context_id: &str,
    ) -> Option<String> {
        match context_type {
            // Task-related contexts: look up task status
            ChatContextType::Task
            | ChatContextType::TaskExecution
            | ChatContextType::Review
            | ChatContextType::Merge => {
                let task_id = TaskId::from_string(context_id.to_string());
                if let Ok(Some(task)) = self.task_repo.get_by_id(&task_id).await {
                    Some(task.internal_status.as_str().to_string())
                } else {
                    None
                }
            }
            // Ideation context: check purpose first (Verification sessions → ralphx-plan-verifier agent)
            // then fall back to status for accepted/readonly routing
            ChatContextType::Ideation => {
                let session_id = IdeationSessionId::from_string(context_id);
                if let Ok(Some(session)) = self.ideation_session_repo.get_by_id(&session_id).await {
                    if session.session_purpose == SessionPurpose::Verification {
                        Some("verification".to_string())
                    } else {
                        Some(session.status.to_string())
                    }
                } else {
                    None
                }
            }
            ChatContextType::Delegation => {
                let session_id =
                    crate::domain::entities::DelegatedSessionId::from_string(context_id);
                if let Ok(Some(session)) = self.delegated_session_repo.get_by_id(&session_id).await
                {
                    Some(session.status)
                } else {
                    None
                }
            }
            // Other contexts don't have status-based agent resolution yet
            ChatContextType::Project => None,
        }
    }
}

#[async_trait]
impl<R: Runtime + 'static> ChatService for AppChatService<R> {
    async fn send_message(
        &self,
        context_type: ChatContextType,
        context_id: &str,
        message: &str,
        options: SendMessageOptions,
    ) -> Result<SendResult, ChatServiceError> {
        tracing::info!(
            %context_type,
            context_id,
            conversation_id_override = ?options
                .conversation_id_override
                .as_ref()
                .map(|id| id.as_str().to_string()),
            message_len = message.len(),
            "chat_service.send_message start"
        );
        let runtime_context_id = runtime_context_id_for_send(
            context_type,
            context_id,
            options.conversation_id_override.as_ref(),
        );
        if runtime_context_id != context_id {
            tracing::info!(
                %context_type,
                context_id,
                runtime_context_id = %runtime_context_id,
                "chat_service.send_message using conversation-scoped runtime key"
            );
        }

        // Runtime halt barrier for all slot-consuming contexts: do not start new
        // task/review/merge/ideation work while the global execution state is
        // paused/stopped. Fresh idle ideation prompts must be durable because
        // the in-memory queue is not replayed after an app restart.
        if claude_launches_paused(context_type, self.execution_state.as_ref()) {
            let (conversation, is_new_conversation) = self
                .get_or_create_conversation_for_send(context_type, context_id, &options)
                .await?;

            if context_type == ChatContextType::Ideation
                && options.caller_context == SendCallerContext::UserInitiated
            {
                let paused_ideation_key =
                    RunningAgentKey::new(context_type.to_string(), &runtime_context_id);
                let paused_ideation_ipr_key =
                    InteractiveProcessKey::new(context_type.to_string(), &runtime_context_id);
                let paused_ideation_has_live_agent = self
                    .running_agent_registry
                    .is_running(&paused_ideation_key)
                    .await
                    || self.ipr().has_process(&paused_ideation_ipr_key).await;

                if !paused_ideation_has_live_agent {
                    match self
                        .ideation_session_repo
                        .set_pending_initial_prompt_if_unset(context_id, message.to_string())
                        .await
                    {
                        Ok(true) => {
                            tracing::info!(
                                %context_type,
                                context_id,
                                runtime_context_id = %runtime_context_id,
                                "chat_service.send_message: execution paused, persisted idle ideation prompt as pending_initial_prompt"
                            );
                            return Ok(SendResult {
                                conversation_id: conversation.id.as_str().to_string(),
                                agent_run_id: String::new(),
                                is_new_conversation,
                                was_queued: true,
                                queued_message_id: None,
                                queued_as_pending: true,
                            });
                        }
                        Ok(false) => {
                            tracing::warn!(
                                %context_type,
                                context_id,
                                runtime_context_id = %runtime_context_id,
                                "chat_service.send_message: execution paused and ideation pending_initial_prompt already exists"
                            );
                            return Err(ChatServiceError::SpawnFailed(
                                "execution paused; ideation session already has a pending prompt"
                                    .to_string(),
                            ));
                        }
                        Err(error) => {
                            tracing::error!(
                                %context_type,
                                context_id,
                                runtime_context_id = %runtime_context_id,
                                error = %error,
                                "chat_service.send_message: execution paused and failed to persist ideation prompt"
                            );
                            return Err(ChatServiceError::SpawnFailed(
                                "execution paused; failed to persist ideation prompt".to_string(),
                            ));
                        }
                    }
                }
            }

            let queued = self.enqueue_pending_send(
                context_type,
                &runtime_context_id,
                message,
                &options,
                Some(conversation.id.as_str()),
            );
            tracing::info!(
                %context_type,
                context_id,
                runtime_context_id = %runtime_context_id,
                queued_message_id = %queued.id,
                "chat_service.send_message: execution paused, queued agent message instead of spawning"
            );
            return Ok(SendResult {
                conversation_id: conversation.id.as_str().to_string(),
                agent_run_id: String::new(),
                is_new_conversation,
                was_queued: true,
                queued_message_id: Some(queued.id),
                queued_as_pending: false,
            });
        }

        self.prepare_agent_workspace_continuation_for_send(
            context_type,
            context_id,
            &runtime_context_id,
            options.conversation_id_override.as_ref(),
        )
        .await?;

        // 1. Interactive fast-path (Gate 1): if an interactive process is already
        //    running for this context, write the message directly to its stdin.
        //    IMPORTANT: Do this BEFORE get_or_create_conversation() because for
        //    TaskExecution/Merge contexts, that call creates a FRESH conversation
        //    (force_fresh=true). When reusing an existing process via stdin, we
        //    must use the EXISTING conversation to avoid the frontend thinking a
        //    new execution started.
        let interactive_key =
            InteractiveProcessKey::new(context_type.to_string(), &runtime_context_id);
        let ipr_ref = self.ipr();
        let has_ipr_entry = ipr_ref.has_process(&interactive_key).await;
        tracing::info!(
            %context_type,
            context_id,
            runtime_context_id = %runtime_context_id,
            gate = "GATE_1_IPR",
            has_ipr_entry,
            "[GATE_TRACE] Gate 1 (IPR lookup)"
        );
        if !has_ipr_entry {
            // Diagnostic: dump all registered IPR keys when lookup fails
            ipr_ref.log_registered_keys("GATE_1_MISS").await;
        }
        if has_ipr_entry && options.force_new_provider_session {
            ipr_ref.remove(&interactive_key).await;
            tracing::info!(
                %context_type,
                context_id,
                runtime_context_id = %runtime_context_id,
                "chat_service.send_message: skipped existing interactive process for fresh provider session"
            );
        }
        if has_ipr_entry && !options.force_new_provider_session {
            tracing::info!(
                %context_type,
                context_id,
                runtime_context_id = %runtime_context_id,
                "chat_service.send_message: interactive process found, writing to stdin"
            );

            // Build the prompt with context wrapping, then format as stream-json input.
            // The interactive process uses `-p - --input-format stream-json`, so each
            // message must be a single-line JSON object.
            // Session history is NOT injected here — the agent is already running and
            // has live context. History injection is only for new process spawns.
            let stdin_prompt = chat_service_context::build_initial_prompt(
                context_type,
                context_id,
                message,
                &[],
                0,
            );
            let stream_json_msg =
                crate::infrastructure::agents::claude::format_stream_json_input(&stdin_prompt);

            match self
                .ipr()
                .write_message(&interactive_key, &stream_json_msg)
                .await
            {
                Ok(()) => {
                    // Re-increment running count only if the process was idle
                    // (TurnComplete decremented and marked idle). If the agent is
                    // already active (mid-turn), skip — prevents double-increment
                    // on rapid burst messages.
                    if uses_execution_slot(context_type) {
                        if let Some(ref exec) = self.execution_state {
                            let slot_key = format!("{}/{}", context_type, context_id);
                            if exec.claim_interactive_slot(&slot_key) {
                                exec.increment_running();
                                if let Some(ref handle) = self.app_handle {
                                    exec.emit_status_changed(handle, "interactive_turn_resumed");
                                }
                            }
                        }
                    }

                    // Use the EXISTING conversation — not a force-fresh one.
                    // The interactive process was spawned with a conversation, so
                    // get_active_for_context should always find it.
                    let existing_conv = match options.conversation_id_override {
                        Some(conversation_id) => self
                            .conversation_repo
                            .get_by_id(&conversation_id)
                            .await
                            .map_err(|e| ChatServiceError::RepositoryError(e.to_string()))?,
                        None => self
                            .conversation_repo
                            .get_active_for_context(context_type, context_id)
                            .await
                            .map_err(|e| ChatServiceError::RepositoryError(e.to_string()))?,
                    };

                    let conversation = match existing_conv {
                        Some(conv) => {
                            tracing::debug!(
                                conversation_id = conv.id.as_str(),
                                "Gate 1: reusing existing conversation for interactive process"
                            );
                            conv
                        }
                        None => {
                            // Edge case: IPR has process but no conversation found.
                            // Create one as fallback (shouldn't happen in practice).
                            tracing::warn!(
                                %context_type,
                                context_id,
                                "Gate 1: no existing conversation found despite IPR entry, creating new"
                            );
                            let (conversation, _) = self
                                .get_or_create_conversation_for_send(
                                    context_type,
                                    context_id,
                                    &options,
                                )
                                .await?;
                            conversation
                        }
                    };

                    let resume_in_place = resume_in_place_requested(options.metadata.as_deref());
                    let persisted_metadata =
                        strip_resume_in_place_metadata(options.metadata.clone());

                    // Store user message for conversation history
                    if !resume_in_place {
                        let user_msg = chat_service_context::create_user_message(
                            context_type,
                            context_id,
                            message,
                            conversation.id,
                            persisted_metadata.clone(),
                            options.created_at,
                        );
                        let user_msg_id = user_msg.id.as_str().to_string();
                        let user_msg_created_at = user_msg.created_at.to_rfc3339();
                        let _ = self.chat_message_repo.create(user_msg).await;

                        if context_type == ChatContextType::Ideation {
                            let _ = self
                                .ideation_session_repo
                                .touch_updated_at(context_id)
                                .await;
                        }

                        // Emit message_created event for frontend
                        self.emit_event(
                            "agent:message_created",
                            AgentMessageCreatedPayload {
                                message_id: user_msg_id,
                                conversation_id: conversation.id.as_str().to_string(),
                                context_type: context_type.to_string(),
                                context_id: context_id.to_string(),
                                role: "user".to_string(),
                                content: message.to_string(),
                                created_at: Some(user_msg_created_at),
                                metadata: persisted_metadata.clone(),
                            },
                        );
                    }

                    // Emit run_started so frontend shows activity spinner
                    let interactive_run_id = uuid::Uuid::new_v4().to_string();
                    let process_metadata = ipr_ref.get_metadata(&interactive_key).await;
                    let (provider_harness, provider_session_id) =
                        interactive_run_started_provider_session(
                            &conversation,
                            process_metadata.as_ref(),
                        );
                    self.emit_event(
                        "agent:run_started",
                        AgentRunStartedPayload::with_provider_session(
                            interactive_run_id,
                            conversation.id.as_str().to_string(),
                            context_type.to_string(),
                            context_id.to_string(),
                            None,
                            None,
                            None,
                            None,
                            Some(provider_harness),
                            provider_session_id,
                        ),
                    );

                    return Ok(SendResult {
                        conversation_id: conversation.id.as_str().to_string(),
                        agent_run_id: uuid::Uuid::new_v4().to_string(),
                        is_new_conversation: false,
                        ..Default::default()
                    });
                }
                Err(e) => {
                    tracing::warn!(
                        %context_type,
                        context_id,
                        error = %e,
                        "chat_service.send_message: interactive stdin write failed, \
                         falling back to new spawn"
                    );
                    // Remove the broken entry so we don't keep trying
                    self.ipr().remove(&interactive_key).await;
                    // Fall through to normal spawn path
                }
            }
        }

        // 2. Get or create conversation (only reached when Gate 1 misses or fails).
        //    For TaskExecution/Merge this creates a fresh conversation (force_fresh=true),
        //    which is correct for new spawns.
        let (mut conversation, spawn_path_is_new_conversation) = self
            .get_or_create_conversation_for_send(context_type, context_id, &options)
            .await?;
        let provider_session_ref = conversation.provider_session_ref();
        let task_metadata = if spawn_settings_require_task_metadata(context_type) {
            self.task_repo
                .get_by_id(&TaskId::from_string(context_id.to_string()))
                .await
                .map_err(|e| ChatServiceError::RepositoryError(e.to_string()))?
                .and_then(|task| task.metadata)
        } else {
            None
        };
        let parent_conversation = if provider_session_ref.is_none() {
            if let Some(parent_id) = conversation.parent_conversation_id.as_deref() {
                self.conversation_repo
                    .get_by_id(&ChatConversationId::from_string(parent_id.to_string()))
                    .await
                    .map_err(|e| ChatServiceError::RepositoryError(e.to_string()))?
            } else {
                None
            }
        } else {
            None
        };
        let agent_workspace = self
            .load_agent_conversation_workspace(context_type, &conversation.id)
            .await?;
        let entity_status = self.get_entity_status(context_type, context_id).await;
        let team_mode_val = self.team_mode.load(Ordering::Relaxed);
        let agent_conversation_mode = conversation
            .agent_mode
            .or_else(|| agent_workspace.as_ref().map(|workspace| workspace.mode));
        let agent_name = resolve_agent_name_for_send(
            &context_type,
            entity_status.as_deref(),
            team_mode_val,
            options.agent_name_override.as_deref(),
            agent_conversation_mode,
        );
        if matches!(
            agent_conversation_mode,
            Some(AgentConversationWorkspaceMode::Edit | AgentConversationWorkspaceMode::Ideation)
        ) && agent_workspace.is_none()
        {
            return Err(ChatServiceError::SpawnFailed(format!(
                "Agent conversation {} is in {} mode but has no isolated workspace",
                conversation.id,
                agent_conversation_mode.unwrap()
            )));
        }
        let spawn_harness_override = options.harness_override.or_else(|| {
            conversation_spawn_harness_override(
                agent_name,
                context_type,
                task_metadata.as_deref(),
                &conversation,
                parent_conversation.as_ref(),
            )
        });
        tracing::debug!(
            conversation_id = conversation.id.as_str(),
            provider_harness = ?provider_session_ref.as_ref().map(|session_ref| session_ref.harness),
            provider_session_id = ?provider_session_ref.as_ref().map(|session_ref| session_ref.provider_session_id.as_str()),
            trigger_origin = ?task_metadata
                .as_deref()
                .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
                .and_then(|metadata| metadata.get("trigger_origin").and_then(|value| value.as_str().map(str::to_string))),
            parent_provider_harness = ?parent_conversation
                .as_ref()
                .and_then(|parent| parent.provider_session_ref().map(|session_ref| session_ref.harness)),
            "chat_service.send_message conversation (new spawn path)"
        );

        // 2b. Atomic guard: claim the agent slot to prevent TOCTOU race.
        //     If an agent is already registered for this context, queue the message.
        //     Create the AgentRun early so its ID can be stored in the slot for ownership tracking.
        let mut agent_run = AgentRun::new(conversation.id);
        let agent_run_id = agent_run.id.as_str().to_string();
        let run_chain_id = agent_run.run_chain_id.clone();

        let registry_key = RunningAgentKey::new(context_type.to_string(), &runtime_context_id);
        tracing::info!(
            %context_type,
            context_id,
            runtime_context_id = %runtime_context_id,
            gate = "GATE_2_REGISTRY",
            "[GATE_TRACE] Gate 2 (running_agent_registry.try_register)"
        );
        let mut registration_result = self
            .running_agent_registry
            .try_register(
                registry_key.clone(),
                conversation.id.as_str().to_string(),
                agent_run_id.clone(),
            )
            .await;

        if let Err(existing) = registration_result.as_ref() {
            let cleaned_stale_entry = self
                .cleanup_stale_registry_block(
                    &registry_key,
                    existing,
                    context_type,
                    &runtime_context_id,
                    "send_message_gate_2",
                )
                .await;
            let cleaned_inactive_entry = if cleaned_stale_entry {
                false
            } else {
                self.cleanup_inactive_registry_block(
                    &registry_key,
                    existing,
                    context_type,
                    &runtime_context_id,
                    "send_message_gate_2",
                )
                .await
            };
            if cleaned_stale_entry || cleaned_inactive_entry {
                registration_result = self
                    .running_agent_registry
                    .try_register(
                        registry_key.clone(),
                        conversation.id.as_str().to_string(),
                        agent_run_id.clone(),
                    )
                    .await;
            }
        }

        if let Err(existing) = registration_result {
            tracing::warn!(
                %context_type,
                context_id,
                runtime_context_id = %runtime_context_id,
                gate = "GATE_2_BLOCKED",
                existing_pid = existing.pid,
                existing_run_id = %existing.agent_run_id,
                "[GATE_TRACE] Gate 2 blocked — agent already running, queuing message"
            );
            let queued = self.enqueue_pending_send(
                context_type,
                &runtime_context_id,
                message,
                &options,
                Some(existing.conversation_id.clone()),
            );
            return Ok(SendResult {
                conversation_id: existing.conversation_id.clone(),
                agent_run_id: existing.agent_run_id.clone(),
                is_new_conversation: false,
                was_queued: true,
                queued_message_id: Some(queued.id),
                queued_as_pending: false,
            });
        }

        // From here on, we hold the agent slot. Any early return must unregister.
        tracing::info!(
            %context_type,
            context_id,
            runtime_context_id = %runtime_context_id,
            gate = "GATE_3_SPAWN",
            "[GATE_TRACE] Gate 3 reached — no IPR entry, no running agent. Will spawn new process."
        );
        let mut running_incremented = false;

        // Cleanup macro: unregisters slot + decrements running count on failure.
        // Uses textual expansion so `.await` works inside the async fn body.
        macro_rules! cleanup_and_err {
            ($err:expr) => {{
                self.running_agent_registry
                    .unregister(&registry_key, &agent_run_id)
                    .await;
                if running_incremented {
                    if let Some(ref exec) = self.execution_state {
                        exec.decrement_running();
                        if let Some(ref handle) = self.app_handle {
                            exec.emit_status_changed(handle, "slot_cleanup");
                        }
                    }
                }
                return Err($err);
            }};
        }

        if uses_execution_slot(context_type) {
            if let Some(ref exec) = self.execution_state {
                if context_type == ChatContextType::Ideation {
                    let session_id = IdeationSessionId::from_string(context_id.to_string());
                    let session = match self.ideation_session_repo.get_by_id(&session_id).await {
                        Ok(Some(session)) => session,
                        Ok(None) => {
                            cleanup_and_err!(ChatServiceError::RepositoryError(format!(
                                "Ideation session not found: {}",
                                context_id
                            )));
                        }
                        Err(e) => {
                            cleanup_and_err!(ChatServiceError::RepositoryError(e.to_string()))
                        }
                    };

                    let project_settings = if let Some(repo) = self.execution_settings_repo.as_ref()
                    {
                        let project_settings_result = repo
                            .get_settings(Some(&session.project_id))
                            .await
                            .map_err(|e| e.to_string());
                        match project_settings_result {
                            Ok(settings) => settings,
                            Err(error) => {
                                cleanup_and_err!(ChatServiceError::RepositoryError(error))
                            }
                        }
                    } else {
                        crate::domain::execution::ExecutionSettings::default()
                    };

                    let running_global_ideation = match self.count_active_ideation_slots().await {
                        Ok(count) => count,
                        Err(e) => cleanup_and_err!(e),
                    };
                    let running_project_ideation = match self
                        .count_active_ideation_slots_for_project(&session.project_id)
                        .await
                    {
                        Ok(count) => count,
                        Err(e) => cleanup_and_err!(e),
                    };
                    let running_project_total = match self
                        .count_active_slot_consuming_contexts_for_project(&session.project_id)
                        .await
                    {
                        Ok(count) => count,
                        Err(e) => cleanup_and_err!(e),
                    };
                    let global_execution_waiting =
                        match self.has_runnable_execution_waiting(None).await {
                            Ok(waiting) => waiting,
                            Err(e) => cleanup_and_err!(e),
                        };
                    let project_execution_waiting = match self
                        .has_runnable_execution_waiting(Some(&session.project_id))
                        .await
                    {
                        Ok(waiting) => waiting,
                        Err(e) => cleanup_and_err!(e),
                    };

                    if !exec.can_start_ideation(
                        running_global_ideation,
                        running_project_ideation,
                        running_project_total,
                        project_settings.max_concurrent_tasks,
                        project_settings.project_ideation_max,
                        global_execution_waiting,
                        project_execution_waiting,
                    ) {
                        let project_borrow_available = exec.allow_ideation_borrow_idle_execution()
                            && !project_execution_waiting;

                        let capacity_err_msg = if running_project_total
                            >= project_settings.max_concurrent_tasks
                        {
                            format!(
                                "project execution capacity reached ({}/{} active slots)",
                                running_project_total, project_settings.max_concurrent_tasks
                            )
                        } else if project_settings.project_ideation_max == 0
                            || (running_project_ideation >= project_settings.project_ideation_max
                                && !project_borrow_available)
                        {
                            format!(
                                    "project ideation capacity reached ({}/{} active ideation slots in project)",
                                    running_project_ideation, project_settings.project_ideation_max
                                )
                        } else {
                            format!(
                                "ideation capacity reached ({}/{} active ideation slots)",
                                running_global_ideation,
                                exec.global_ideation_max()
                            )
                        };

                        if options.caller_context == SendCallerContext::UserInitiated {
                            // Try to persist the user's message as pending_initial_prompt so
                            // the drain service can launch the session when capacity frees up.
                            // `running_incremented` is still false here (capacity check fires
                            // before exec.increment_running), so cleanup is just registry
                            // unregister.
                            match self
                                .ideation_session_repo
                                .set_pending_initial_prompt_if_unset(
                                    context_id,
                                    message.to_string(),
                                )
                                .await
                            {
                                Ok(true) => {
                                    // Persisted — release the registry slot and return queued.
                                    self.running_agent_registry
                                        .unregister(&registry_key, &agent_run_id)
                                        .await;
                                    tracing::info!(
                                        %context_type,
                                        context_id,
                                        "send_message: capacity full, \
                                         message persisted as pending_initial_prompt"
                                    );
                                    return Ok(SendResult {
                                        conversation_id: conversation.id.as_str().to_string(),
                                        agent_run_id: agent_run_id.clone(),
                                        is_new_conversation: spawn_path_is_new_conversation,
                                        was_queued: true,
                                        queued_as_pending: true,
                                        queued_message_id: None,
                                    });
                                }
                                Ok(false) => {
                                    // Multi-message guard: a prompt is already set, reject.
                                    tracing::warn!(
                                        %context_type,
                                        context_id,
                                        "send_message: capacity full and \
                                         pending_initial_prompt already set — rejecting"
                                    );
                                    cleanup_and_err!(ChatServiceError::SpawnFailed(
                                        capacity_err_msg
                                    ));
                                }
                                Err(e) => {
                                    // Persist failed — surface error so the frontend keeps the
                                    // message in the input field for retry (never lose silently).
                                    tracing::error!(
                                        %context_type,
                                        context_id,
                                        error = %e,
                                        "send_message: capacity full and persist failed — \
                                         returning SpawnFailed to caller"
                                    );
                                    cleanup_and_err!(ChatServiceError::SpawnFailed(
                                        capacity_err_msg
                                    ));
                                }
                            }
                        } else {
                            // DrainService caller: propagate Err so drain breaks cleanly
                            // and does not re-persist (it already handles that itself).
                            cleanup_and_err!(ChatServiceError::SpawnFailed(capacity_err_msg));
                        }
                    }
                } else {
                    let task_id = TaskId::from_string(context_id.to_string());
                    let task = match self.task_repo.get_by_id(&task_id).await {
                        Ok(Some(task)) => task,
                        Ok(None) => {
                            cleanup_and_err!(ChatServiceError::RepositoryError(format!(
                                "Task not found: {}",
                                context_id
                            )));
                        }
                        Err(e) => {
                            cleanup_and_err!(ChatServiceError::RepositoryError(e.to_string()))
                        }
                    };

                    let project_settings = if let Some(repo) = self.execution_settings_repo.as_ref()
                    {
                        let project_settings_result = repo
                            .get_settings(Some(&task.project_id))
                            .await
                            .map_err(|e| e.to_string());
                        match project_settings_result {
                            Ok(settings) => settings,
                            Err(error) => {
                                cleanup_and_err!(ChatServiceError::RepositoryError(error))
                            }
                        }
                    } else {
                        crate::domain::execution::ExecutionSettings::default()
                    };

                    let running_project_total = match self
                        .count_active_slot_consuming_contexts_for_project(&task.project_id)
                        .await
                    {
                        Ok(count) => count,
                        Err(e) => cleanup_and_err!(e),
                    };

                    if !exec.can_start_execution_context(
                        running_project_total,
                        project_settings.max_concurrent_tasks,
                    ) {
                        let message =
                            if running_project_total >= project_settings.max_concurrent_tasks {
                                format!(
                                    "project execution capacity reached ({}/{} active slots)",
                                    running_project_total, project_settings.max_concurrent_tasks
                                )
                            } else {
                                format!(
                                    "execution capacity reached ({}/{} active slots)",
                                    exec.running_count(),
                                    exec.global_max_concurrent()
                                )
                            };
                        cleanup_and_err!(ChatServiceError::SpawnFailed(message));
                    }
                }
            }
        }

        let conversation_id = conversation.id;

        // 2a. Update state history metadata for task-related contexts
        // This links the conversation_id and agent_run_id to the state history entry,
        // enabling history navigation to show the correct conversation for each state.
        // Best-effort: don't fail send_message if metadata update fails.
        if matches!(
            context_type,
            ChatContextType::TaskExecution | ChatContextType::Review | ChatContextType::Merge
        ) {
            let task_id = TaskId::from_string(context_id.to_string());
            let metadata = StateHistoryMetadata {
                conversation_id: conversation_id.as_str().to_string(),
                agent_run_id: agent_run_id.clone(),
            };
            // Ignore errors - state history metadata is non-critical for message flow
            let _ = self
                .task_repo
                .update_latest_state_history_metadata(&task_id, &metadata)
                .await;
        }

        // 3. run_started event emitted below at step 7b-pre4 after model resolution
        // so that effective_model_id / effective_model_label can be included in the payload.

        let resume_in_place = resume_in_place_requested(options.metadata.as_deref());
        let persisted_metadata = strip_resume_in_place_metadata(options.metadata.clone());

        // 4. Store user message
        if !resume_in_place {
            let user_msg = chat_service_context::create_user_message(
                context_type,
                context_id,
                message,
                conversation_id,
                persisted_metadata.clone(),
                options.created_at,
            );
            let user_msg_id = user_msg.id.as_str().to_string();
            let user_msg_created_at = user_msg.created_at.to_rfc3339();
            if let Err(e) = self.chat_message_repo.create(user_msg).await {
                cleanup_and_err!(ChatServiceError::RepositoryError(e.to_string()));
            }
            if context_type == ChatContextType::Ideation {
                let _ = self
                    .ideation_session_repo
                    .touch_updated_at(context_id)
                    .await;
            }
            tracing::debug!(
                message_id = %user_msg_id,
                "chat_service.send_message user message stored"
            );

            // 4b. Link pending attachments to the user message
            let pending_attachments = match self
                .chat_attachment_repo
                .find_by_conversation_id(&conversation_id)
                .await
            {
                Ok(v) => v
                    .into_iter()
                    .filter(|a| a.message_id.is_none())
                    .collect::<Vec<_>>(),
                Err(e) => {
                    cleanup_and_err!(ChatServiceError::RepositoryError(e.to_string()));
                }
            };

            if !pending_attachments.is_empty() {
                let attachment_ids: Vec<_> =
                    pending_attachments.iter().map(|a| a.id.clone()).collect();
                if let Err(e) = self
                    .chat_attachment_repo
                    .update_message_ids(&attachment_ids, &ChatMessageId::from_string(&user_msg_id))
                    .await
                {
                    cleanup_and_err!(ChatServiceError::RepositoryError(e.to_string()));
                }
                tracing::debug!(
                    message_id = %user_msg_id,
                    attachment_count = pending_attachments.len(),
                    "chat_service.send_message linked attachments to user message"
                );
            }

            // 5. Emit message created event
            self.emit_event(
                "agent:message_created",
                AgentMessageCreatedPayload {
                    message_id: user_msg_id.clone(),
                    conversation_id: conversation_id.as_str().to_string(),
                    context_type: context_type.to_string(),
                    context_id: context_id.to_string(),
                    role: "user".to_string(),
                    content: message.to_string(),
                    created_at: Some(user_msg_created_at),
                    metadata: persisted_metadata.clone(),
                },
            );
        }

        // 6. Resolve working directory
        let has_working_directory_override = options.working_directory_override.is_some();
        let mut working_directory =
            if let Some(override_path) = options.working_directory_override.as_ref() {
                override_path.clone()
            } else if let Some(workspace) = agent_workspace.as_ref() {
                match self
                    .resolve_agent_workspace_working_directory(workspace)
                    .await
                {
                    Ok(dir) => dir,
                    Err(e) => {
                        cleanup_and_err!(e);
                    }
                }
            } else {
                match self
                    .resolve_working_directory(context_type, context_id)
                    .await
                {
                    Ok(dir) => dir,
                    Err(e) => {
                        cleanup_and_err!(ChatServiceError::SpawnFailed(e));
                    }
                }
            };
        if !working_directory.exists() || !working_directory.is_dir() {
            if agent_workspace.is_some() || has_working_directory_override {
                cleanup_and_err!(ChatServiceError::SpawnFailed(format!(
                    "Agent conversation workspace is missing: {}",
                    working_directory.display()
                )));
            }
            tracing::warn!(
                context_type = ?context_type,
                context_id = context_id,
                missing = %working_directory.display(),
                default = %self.default_working_directory.display(),
                "chat_service.send_message: resolved working_directory does not exist, \
                 falling back to default. Agent may operate in unexpected directory."
            );
            working_directory = self.default_working_directory.clone();
        }
        tracing::debug!(
            working_directory = %working_directory.display(),
            "chat_service.send_message working_directory resolved"
        );

        // 6a. Resolve project ID for RALPHX_PROJECT_ID env var
        let project_id = chat_service_context::resolve_project_id(
            context_type,
            context_id,
            Arc::clone(&self.task_repo),
            Arc::clone(&self.ideation_session_repo),
            Arc::clone(&self.delegated_session_repo),
        )
        .await;

        if context_type == ChatContextType::Ideation {
            let lane_repo = self.agent_lane_settings_repo.as_ref().ok_or_else(|| {
                ChatServiceError::SpawnFailed(
                    "Unified ideation chat service requires agent lane settings repo".to_string(),
                )
            })?;
            let lane_availability =
                crate::application::resolve_primary_ideation_harness_availability(
                    lane_repo,
                    project_id.as_deref(),
                )
                .await;
            if !lane_availability.available {
                let error = lane_availability
                    .error
                    .clone()
                    .unwrap_or_else(|| "Configured ideation harness is not available".to_string());
                cleanup_and_err!(ChatServiceError::SpawnFailed(error));
            }
        }

        // 7. Increment running count for task execution contexts BEFORE spawning
        // This tracks concurrency for agent-active states (Executing, Reviewing, ReExecuting)
        // The count is decremented in TransitionHandler::on_exit when leaving these states
        // IMPORTANT: Must increment before spawn to ensure scheduling respects capacity
        if uses_execution_slot(context_type) {
            if let Some(ref exec) = self.execution_state {
                exec.increment_running();
                running_incremented = true;
                // Emit status_changed event to frontend for real-time UI update
                if let Some(ref handle) = self.app_handle {
                    exec.emit_status_changed(handle, "task_started");
                }
            }
        }

        // 7a. Build and spawn command
        let mut resolved_spawn_settings =
            crate::application::agent_lane_resolution::resolve_agent_spawn_settings(
                agent_name,
                project_id.as_deref(),
                context_type,
                entity_status.as_deref(),
                spawn_harness_override,
                options.model_override.as_deref(),
                self.agent_lane_settings_repo.as_ref(),
            )
            .await;
        apply_send_message_overrides(&mut resolved_spawn_settings, &options);
        let runtime_team_mode = chat_service_helpers::effective_team_mode_for_harness(
            team_mode_val,
            resolved_spawn_settings.effective_harness,
        );
        if team_mode_val && !runtime_team_mode {
            tracing::info!(
                %context_type,
                context_id,
                harness = %resolved_spawn_settings.effective_harness,
                "Disabling team mode because the selected harness does not support it"
            );
        }
        let stored_provider_session = if options.force_new_provider_session {
            None
        } else {
            conversation.provider_session_ref().filter(|session_ref| {
                session_ref.harness == resolved_spawn_settings.effective_harness
            })
        };
        let stored_session_id = stored_provider_session
            .as_ref()
            .map(|session_ref| session_ref.provider_session_id.clone());
        let is_new_conversation = stored_session_id.is_none();
        let resolved_agent_name = agent_name.to_string();
        let (upstream_provider, provider_profile) =
            chat_service_helpers::provider_origin_for_harness(
                resolved_spawn_settings.effective_harness,
                Some(&resolved_agent_name),
            );

        if conversation.upstream_provider != upstream_provider
            || conversation.provider_profile != provider_profile
        {
            if let Err(error) = self
                .conversation_repo
                .update_provider_origin(
                    &conversation.id,
                    upstream_provider.as_deref(),
                    provider_profile.as_deref(),
                )
                .await
            {
                cleanup_and_err!(ChatServiceError::RepositoryError(error.to_string()));
            }
            conversation.set_provider_origin(upstream_provider.clone(), provider_profile.clone());
        }

        agent_run.harness = Some(resolved_spawn_settings.effective_harness);
        agent_run.provider_session_id = stored_session_id.clone();
        agent_run.upstream_provider = upstream_provider.clone();
        agent_run.provider_profile = provider_profile.clone();
        agent_run.logical_model = resolved_spawn_settings.configured_model.clone();
        agent_run.effective_model_id = Some(resolved_spawn_settings.model.clone());
        agent_run.logical_effort = resolved_spawn_settings.configured_logical_effort;
        agent_run.effective_effort = Some(chat_service_helpers::effective_effort_for_harness(
            resolved_spawn_settings.effective_harness,
            resolved_spawn_settings.claude_effort.as_deref(),
            resolved_spawn_settings.logical_effort,
        ));
        agent_run.approval_policy = resolved_spawn_settings.approval_policy.clone();
        agent_run.sandbox_mode = resolved_spawn_settings.sandbox_mode.clone();

        // Persist agent run record after the effective harness/model metadata is populated.
        if let Err(e) = self.agent_run_repo.create(agent_run).await {
            cleanup_and_err!(ChatServiceError::RepositoryError(e.to_string()));
        }
        tracing::debug!(
            run_id = %agent_run_id,
            "chat_service.send_message agent_run created"
        );

        let effective_model_id = resolved_spawn_settings.model.clone();
        let effective_model_label = Some(chat_service_helpers::effective_model_label_for_harness(
            resolved_spawn_settings.effective_harness,
            &effective_model_id,
        ));

        // 3. Emit run started event (deferred from step 3 to include effective model info)
        self.emit_event(
            "agent:run_started",
            AgentRunStartedPayload::with_provider_session(
                agent_run_id.clone(),
                conversation_id.as_str().to_string(),
                context_type.to_string(),
                context_id.to_string(),
                run_chain_id.clone(),
                None,
                Some(effective_model_id.clone()),
                effective_model_label,
                Some(resolved_spawn_settings.effective_harness),
                stored_session_id.clone(),
            ),
        );

        // Fetch recent session messages for Ideation context ONLY when spawning a new process.
        // The agent has no prior context at spawn time, so we inject the history into the prompt.
        // For non-ideation contexts and already-running agents (IPR path above), we pass empty slice.
        let (session_messages, session_total) = if context_type == ChatContextType::Ideation {
            let session_id = IdeationSessionId::from_string(context_id.to_string());
            let total = self
                .chat_message_repo
                .count_by_session(&session_id)
                .await
                .unwrap_or(0);
            if total > 0 {
                let msgs = self
                    .chat_message_repo
                    .get_recent_by_session(
                        &session_id,
                        chat_service_context::SESSION_HISTORY_LIMIT as u32,
                    )
                    .await
                    .unwrap_or_default();
                (msgs, total as usize)
            } else {
                (vec![], 0usize)
            }
        } else {
            (vec![], 0usize)
        };
        let (selected_cli_path, child, interactive_process_registry) = match self
            .spawn_process_for_harness(
                &conversation,
                message,
                Some(resolved_agent_name.as_str()),
                context_type,
                context_id,
                &runtime_context_id,
                &working_directory,
                entity_status.as_deref(),
                project_id.as_deref(),
                &session_messages,
                session_total,
                options.is_external_mcp,
                runtime_team_mode,
                stored_session_id.as_deref(),
                &resolved_spawn_settings,
            )
            .await
        {
            Ok(result) => result,
            Err(error) => cleanup_and_err!(error),
        };

        // Register verification child PID for explicit cleanup after reconciliation (Fix A).
        // Only for Ideation sessions with SessionPurpose::Verification.
        if context_type == ChatContextType::Ideation {
            if let Some(pid) = child.id() {
                let child_session_id =
                    crate::domain::entities::IdeationSessionId::from_string(context_id.to_string());
                match self
                    .ideation_session_repo
                    .get_by_id(&child_session_id)
                    .await
                {
                    Ok(Some(session))
                        if session.session_purpose == SessionPurpose::Verification =>
                    {
                        self.verification_child_registry.register(context_id, pid);
                        tracing::info!(
                            context_id,
                            pid,
                            "Registered verification child PID for post-reconcile cleanup"
                        );
                    }
                    _ => {} // Not a verification session — do not register
                }
            }
        }

        // Spawn merge completion watcher for Merge context
        if context_type == ChatContextType::Merge
            && chat_service_helpers::harness_supports_merge_completion_watcher(
                resolved_spawn_settings.effective_harness,
            )
        {
            chat_service_merge::spawn_merge_completion_watcher(
                context_id.to_string(),
                working_directory.clone(),
                self.ipr(),
                Arc::clone(&self.task_repo),
                Arc::clone(&self.project_repo),
                self.plan_branch_repo.lock().unwrap().clone(),
            );
        }

        let registry_worktree = working_directory.to_string_lossy().to_string();

        // 7b. Update process details in registry now that spawn succeeded
        let cancellation_token = CancellationToken::new();
        if let Some(pid) = child.id() {
            if let Err(e) = self
                .running_agent_registry
                .update_agent_process(
                    &registry_key,
                    pid,
                    &conversation_id.as_str(),
                    &agent_run_id,
                    Some(registry_worktree.clone()),
                    Some(cancellation_token.clone()),
                    Some(effective_model_id.clone()),
                )
                .await
            {
                tracing::error!(
                    pid,
                    error = %e,
                    "chat_service.send_message: failed to update agent process in registry — slot claimed but PID not persisted"
                );
            }
        }

        // 7c. Persist effective model to ideation_sessions (non-fatal, WARN on failure)
        if context_type == ChatContextType::Ideation {
            if let Err(e) = self
                .ideation_session_repo
                .update_last_effective_model(context_id, &effective_model_id)
                .await
            {
                tracing::warn!(
                    context_id,
                    effective_model = %effective_model_id,
                    error = %e,
                    "chat_service.send_message: failed to persist last_effective_model — non-fatal"
                );
            }
        }

        // 8. Build background context and spawn
        let bg_ctx = chat_service_send_background::BackgroundRunContext {
            child,
            harness: resolved_spawn_settings.effective_harness,
            context_type,
            context_id: context_id.to_string(),
            runtime_context_id: runtime_context_id.clone(),
            conversation_id,
            agent_run_id: agent_run_id.clone(),
            stored_session_id: stored_session_id.clone(),
            working_directory,
            cli_path: selected_cli_path,
            plugin_dir: self.plugin_dir.clone(),
            repos: chat_service_send_background::BackgroundRunRepos {
                chat_message_repo: Arc::clone(&self.chat_message_repo),
                chat_attachment_repo: Arc::clone(&self.chat_attachment_repo),
                artifact_repo: Arc::clone(&self.artifact_repo),
                conversation_repo: Arc::clone(&self.conversation_repo),
                agent_run_repo: Arc::clone(&self.agent_run_repo),
                task_repo: Arc::clone(&self.task_repo),
                task_dependency_repo: Arc::clone(&self.task_dependency_repo),
                project_repo: Arc::clone(&self.project_repo),
                ideation_session_repo: Arc::clone(&self.ideation_session_repo),
                delegated_session_repo: Arc::clone(&self.delegated_session_repo),
                execution_settings_repo: self.execution_settings_repo.clone(),
                agent_lane_settings_repo: self.agent_lane_settings_repo.clone(),
                ideation_effort_settings_repo: self.ideation_effort_settings_repo.clone(),
                ideation_model_settings_repo: self.ideation_model_settings_repo.clone(),
                agent_conversation_workspace_repo: self
                    .agent_conversation_workspace_repo
                    .lock()
                    .unwrap()
                    .clone(),
                activity_event_repo: Arc::clone(&self.activity_event_repo),
                memory_event_repo: Arc::clone(&self.memory_event_repo),
                message_queue: Arc::clone(&self.message_queue),
                running_agent_registry: Arc::clone(&self.running_agent_registry),
                task_proposal_repo: self.task_proposal_repo.clone(),
                task_step_repo: self.task_step_repo.clone(),
                review_repo: self.review_repo.clone(),
            },
            execution_state: self.execution_state.clone(),
            question_state: self.question_state.clone(),
            plan_branch_repo: self.plan_branch_repo.lock().unwrap().clone(),
            app_handle: self.app_handle.clone(),
            run_chain_id,
            is_retry_attempt: false,
            user_message_content: Some(message.to_string()),
            conversation: Some(conversation.clone()),
            agent_name: Some(resolved_agent_name),
            team_mode: runtime_team_mode,
            assistant_message_attribution: crate::domain::entities::ChatMessageAttribution {
                attribution_source: Some("native_runtime".to_string()),
                provider_harness: Some(resolved_spawn_settings.effective_harness),
                provider_session_id: stored_session_id.clone(),
                upstream_provider,
                provider_profile,
                logical_model: resolved_spawn_settings.configured_model.clone(),
                effective_model_id: Some(effective_model_id.clone()),
                logical_effort: resolved_spawn_settings.configured_logical_effort,
                effective_effort: Some(chat_service_helpers::effective_effort_for_harness(
                    resolved_spawn_settings.effective_harness,
                    resolved_spawn_settings.claude_effort.as_deref(),
                    resolved_spawn_settings.logical_effort,
                )),
            },
            persist_conversation_provider_session_ref: !options
                .preserve_conversation_provider_session_ref,
            cancellation_token,
            team_service: self.team_service.clone(),
            streaming_state_cache: self.streaming_state_cache.clone(),
            interactive_process_registry,
            verification_child_registry: Some(Arc::clone(&self.verification_child_registry)),
        };

        // 9. Process stream in background (extracted to separate module)
        chat_service_send_background::spawn_send_message_background(bg_ctx);
        tracing::debug!(
            conversation_id = conversation_id.as_str(),
            "chat_service.send_message background spawn kicked"
        );

        // Return immediately
        Ok(SendResult {
            conversation_id: conversation_id.as_str().to_string(),
            agent_run_id,
            is_new_conversation,
            ..Default::default()
        })
    }

    async fn queue_message(
        &self,
        context_type: ChatContextType,
        context_id: &str,
        content: &str,
        client_id: Option<&str>,
    ) -> Result<QueuedMessage, ChatServiceError> {
        // Interactive fast-path: if an interactive process exists, send immediately
        // instead of queuing. The Claude CLI handles internal message queuing mid-turn.
        let interactive_key = InteractiveProcessKey::new(context_type.to_string(), context_id);
        if self.ipr().has_process(&interactive_key).await {
            tracing::info!(
                %context_type,
                context_id,
                "queue_message: interactive process found, sending immediately via stdin"
            );

            // Agent is already running — no session history needed here.
            let stdin_prompt = chat_service_context::build_initial_prompt(
                context_type,
                context_id,
                content,
                &[],
                0,
            );
            let stream_json_msg =
                crate::infrastructure::agents::claude::format_stream_json_input(&stdin_prompt);

            match self
                .ipr()
                .write_message(&interactive_key, &stream_json_msg)
                .await
            {
                Ok(()) => {
                    // Re-increment running count only if the process was idle.
                    // Same guard as send_message fast-path: prevents double-increment.
                    if uses_execution_slot(context_type) {
                        if let Some(ref exec) = self.execution_state {
                            let slot_key = format!("{}/{}", context_type, context_id);
                            if exec.claim_interactive_slot(&slot_key) {
                                exec.increment_running();
                                if let Some(ref handle) = self.app_handle {
                                    exec.emit_status_changed(handle, "interactive_turn_resumed");
                                }
                            }
                        }
                    }

                    // Use the EXISTING conversation — not a force-fresh one.
                    // The interactive process was spawned with a conversation, so
                    // get_active_for_context should always find it.
                    let existing_conv = self
                        .conversation_repo
                        .get_active_for_context(context_type, context_id)
                        .await
                        .map_err(|e| ChatServiceError::RepositoryError(e.to_string()))?;

                    let conversation = match existing_conv {
                        Some(conv) => {
                            tracing::debug!(
                                conversation_id = conv.id.as_str(),
                                "queue_message: reusing existing conversation for interactive process"
                            );
                            conv
                        }
                        None => {
                            // Edge case: IPR has process but no conversation found.
                            // Create one as fallback (shouldn't happen in practice).
                            tracing::warn!(
                                %context_type,
                                context_id,
                                "queue_message: no existing conversation found despite IPR entry, creating new"
                            );
                            let (conversation, _) = self
                                .get_or_create_conversation(context_type, context_id)
                                .await?;
                            conversation
                        }
                    };
                    let user_msg = chat_service_context::create_user_message(
                        context_type,
                        context_id,
                        content,
                        conversation.id,
                        None,
                        None,
                    );
                    let user_msg_id = user_msg.id.as_str().to_string();
                    let user_msg_created_at = user_msg.created_at.to_rfc3339();
                    let _ = self.chat_message_repo.create(user_msg).await;

                    if context_type == ChatContextType::Ideation {
                        let _ = self
                            .ideation_session_repo
                            .touch_updated_at(context_id)
                            .await;
                    }

                    // Emit message_created so frontend shows the user message
                    self.emit_event(
                        "agent:message_created",
                        AgentMessageCreatedPayload {
                            message_id: user_msg_id,
                            conversation_id: conversation.id.as_str().to_string(),
                            context_type: context_type.to_string(),
                            context_id: context_id.to_string(),
                            role: "user".to_string(),
                            content: content.to_string(),
                            created_at: Some(user_msg_created_at),
                            metadata: None,
                        },
                    );

                    // Build a QueuedMessage for API compatibility
                    let msg_id = client_id
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
                    let queued_msg = QueuedMessage::with_id(msg_id.clone(), content.to_string());

                    // Emit queue_sent to remove from frontend optimistic queue UI
                    self.emit_event(
                        "agent:queue_sent",
                        AgentQueueSentPayload {
                            message_id: msg_id,
                            conversation_id: conversation.id.as_str().to_string(),
                            context_type: context_type.to_string(),
                            context_id: context_id.to_string(),
                        },
                    );

                    return Ok(queued_msg);
                }
                Err(e) => {
                    tracing::warn!(
                        %context_type,
                        context_id,
                        error = %e,
                        "queue_message: interactive stdin write failed, falling back to normal queue"
                    );
                    // Remove broken entry, fall through to normal queue
                    self.ipr().remove(&interactive_key).await;
                }
            }
        }

        // Normal queue path (no interactive process or stdin write failed)
        Ok(match client_id {
            Some(id) => self.message_queue.queue_with_client_id(
                context_type,
                context_id,
                content.to_string(),
                id.to_string(),
            ),
            None => self
                .message_queue
                .queue(context_type, context_id, content.to_string()),
        })
    }

    async fn get_queued_messages(
        &self,
        context_type: ChatContextType,
        context_id: &str,
    ) -> Result<Vec<QueuedMessage>, ChatServiceError> {
        Ok(self.message_queue.get_queued(context_type, context_id))
    }

    async fn delete_queued_message(
        &self,
        context_type: ChatContextType,
        context_id: &str,
        message_id: &str,
    ) -> Result<bool, ChatServiceError> {
        Ok(self
            .message_queue
            .delete(context_type, context_id, message_id))
    }

    async fn get_or_create_conversation(
        &self,
        context_type: ChatContextType,
        context_id: &str,
    ) -> Result<(ChatConversation, bool), ChatServiceError> {
        let (conv, is_new) = chat_service_repository::get_or_create_conversation(
            Arc::clone(&self.conversation_repo),
            context_type,
            context_id,
        )
        .await?;
        if is_new {
            self.emit_event(
                "agent:conversation_created",
                AgentConversationCreatedPayload {
                    conversation_id: conv.id.as_str().to_string(),
                    context_type: context_type.to_string(),
                    context_id: context_id.to_string(),
                },
            );
        }
        Ok((conv, is_new))
    }

    async fn get_conversation_with_messages(
        &self,
        conversation_id: &ChatConversationId,
    ) -> Result<Option<ChatConversationWithMessages>, ChatServiceError> {
        chat_service_repository::get_conversation_with_messages(
            Arc::clone(&self.conversation_repo),
            Arc::clone(&self.chat_message_repo),
            conversation_id,
        )
        .await
    }

    async fn list_conversations(
        &self,
        context_type: ChatContextType,
        context_id: &str,
    ) -> Result<Vec<ChatConversation>, ChatServiceError> {
        chat_service_repository::list_conversations(
            Arc::clone(&self.conversation_repo),
            context_type,
            context_id,
        )
        .await
    }

    async fn get_active_run(
        &self,
        conversation_id: &ChatConversationId,
    ) -> Result<Option<AgentRun>, ChatServiceError> {
        self.agent_run_repo
            .get_active_for_conversation(conversation_id)
            .await
            .map_err(|e| ChatServiceError::RepositoryError(e.to_string()))
    }

    async fn is_available(&self) -> bool {
        default_harness_runtime_available()
    }

    async fn stop_agent(
        &self,
        context_type: ChatContextType,
        context_id: &str,
    ) -> Result<bool, ChatServiceError> {
        let key = RunningAgentKey::new(context_type.to_string(), context_id);

        // Also remove from interactive process registry (closes stdin pipe)
        let interactive_key = InteractiveProcessKey::new(context_type.to_string(), context_id);
        self.ipr().remove(&interactive_key).await;

        match self.running_agent_registry.stop(&key).await {
            Ok(Some(info)) => {
                // Emit stopped event
                self.emit_event(
                    "agent:stopped",
                    serde_json::json!({
                        "conversation_id": info.conversation_id,
                        "agent_run_id": info.agent_run_id,
                        "context_type": context_type.to_string(),
                        "context_id": context_id,
                    }),
                );

                // Mark the agent run as failed with a stopped message
                let _ = self
                    .agent_run_repo
                    .fail(
                        &crate::domain::entities::AgentRunId::from_string(&info.agent_run_id),
                        "Agent stopped by user",
                    )
                    .await;

                // Also emit run_completed so frontend knows agent is no longer running
                self.emit_event(
                    "agent:run_completed",
                    AgentRunCompletedPayload::with_provider_session(
                        info.conversation_id,
                        context_type.to_string(),
                        context_id.to_string(),
                        None,
                        None,
                        None,
                    ),
                );

                Ok(true)
            }
            Ok(None) => {
                // No agent was running
                Ok(false)
            }
            Err(e) => Err(ChatServiceError::AgentRunFailed(e)),
        }
    }

    async fn is_agent_running(&self, context_type: ChatContextType, context_id: &str) -> bool {
        let key = RunningAgentKey::new(context_type.to_string(), context_id);
        let Some(info) = self.running_agent_registry.get(&key).await else {
            return false;
        };

        if self
            .cleanup_stale_registry_block(&key, &info, context_type, context_id, "is_agent_running")
            .await
            || self
                .cleanup_inactive_registry_block(
                    &key,
                    &info,
                    context_type,
                    context_id,
                    "is_agent_running",
                )
                .await
        {
            return false;
        }

        true
    }

    fn set_team_mode(&self, mode: bool) {
        self.team_mode.store(mode, Ordering::Relaxed);
    }

    fn set_plan_branch_repo(&self, repo: Arc<dyn PlanBranchRepository>) {
        *self.plan_branch_repo.lock().unwrap() = Some(repo);
    }

    fn set_interactive_process_registry(&self, registry: Arc<InteractiveProcessRegistry>) {
        *self.interactive_process_registry.lock().unwrap() = registry;
    }
}

// ============================================================================
// Module re-exports are at the top of this file
// ============================================================================

#[cfg(test)]
mod stale_registry_gate_tests {
    use super::{
        claude_launches_paused, registry_entry_blocks_send_because_run_inactive,
        registry_entry_blocks_send_but_is_stale, runtime_context_id_for_send, AgentRunStatus,
        ChatContextType, ChatConversationId, RunningAgentInfo,
    };
    use crate::commands::ExecutionState;
    use std::sync::Arc;

    fn registry_info(pid: u32, started_at: chrono::DateTime<chrono::Utc>) -> RunningAgentInfo {
        RunningAgentInfo {
            pid,
            conversation_id: "conv-1".to_string(),
            agent_run_id: "run-1".to_string(),
            started_at,
            worktree_path: None,
            cancellation_token: None,
            last_active_at: None,
            model: None,
        }
    }

    #[test]
    fn project_send_with_explicit_conversation_uses_conversation_runtime_key() {
        let conversation_id =
            ChatConversationId::from_string("11111111-1111-1111-1111-111111111111".to_string());

        assert_eq!(
            runtime_context_id_for_send(
                ChatContextType::Project,
                "project-1",
                Some(&conversation_id),
            ),
            "11111111-1111-1111-1111-111111111111"
        );
    }

    #[test]
    fn project_send_without_explicit_conversation_uses_project_runtime_key() {
        assert_eq!(
            runtime_context_id_for_send(ChatContextType::Project, "project-1", None),
            "project-1"
        );
    }

    #[test]
    fn non_project_send_keeps_context_runtime_key() {
        let conversation_id =
            ChatConversationId::from_string("11111111-1111-1111-1111-111111111111".to_string());

        assert_eq!(
            runtime_context_id_for_send(
                ChatContextType::Ideation,
                "session-1",
                Some(&conversation_id),
            ),
            "session-1"
        );
    }

    #[test]
    fn paused_execution_blocks_slot_consuming_contexts() {
        let execution_state = Arc::new(ExecutionState::new());
        execution_state.pause();

        assert!(claude_launches_paused(
            ChatContextType::TaskExecution,
            Some(&execution_state),
        ));
        assert!(claude_launches_paused(
            ChatContextType::Ideation,
            Some(&execution_state),
        ));
        assert!(claude_launches_paused(
            ChatContextType::Review,
            Some(&execution_state),
        ));
        assert!(claude_launches_paused(
            ChatContextType::Merge,
            Some(&execution_state),
        ));
        assert!(claude_launches_paused(
            ChatContextType::Project,
            Some(&execution_state),
        ));
        assert!(claude_launches_paused(
            ChatContextType::Task,
            Some(&execution_state),
        ));
    }

    #[test]
    fn paused_execution_does_not_block_regular_chat_contexts() {
        let execution_state = Arc::new(ExecutionState::new());
        execution_state.pause();

        assert!(!claude_launches_paused(
            ChatContextType::Delegation,
            Some(&execution_state),
        ));
    }

    #[test]
    fn young_pid_zero_registry_entry_is_not_cleaned_before_spawn_finishes() {
        let now = chrono::Utc::now();
        let info = registry_info(pid_zero(), now - chrono::Duration::seconds(5));

        assert!(!registry_entry_blocks_send_but_is_stale(&info, now));
    }

    #[test]
    fn old_pid_zero_registry_entry_is_treated_as_stale() {
        let now = chrono::Utc::now();
        let info = registry_info(pid_zero(), now - chrono::Duration::seconds(31));

        assert!(registry_entry_blocks_send_but_is_stale(&info, now));
    }

    #[test]
    fn current_process_registry_entry_is_not_stale() {
        let now = chrono::Utc::now();
        let info = registry_info(std::process::id(), now - chrono::Duration::minutes(5));

        assert!(!registry_entry_blocks_send_but_is_stale(&info, now));
    }

    #[test]
    fn running_agent_run_keeps_registry_entry_active() {
        let now = chrono::Utc::now();
        let info = registry_info(pid_zero(), now - chrono::Duration::minutes(5));

        assert!(!registry_entry_blocks_send_because_run_inactive(
            &info,
            Some(AgentRunStatus::Running),
            now,
        ));
    }

    #[test]
    fn terminal_agent_run_unblocks_registry_entry() {
        let now = chrono::Utc::now();
        let info = registry_info(std::process::id(), now - chrono::Duration::minutes(5));

        assert!(registry_entry_blocks_send_because_run_inactive(
            &info,
            Some(AgentRunStatus::Completed),
            now,
        ));
        assert!(registry_entry_blocks_send_because_run_inactive(
            &info,
            Some(AgentRunStatus::Failed),
            now,
        ));
        assert!(registry_entry_blocks_send_because_run_inactive(
            &info,
            Some(AgentRunStatus::Cancelled),
            now,
        ));
    }

    #[test]
    fn young_missing_agent_run_does_not_unblock_in_flight_registration() {
        let now = chrono::Utc::now();
        let info = registry_info(pid_zero(), now - chrono::Duration::seconds(5));

        assert!(!registry_entry_blocks_send_because_run_inactive(
            &info, None, now
        ));
    }

    #[test]
    fn old_missing_agent_run_unblocks_registry_entry() {
        let now = chrono::Utc::now();
        let info = registry_info(pid_zero(), now - chrono::Duration::seconds(31));

        assert!(registry_entry_blocks_send_because_run_inactive(
            &info, None, now
        ));
    }

    fn pid_zero() -> u32 {
        0
    }
}

#[cfg(test)]
mod chat_service_redaction_tests;
#[cfg(test)]
mod freshness_routing_tests;
#[cfg(test)]
mod interactive_runtime_tests;
