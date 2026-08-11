// Success/error handler logic for background send processing.
//
// Extracted from chat_service_send_background.rs to reduce file size.
// Contains:
// - handle_stream_success: task transitions (TaskExecution → PendingReview/Failed)
//   and merge auto-completion after successful stream processing
// - handle_stream_error: error classification, stale session recovery retry,
//   agent run failure recording, message finalization, and fallback task transitions

use ralphx_events::{emit_serialized, EventSink};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::application::agent_runtime_context::{
    compose_agent_runtime_context, AgentRuntimeContextScope,
};
use crate::application::git_service::GitService;
use crate::application::notification_service::NotificationService;
use crate::application::persona_resolver::resolve_persona_for_send;
use crate::application::question_state::QuestionState;
use crate::application::runtime_factory::{
    build_task_scheduler_from_deps, build_transition_service_from_deps, ChatRuntimeFactoryDeps,
    RuntimeFactoryDeps,
};
use crate::application::task_diff_base::ensure_task_has_non_empty_captured_diff;
use crate::application::task_notification_producer::TaskPipelineNotificationProducer;
use crate::application::task_scheduler_service::TaskSchedulerService;
use crate::application::task_transition_service::TaskTransitionService;
use crate::application::InteractiveProcessRegistry;
use crate::application::execution_state::{ExecutionState, AGENT_ACTIVE_STATUSES};
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{
    app_state::ExecutionHaltMode, AgentRunId, AgentRunStatus, ChatContextType, ChatConversation,
    ChatConversationId, ChatMessageId, IdeationSessionId, InternalStatus, MergeFailureSource,
    MergeRecoveryEvent, MergeRecoveryEventKind, MergeRecoveryMetadata, MergeRecoveryReasonCode,
    MergeRecoverySource, MergeRecoveryState, PersonaDirective, ReviewNote, ReviewOutcome,
    ReviewerType, SessionPurpose, Task, TaskId, TaskStepStatus, ValidationCacheMetadata,
    VerificationGap, VerificationStatus,
};
use crate::domain::repositories::{
    ActivityEventRepository, AgentLaneSettingsRepository, AgentProviderSettingsRepository,
    AgentRunRepository, ArtifactRepository, ChatAttachmentRepository, ChatConversationRepository,
    ChatMessageRepository, ChatTimelineRepository, DelegatedSessionRepository,
    ExecutionSettingsRepository, ExternalEventsRepository, IdeationEffortSettingsRepository,
    IdeationModelSettingsRepository, IdeationSessionRepository, MemoryEventRepository,
    PlanBranchRepository, ProjectRepository, ReviewRepository, TaskDependencyRepository,
    TaskProposalRepository, TaskRepository, TaskStepRepository, ValidationRunRepository,
};
use crate::domain::services::{MessageQueue, QueueKey, QueuedMessage, RunningAgentRegistry};
use crate::domain::state_machine::services::{TaskScheduler, WebhookPublisher};
use crate::error::AppError;
use crate::infrastructure::agents::claude::{stream_timeouts, ContentBlockItem, ToolCall};

use super::chat_service_context;
use super::chat_service_errors::{
    classify_agent_error, is_nonfatal_mcp_tool_cancellation, StreamError,
};
use super::chat_service_helpers::get_assistant_role;
use super::chat_service_types::{AgentErrorPayload, AgentRunCompletedPayload};
use super::EventContextPayload;
use crate::application::reconciliation::verification_handoff;
use crate::application::reconciliation::verification_reconciliation::ReconcileVerificationChildCompletion;
use crate::utils::path_safety::validate_absolute_non_root_path;
use crate::utils::secret_redactor::redact;

fn should_requeue_after_provider_pause(context_type: ChatContextType) -> bool {
    matches!(
        context_type,
        ChatContextType::Ideation
            | ChatContextType::Task
            | ChatContextType::Project
            | ChatContextType::Standalone
    )
}

fn provider_pause_targets_execution(context_type: ChatContextType) -> bool {
    matches!(
        context_type,
        ChatContextType::TaskExecution | ChatContextType::Review | ChatContextType::Merge
    )
}

const VERIFICATION_AUTO_CONTINUE_METADATA: &str = r#"{"resume_in_place":true}"#;
const AGENT_STOPPED_BY_USER_MESSAGE: &str = "Agent stopped by user";
const AGENT_STOPPED_BY_SYSTEM_RECOVERY_MESSAGE: &str = "Agent stream cancelled by system recovery";

fn task_metadata_indicates_recovery_cancellation(metadata: Option<&str>) -> bool {
    let Some(metadata) = metadata
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .and_then(|value| value.as_object().cloned())
    else {
        return false;
    };

    if metadata
        .get("trigger_origin")
        .and_then(|value| value.as_str())
        == Some("recovery")
    {
        return true;
    }

    let Some(execution_recovery) = metadata.get("execution_recovery") else {
        return false;
    };
    execution_recovery
        .get("last_state")
        .and_then(|value| value.as_str())
        == Some("retrying")
        && !execution_recovery
            .get("stop_retrying")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
}

async fn cancelled_stream_failure_message(
    context_type: ChatContextType,
    context_id: &str,
    task_repo: &Arc<dyn TaskRepository>,
) -> &'static str {
    if matches!(
        context_type,
        ChatContextType::TaskExecution | ChatContextType::Review | ChatContextType::Merge
    ) {
        let task_id = TaskId::from_string(context_id.to_string());
        match task_repo.get_by_id(&task_id).await {
            Ok(Some(task))
                if task_metadata_indicates_recovery_cancellation(task.metadata.as_deref()) =>
            {
                return AGENT_STOPPED_BY_SYSTEM_RECOVERY_MESSAGE;
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(
                    context_type = %context_type,
                    context_id,
                    error = %error,
                    "Failed to load task while classifying stream cancellation"
                );
            }
        }
    }

    AGENT_STOPPED_BY_USER_MESSAGE
}

async fn mark_cancelled_stream_as_cancelled(
    agent_run_repo: &Arc<dyn AgentRunRepository>,
    agent_run_id: &str,
    context_type: ChatContextType,
    context_id: &str,
    task_repo: &Arc<dyn TaskRepository>,
) {
    let run_id = AgentRunId::from_string(agent_run_id);
    match agent_run_repo.get_by_id(&run_id).await {
        Ok(Some(run)) if run.status != AgentRunStatus::Running => {
            tracing::info!(
                agent_run_id,
                status = %run.status,
                "Stream cancellation found an already-terminal agent run; preserving existing status"
            );
        }
        Ok(_) => {
            let message =
                cancelled_stream_failure_message(context_type, context_id, task_repo).await;
            if let Err(error) = agent_run_repo.fail(&run_id, message).await {
                tracing::warn!(
                    agent_run_id,
                    error = %error,
                    "Failed to mark cancelled stream as cancelled"
                );
            }
        }
        Err(error) => {
            tracing::warn!(
                agent_run_id,
                error = %error,
                "Failed to load agent run before cancelled stream labeling; preserving existing run state"
            );
        }
    }
}

async fn provider_env_for_harness(
    agent_provider_settings_repo: &Option<Arc<dyn AgentProviderSettingsRepository>>,
    harness: AgentHarnessKind,
) -> Result<HashMap<String, String>, String> {
    crate::application::provider_env_file::load_provider_custom_env_file_for_harness(
        agent_provider_settings_repo.as_ref(),
        harness,
    )
    .await
}

#[derive(Debug, PartialEq, Eq)]
enum RecoveryRetryProviderDecision {
    ApplyEnv(HashMap<String, String>),
    AllowWithoutProviderSettings,
}

#[derive(Debug, PartialEq, Eq)]
enum RecoveryRetryProviderBlock {
    Disabled(String),
    Env(String),
    MissingProviderSettings,
}

async fn recovery_retry_provider_decision(
    agent_provider_settings_repo: &Option<Arc<dyn AgentProviderSettingsRepository>>,
    recovery_harness: AgentHarnessKind,
    context_type: ChatContextType,
) -> Result<RecoveryRetryProviderDecision, RecoveryRetryProviderBlock> {
    let Some(provider_repo) = agent_provider_settings_repo.as_ref() else {
        return if super::uses_execution_slot(context_type) {
            Err(RecoveryRetryProviderBlock::MissingProviderSettings)
        } else {
            Ok(RecoveryRetryProviderDecision::AllowWithoutProviderSettings)
        };
    };

    crate::application::ensure_provider_spawn_enabled(
        provider_repo,
        recovery_harness,
        "recovery_retry",
    )
    .await
    .map_err(RecoveryRetryProviderBlock::Disabled)?;

    let provider_env = provider_env_for_harness(agent_provider_settings_repo, recovery_harness)
        .await
        .map_err(RecoveryRetryProviderBlock::Env)?;

    Ok(RecoveryRetryProviderDecision::ApplyEnv(provider_env))
}

async fn recovery_retry_spawnable_with_provider_gate(
    agent_provider_settings_repo: &Option<Arc<dyn AgentProviderSettingsRepository>>,
    recovery_harness: AgentHarnessKind,
    context_type: ChatContextType,
    project_id: Option<&str>,
    working_directory: &Path,
    runtime_factory_deps: Option<&ChatRuntimeFactoryDeps>,
    mut provider_spawnable: chat_service_context::ProviderSpawnableCommand,
) -> Result<Option<crate::infrastructure::agents::claude::SpawnableCommand>, String> {
    let provider_env = match recovery_retry_provider_decision(
        agent_provider_settings_repo,
        recovery_harness,
        context_type,
    )
    .await
    {
        Ok(RecoveryRetryProviderDecision::ApplyEnv(provider_env)) => Some(provider_env),
        Ok(RecoveryRetryProviderDecision::AllowWithoutProviderSettings) => None,
        Err(_) => return Ok(None),
    };
    let Some(policy_service) =
        runtime_factory_deps.and_then(|deps| deps.mcp_policy_service.as_ref())
    else {
        return Ok(None);
    };
    let policy = match policy_service
        .resolve_launch_policy(recovery_harness, project_id, Some(working_directory))
        .await
    {
        Ok(policy) => policy,
        Err(error) => {
            let error = error.to_string();
            if error.contains(crate::domain::agents::MCP_SETUP_PREFLIGHT_MARKER) {
                return Err(error);
            }
            tracing::error!(
                harness = %recovery_harness,
                "Failed to resolve MCP policy for recovery retry"
            );
            return Ok(None);
        }
    };
    provider_spawnable.apply_mcp_policy(recovery_harness, &policy);
    if let Some(provider_env) = provider_env.as_ref() {
        provider_spawnable.apply_provider_env(provider_env);
    }
    Ok(Some(provider_spawnable.spawnable))
}

#[derive(Clone, Copy)]
struct RecoveryRetryProviderGate<'a> {
    agent_provider_settings_repo: &'a Option<Arc<dyn AgentProviderSettingsRepository>>,
    recovery_harness: AgentHarnessKind,
    context_type: ChatContextType,
    project_id: Option<&'a str>,
    working_directory: &'a Path,
    runtime_factory_deps: Option<&'a ChatRuntimeFactoryDeps>,
}

impl<'a> RecoveryRetryProviderGate<'a> {
    fn new(
        agent_provider_settings_repo: &'a Option<Arc<dyn AgentProviderSettingsRepository>>,
        recovery_harness: AgentHarnessKind,
        context_type: ChatContextType,
        project_id: Option<&'a str>,
        working_directory: &'a Path,
        runtime_factory_deps: Option<&'a ChatRuntimeFactoryDeps>,
    ) -> Self {
        Self {
            agent_provider_settings_repo,
            recovery_harness,
            context_type,
            project_id,
            working_directory,
            runtime_factory_deps,
        }
    }
}

async fn resolve_recovery_retry_spawnable(
    retry_provider_spawnable: Result<chat_service_context::ProviderSpawnableCommand, String>,
    provider_gate: RecoveryRetryProviderGate<'_>,
) -> Result<Option<crate::infrastructure::agents::claude::SpawnableCommand>, String> {
    match retry_provider_spawnable {
        Ok(provider_spawnable) => {
            recovery_retry_spawnable_with_provider_gate(
                provider_gate.agent_provider_settings_repo,
                provider_gate.recovery_harness,
                provider_gate.context_type,
                provider_gate.project_id,
                provider_gate.working_directory,
                provider_gate.runtime_factory_deps,
                provider_spawnable,
            )
            .await
        }
        Err(error) => {
            tracing::error!(
                error = %error,
                harness = %provider_gate.recovery_harness,
                "Failed to build recovery retry spawnable"
            );
            Ok(None)
        }
    }
}

#[derive(Clone, Default)]
struct RecoveryRetryAppRepos {
    ideation_effort_settings_repo: Option<Arc<dyn IdeationEffortSettingsRepository>>,
    ideation_model_settings_repo: Option<Arc<dyn IdeationModelSettingsRepository>>,
    delegated_session_repo: Option<Arc<dyn DelegatedSessionRepository>>,
}

impl RecoveryRetryAppRepos {
    fn from_runtime_factory_deps(runtime_factory_deps: Option<&ChatRuntimeFactoryDeps>) -> Self {
        let Some(deps) = runtime_factory_deps else {
            return Self::default();
        };
        Self {
            ideation_effort_settings_repo: deps
                .ideation_effort_settings_repo
                .as_ref()
                .map(Arc::clone),
            ideation_model_settings_repo: deps
                .ideation_model_settings_repo
                .as_ref()
                .map(Arc::clone),
            delegated_session_repo: deps.delegated_session_repo.as_ref().map(Arc::clone),
        }
    }
}

async fn recovery_retry_folder_refs_context(
    runtime_factory_deps: Option<&ChatRuntimeFactoryDeps>,
    conversation: &ChatConversation,
    project_id: Option<&str>,
    working_directory: &Path,
) -> Result<(Option<String>, Vec<PathBuf>), String> {
    let Some(deps) = runtime_factory_deps else {
        tracing::warn!(
            conversation_id = conversation.id.as_str(),
            reason = chat_service_context::FOLDER_REFS_SKIPPED_CONTEXT_UNAVAILABLE,
            "folder_refs_skipped"
        );
        return Ok((None, Vec::new()));
    };
    let resolved = chat_service_context::resolve_conversation_spawn_context(
        conversation,
        conversation.agent_mode,
        project_id,
        Arc::clone(&deps.project_repo),
        working_directory,
        deps.folder_reference_app_data_dir.as_deref(),
        deps.folder_reference_app_data_dir.as_deref(),
        deps.conversation_folder_reference_repo
            .as_ref()
            .map(Arc::clone),
    )
    .await
    .map_err(|error| error.to_string())?;
    Ok((resolved.folder_refs_block, resolved.folder_roots))
}

async fn resolve_recovery_retry_persona(
    runtime_factory_deps: Option<&ChatRuntimeFactoryDeps>,
    feature_enabled: bool,
    conversation: &ChatConversation,
    context_type: ChatContextType,
    agent_name_override_set: bool,
) -> Result<Option<crate::application::persona_prompt::ResolvedPersona>, String> {
    if !feature_enabled {
        return Ok(None);
    }
    let Some(deps) = runtime_factory_deps else {
        return Ok(None);
    };
    let Some(workspace_repo) = deps.agent_conversation_workspace_repo.as_ref() else {
        return Ok(None);
    };
    let Some(persona_repo) = deps.persona_repo.as_ref() else {
        return Ok(None);
    };
    let workspace_mode = workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .map_err(|error| format!("Persona workspace lookup failed: {error}"))?
        .map(|workspace| workspace.mode);
    resolve_persona_for_send(
        conversation,
        &PersonaDirective::Inherit,
        super::persona_resolve_flags_for_conversation(
            feature_enabled,
            false,
            agent_name_override_set || conversation.bound_agent_name.is_some(),
            context_type,
            conversation,
            workspace_mode,
        ),
        Arc::clone(persona_repo),
    )
    .await
    .map_err(|error| error.to_string())
}

fn queue_verification_auto_continue(
    message_queue: &Arc<MessageQueue>,
    child_id: &IdeationSessionId,
    continuation_message: String,
) -> QueuedMessage {
    let mut queued = QueuedMessage::new(continuation_message);
    queued.metadata_override = Some(VERIFICATION_AUTO_CONTINUE_METADATA.to_string());
    message_queue.queue_front_existing(
        ChatContextType::Ideation,
        child_id.as_str(),
        queued.clone(),
    );
    queued
}

pub(super) async fn handle_verification_child_completion(
    child_id: &IdeationSessionId,
    parent_id: &IdeationSessionId,
    ideation_session_repo: &Arc<dyn IdeationSessionRepository>,
    conversation_repo: &Arc<dyn ChatConversationRepository>,
    chat_message_repo: &Arc<dyn ChatMessageRepository>,
    message_queue: &Arc<MessageQueue>,
    queued_message_repo: Option<&Arc<dyn crate::domain::repositories::QueuedMessageRepository>>,
    _events: &dyn EventSink,
    verification_child_registry: &Option<
        Arc<super::verification_child_process_registry::VerificationChildProcessRegistry>,
    >,
) {
    let reconcile_result = crate::application::reconciliation::verification_reconciliation::reconcile_verification_on_child_complete::<tauri::Wry>(
        parent_id,
        child_id,
        ideation_session_repo,
        None,
    )
    .await;

    match reconcile_result {
        Some(ReconcileVerificationChildCompletion::Terminal(result)) => {
            verification_handoff::maybe_inject_verification_result_message(
                parent_id,
                &result,
                conversation_repo,
                chat_message_repo,
                message_queue,
                queued_message_repo,
            )
            .await;

            if let Some(registry) = verification_child_registry {
                tracing::info!(
                    context_id = child_id.as_str(),
                    "Sending SIGTERM to verification child process after terminal reconciliation"
                );
                registry.remove_and_kill(child_id.as_str());
            }
        }
        Some(ReconcileVerificationChildCompletion::AutoContinue(request)) => {
            let queued = queue_verification_auto_continue(
                message_queue,
                child_id,
                request.continuation_message,
            );
            if let Some(repo) = queued_message_repo {
                let key = QueueKey::new(ChatContextType::Ideation, child_id.as_str());
                if let Err(error) = repo.enqueue_front(&key, &queued).await {
                    tracing::warn!(
                        context_id = child_id.as_str(),
                        queued_message_id = queued.id.as_str(),
                        error = %error,
                        "Failed to persist verification auto-continue queued message"
                    );
                }
            }
            tracing::info!(
                context_id = child_id.as_str(),
                current_round = request.snapshot.current_round,
                max_rounds = request.snapshot.max_rounds,
                gap_count = request.snapshot.current_gaps.len(),
                "Queued hidden resume-in-place continuation for actionable non-terminal verification state"
            );

            if let Some(registry) = verification_child_registry {
                tracing::info!(
                    context_id = child_id.as_str(),
                    "Sending SIGTERM to verification child process before in-place verification continuation"
                );
                registry.remove_and_kill(child_id.as_str());
            }
        }
        None => {}
    }
}

/// Returns true if all steps for `task_id` are Completed or Skipped (and at least one
/// step exists). Safe-fallback: returns false if repo is None or returns an error.
pub(crate) async fn all_steps_completed(
    task_step_repo: &Option<Arc<dyn TaskStepRepository>>,
    task_id: &TaskId,
) -> bool {
    let Some(ref repo) = task_step_repo else {
        return false;
    };
    match repo.get_by_task(task_id).await {
        Ok(steps) => {
            !steps.is_empty()
                && steps.iter().all(|s| {
                    s.status == TaskStepStatus::Completed || s.status == TaskStepStatus::Skipped
                })
        }
        Err(e) => {
            tracing::warn!(
                task_id = task_id.as_str(),
                error = %e,
                "Failed to query steps for all-complete check"
            );
            false
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StepCompletionState {
    NoSteps,
    AllComplete,
    Incomplete,
    Unknown,
}

pub(crate) async fn fetch_step_completion_state(
    task_step_repo: &Option<Arc<dyn TaskStepRepository>>,
    task_id: &TaskId,
) -> StepCompletionState {
    let Some(ref repo) = task_step_repo else {
        return StepCompletionState::Unknown;
    };
    match repo.get_by_task(task_id).await {
        Ok(steps) if steps.is_empty() => StepCompletionState::NoSteps,
        Ok(steps)
            if steps.iter().all(|s| {
                s.status == TaskStepStatus::Completed || s.status == TaskStepStatus::Skipped
            }) =>
        {
            StepCompletionState::AllComplete
        }
        Ok(_) => StepCompletionState::Incomplete,
        Err(e) => {
            tracing::warn!(
                task_id = task_id.as_str(),
                error = %e,
                "Failed to query steps for completion-state check"
            );
            StepCompletionState::Unknown
        }
    }
}

/// Pure predicate: does a HEAD-matched validation cache prove the run's work is
/// validated-complete?
///
/// Requires the cache's commit SHA to match current HEAD **and** tests to have
/// actually run and passed. A cache with `tests_ran=false` (e.g. a self-blocked
/// no-op that claimed success without running tests) deliberately does NOT count —
/// this prevents rescuing a task that never did real work. No git calls, no side
/// effects — fully unit-testable.
pub(crate) fn validation_cache_proves_completion(
    cache: &ValidationCacheMetadata,
    current_head_sha: &str,
) -> bool {
    cache.commit_sha == current_head_sha && cache.tests_ran && cache.tests_passed
}

pub(crate) fn validation_cache_fresh_for_episode(
    cache: &ValidationCacheMetadata,
    current_head_sha: &str,
    episode_entered_at: chrono::DateTime<chrono::Utc>,
) -> bool {
    validation_cache_proves_completion(cache, current_head_sha)
        && cache.captured_at >= episode_entered_at
}

/// Async wrapper around [`validation_cache_proves_completion`]: parses the task's
/// `validation_cache` metadata, resolves the worktree HEAD SHA, and reports whether
/// the cache proves completion for the current commit.
///
/// Used to override a would-be `Failed` transition when `execution_complete` already
/// captured a green, HEAD-matched validation cache — the case where a lingering
/// terminal `failed` step (which cannot be cleared) would otherwise trap a fully
/// validated task in `Failed` and drive an endless auto-retry loop.
///
/// Safe-fallback: returns false if there is no cache, no worktree path, or the HEAD
/// SHA cannot be resolved.
async fn validated_completion_override(
    task: &Task,
    episode_entered_at: chrono::DateTime<chrono::Utc>,
    validation_run_repo: &Option<Arc<dyn ValidationRunRepository>>,
) -> bool {
    let Some(worktree_path) = task.worktree_path.as_deref() else {
        return false;
    };
    let safe_worktree_path =
        match validate_absolute_non_root_path(Path::new(worktree_path), "task worktree") {
            Ok(path) => path,
            Err(e) => {
                tracing::warn!(
                    task_id = task.id.as_str(),
                    error = %e,
                    "Rejecting unsafe worktree path for completion override"
                );
                return false;
            }
        };
    let current_head_sha = match GitService::get_head_sha(&safe_worktree_path).await {
        Ok(sha) => sha,
        Err(e) => {
            tracing::warn!(
                task_id = task.id.as_str(),
                error = %e,
                "Failed to resolve HEAD SHA for completion override"
            );
            return false;
        }
    };

    let Some(validation_run_repo) = validation_run_repo.as_ref() else {
        tracing::warn!(
            task_id = task.id.as_str(),
            "First-class validation repository unavailable for completion override"
        );
        return false;
    };
    match validation_run_repo
        .latest_non_baseline_run_with_results_for_task(&task.id)
        .await
    {
        Ok(Some(evidence)) => {
            return crate::application::validation_service::validation_run_proves_current_completion(
                &evidence,
                &current_head_sha,
                episode_entered_at,
            );
        }
        Err(error) => {
            tracing::warn!(
                task_id = task.id.as_str(),
                error = %error,
                "First-class validation query failed for completion override"
            );
            return false;
        }
        Ok(None) => {}
    }

    let cache = match ValidationCacheMetadata::from_task_metadata(task.metadata.as_deref()) {
        Ok(Some(cache)) => cache,
        Ok(None) => return false,
        Err(e) => {
            tracing::warn!(
                task_id = task.id.as_str(),
                error = %e,
                "Failed to parse validation_cache for completion override"
            );
            return false;
        }
    };
    validation_cache_fresh_for_episode(&cache, &current_head_sha, episode_entered_at)
}

/// Parse an ISO 8601 retry_after string and set the execution-lane provider gate.
/// This gate is still global within execution scheduling, so callers must only
/// apply it for execution-owned contexts.
fn apply_global_rate_limit_backpressure(
    execution_state: &Option<Arc<ExecutionState>>,
    retry_after: &Option<String>,
    context: &str,
    context_id: &str,
) {
    if let Some(retry_after_str) = retry_after {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(retry_after_str) {
            let epoch_secs = dt.timestamp() as u64;
            if let Some(ref exec) = execution_state {
                exec.set_provider_blocked_until(epoch_secs);
                tracing::info!(
                    context = context,
                    context_id = context_id,
                    retry_after = %retry_after_str,
                    epoch_secs = epoch_secs,
                    "Global rate limit backpressure set — all spawns blocked until retry_after"
                );
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExecutionCompletionAction {
    PendingReview,
    Failed,
}

#[derive(Clone)]
struct RuntimeSupportRepos {
    events: Arc<dyn EventSink>,
    chat_runtime_deps: Option<ChatRuntimeFactoryDeps>,
    execution_settings_repo: Option<Arc<dyn ExecutionSettingsRepository>>,
    agent_lane_settings_repo: Option<Arc<dyn AgentLaneSettingsRepository>>,
    agent_provider_settings_repo: Option<Arc<dyn AgentProviderSettingsRepository>>,
    plan_branch_repo: Option<Arc<dyn PlanBranchRepository>>,
    interactive_process_registry: Option<Arc<InteractiveProcessRegistry>>,
    task_step_repo: Option<Arc<dyn TaskStepRepository>>,
    validation_run_repo: Option<Arc<dyn ValidationRunRepository>>,
    external_events_repo: Option<Arc<dyn ExternalEventsRepository>>,
    webhook_publisher: Option<Arc<dyn WebhookPublisher>>,
}

impl RuntimeSupportRepos {
    fn new(
        execution_settings_repo: &Option<Arc<dyn ExecutionSettingsRepository>>,
        agent_lane_settings_repo: &Option<Arc<dyn AgentLaneSettingsRepository>>,
        agent_provider_settings_repo: &Option<Arc<dyn AgentProviderSettingsRepository>>,
        plan_branch_repo: &Option<Arc<dyn PlanBranchRepository>>,
        interactive_process_registry: &Option<Arc<InteractiveProcessRegistry>>,
        task_step_repo: &Option<Arc<dyn TaskStepRepository>>,
        validation_run_repo: &Option<Arc<dyn ValidationRunRepository>>,
    ) -> Self {
        Self {
            events: Arc::new(ralphx_events::NullEventSink),
            chat_runtime_deps: None,
            execution_settings_repo: execution_settings_repo.as_ref().map(Arc::clone),
            agent_lane_settings_repo: agent_lane_settings_repo.as_ref().map(Arc::clone),
            agent_provider_settings_repo: agent_provider_settings_repo.as_ref().map(Arc::clone),
            plan_branch_repo: plan_branch_repo.as_ref().map(Arc::clone),
            interactive_process_registry: interactive_process_registry.as_ref().map(Arc::clone),
            task_step_repo: task_step_repo.as_ref().map(Arc::clone),
            validation_run_repo: validation_run_repo.as_ref().map(Arc::clone),
            external_events_repo: None,
            webhook_publisher: None,
        }
    }

    fn with_completion_event_delivery(
        mut self,
        external_events_repo: &Option<Arc<dyn ExternalEventsRepository>>,
        webhook_publisher: &Option<Arc<dyn WebhookPublisher>>,
    ) -> Self {
        self.external_events_repo = external_events_repo.as_ref().map(Arc::clone);
        self.webhook_publisher = webhook_publisher.as_ref().map(Arc::clone);
        self
    }

    fn with_runtime_factory_deps(mut self, deps: Option<&ChatRuntimeFactoryDeps>) -> Self {
        if let Some(deps) = deps {
            self.events = Arc::clone(&deps.events);
            self.chat_runtime_deps = Some(deps.clone());
        }
        self
    }

    fn with_events(mut self, events: Arc<dyn EventSink>) -> Self {
        self.events = events;
        self
    }
}

fn execution_completion_action(
    _has_output: bool,
    step_state: StepCompletionState,
    completion_tool_called: bool,
    validation_complete: bool,
) -> ExecutionCompletionAction {
    match step_state {
        StepCompletionState::AllComplete => ExecutionCompletionAction::PendingReview,
        StepCompletionState::NoSteps if completion_tool_called || validation_complete => {
            ExecutionCompletionAction::PendingReview
        }
        StepCompletionState::Incomplete | StepCompletionState::Unknown if validation_complete => {
            ExecutionCompletionAction::PendingReview
        }
        _ => ExecutionCompletionAction::Failed,
    }
}

fn build_transition_service(
    task_repo: Arc<dyn TaskRepository>,
    task_dependency_repo: Arc<dyn TaskDependencyRepository>,
    project_repo: Arc<dyn ProjectRepository>,
    artifact_repo: Arc<dyn ArtifactRepository>,
    chat_message_repo: Arc<dyn ChatMessageRepository>,
    chat_attachment_repo: Arc<dyn ChatAttachmentRepository>,
    conversation_repo: Arc<dyn ChatConversationRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    ideation_session_repo: Arc<dyn IdeationSessionRepository>,
    activity_event_repo: Arc<dyn ActivityEventRepository>,
    message_queue: Arc<MessageQueue>,
    running_agent_registry: Arc<dyn RunningAgentRegistry>,
    execution_state: Arc<ExecutionState>,
    memory_event_repo: Arc<dyn MemoryEventRepository>,
    runtime_support: RuntimeSupportRepos,
) -> TaskTransitionService {
    let deps = build_runtime_factory_deps(
        task_repo,
        task_dependency_repo,
        project_repo,
        artifact_repo,
        chat_message_repo,
        chat_attachment_repo,
        conversation_repo,
        agent_run_repo,
        ideation_session_repo,
        activity_event_repo,
        message_queue,
        running_agent_registry,
        memory_event_repo,
        runtime_support,
    );
    build_transition_service_from_deps(None, execution_state, &deps)
}

#[allow(clippy::too_many_arguments)]
fn build_task_scheduler_service(
    project_repo: Arc<dyn ProjectRepository>,
    task_repo: Arc<dyn TaskRepository>,
    task_dependency_repo: Arc<dyn TaskDependencyRepository>,
    artifact_repo: Arc<dyn ArtifactRepository>,
    chat_message_repo: Arc<dyn ChatMessageRepository>,
    chat_attachment_repo: Arc<dyn ChatAttachmentRepository>,
    conversation_repo: Arc<dyn ChatConversationRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    ideation_session_repo: Arc<dyn IdeationSessionRepository>,
    activity_event_repo: Arc<dyn ActivityEventRepository>,
    message_queue: Arc<MessageQueue>,
    running_agent_registry: Arc<dyn RunningAgentRegistry>,
    execution_state: Arc<ExecutionState>,
    memory_event_repo: Arc<dyn MemoryEventRepository>,
    runtime_support: RuntimeSupportRepos,
) -> TaskSchedulerService {
    let deps = build_runtime_factory_deps(
        task_repo,
        task_dependency_repo,
        project_repo,
        artifact_repo,
        chat_message_repo,
        chat_attachment_repo,
        conversation_repo,
        agent_run_repo,
        ideation_session_repo,
        activity_event_repo,
        message_queue,
        running_agent_registry,
        memory_event_repo,
        runtime_support,
    );
    build_task_scheduler_from_deps(None, execution_state, &deps)
}

#[allow(clippy::too_many_arguments)]
fn build_runtime_factory_deps(
    task_repo: Arc<dyn TaskRepository>,
    task_dependency_repo: Arc<dyn TaskDependencyRepository>,
    project_repo: Arc<dyn ProjectRepository>,
    artifact_repo: Arc<dyn ArtifactRepository>,
    chat_message_repo: Arc<dyn ChatMessageRepository>,
    chat_attachment_repo: Arc<dyn ChatAttachmentRepository>,
    conversation_repo: Arc<dyn ChatConversationRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    ideation_session_repo: Arc<dyn IdeationSessionRepository>,
    activity_event_repo: Arc<dyn ActivityEventRepository>,
    message_queue: Arc<MessageQueue>,
    running_agent_registry: Arc<dyn RunningAgentRegistry>,
    memory_event_repo: Arc<dyn MemoryEventRepository>,
    runtime_support: RuntimeSupportRepos,
) -> RuntimeFactoryDeps {
    if let Some(deps) = runtime_support.chat_runtime_deps {
        return RuntimeFactoryDeps::from_chat_runtime_deps(&deps);
    }

    RuntimeFactoryDeps::from_core(
        task_repo,
        task_dependency_repo,
        project_repo,
        artifact_repo,
        chat_message_repo,
        chat_attachment_repo,
        conversation_repo,
        agent_run_repo,
        ideation_session_repo,
        activity_event_repo,
        message_queue,
        running_agent_registry,
        memory_event_repo,
    )
    .with_events(Arc::clone(&runtime_support.events))
    .with_chat_runtime_deps_option(runtime_support.chat_runtime_deps.clone())
    .with_runtime_support(
        runtime_support.execution_settings_repo,
        runtime_support.agent_lane_settings_repo,
        runtime_support.agent_provider_settings_repo,
        runtime_support.plan_branch_repo,
        runtime_support.interactive_process_registry,
    )
    .with_completion_authority_repositories(
        runtime_support.task_step_repo,
        runtime_support.validation_run_repo,
    )
    .with_completion_event_delivery(
        runtime_support.external_events_repo,
        runtime_support.webhook_publisher,
    )
    .with_agent_conversation_workspace_repo(None)
}

#[allow(clippy::too_many_arguments)]
fn build_recovery_retry_background_context(
    retry_child: tokio::process::Child,
    recovery_harness: AgentHarnessKind,
    context_type: ChatContextType,
    context_id: &str,
    conversation_id: ChatConversationId,
    agent_run_id: &str,
    new_session_id: String,
    working_directory: &Path,
    cli_path: &Path,
    plugin_dir: &Path,
    chat_message_repo: &Arc<dyn ChatMessageRepository>,
    chat_attachment_repo: &Arc<dyn ChatAttachmentRepository>,
    artifact_repo: &Arc<dyn ArtifactRepository>,
    conversation_repo: &Arc<dyn ChatConversationRepository>,
    agent_run_repo: &Arc<dyn AgentRunRepository>,
    task_repo: &Arc<dyn TaskRepository>,
    task_dependency_repo: &Arc<dyn TaskDependencyRepository>,
    project_repo: &Arc<dyn ProjectRepository>,
    ideation_session_repo: &Arc<dyn IdeationSessionRepository>,
    delegated_session_repo: &Arc<dyn crate::domain::repositories::DelegatedSessionRepository>,
    execution_settings_repo: &Option<Arc<dyn ExecutionSettingsRepository>>,
    agent_lane_settings_repo: &Option<Arc<dyn AgentLaneSettingsRepository>>,
    agent_provider_settings_repo: &Option<Arc<dyn AgentProviderSettingsRepository>>,
    task_proposal_repo: &Option<Arc<dyn TaskProposalRepository>>,
    activity_event_repo: &Arc<dyn ActivityEventRepository>,
    memory_event_repo: &Arc<dyn MemoryEventRepository>,
    message_queue: &Arc<MessageQueue>,
    running_agent_registry: &Arc<dyn RunningAgentRegistry>,
    execution_state: &Option<Arc<ExecutionState>>,
    question_state: &Option<Arc<QuestionState>>,
    plan_branch_repo: &Option<Arc<dyn PlanBranchRepository>>,
    events: Arc<dyn EventSink>,
    runtime_factory_deps: Option<ChatRuntimeFactoryDeps>,
    run_chain_id: Option<String>,
    persona_feature_enabled: bool,
    agent_name_override_set: bool,
    user_message_content: Option<&str>,
    retry_conv: ChatConversation,
    agent_name: Option<&str>,
    review_repo: &Option<Arc<dyn ReviewRepository>>,
    task_step_repo: &Option<Arc<dyn TaskStepRepository>>,
    validation_run_repo: &Option<Arc<dyn ValidationRunRepository>>,
    external_events_repo: &Option<Arc<dyn ExternalEventsRepository>>,
    webhook_publisher: &Option<Arc<dyn WebhookPublisher>>,
    interactive_process_registry: &Option<Arc<InteractiveProcessRegistry>>,
    verification_child_registry: &Option<
        Arc<super::verification_child_process_registry::VerificationChildProcessRegistry>,
    >,
) -> super::chat_service_send_background::BackgroundRunContext {
    use super::chat_service_send_background::{BackgroundRunContext, BackgroundRunRepos};

    BackgroundRunContext {
        child: retry_child,
        harness: recovery_harness,
        context_type,
        context_id: context_id.to_string(),
        runtime_context_id: context_id.to_string(),
        conversation_id,
        agent_run_id: agent_run_id.to_string(),
        stored_session_id: Some(new_session_id.clone()),
        working_directory: working_directory.to_path_buf(),
        cli_path: cli_path.to_path_buf(),
        plugin_dir: plugin_dir.to_path_buf(),
        repos: BackgroundRunRepos {
            chat_message_repo: Arc::clone(chat_message_repo),
            chat_timeline_repo: None,
            chat_attachment_repo: Arc::clone(chat_attachment_repo),
            artifact_repo: Arc::clone(artifact_repo),
            conversation_repo: Arc::clone(conversation_repo),
            agent_run_repo: Arc::clone(agent_run_repo),
            task_repo: Arc::clone(task_repo),
            task_dependency_repo: Arc::clone(task_dependency_repo),
            project_repo: Arc::clone(project_repo),
            ideation_session_repo: Arc::clone(ideation_session_repo),
            delegated_session_repo: Arc::clone(delegated_session_repo),
            execution_settings_repo: execution_settings_repo.clone(),
            agent_lane_settings_repo: agent_lane_settings_repo.clone(),
            agent_provider_settings_repo: agent_provider_settings_repo.clone(),
            task_proposal_repo: task_proposal_repo.clone(),
            activity_event_repo: Arc::clone(activity_event_repo),
            memory_event_repo: Arc::clone(memory_event_repo),
            notification_service: None,
            message_queue: Arc::clone(message_queue),
            queued_message_repo: runtime_factory_deps
                .as_ref()
                .and_then(|deps| deps.queued_message_repo.as_ref().map(Arc::clone)),
            running_agent_registry: Arc::clone(running_agent_registry),
            task_step_repo: task_step_repo.clone(),
            validation_run_repo: validation_run_repo.as_ref().map(Arc::clone),
            external_events_repo: external_events_repo.as_ref().map(Arc::clone),
            webhook_publisher: webhook_publisher.as_ref().map(Arc::clone),
            review_repo: review_repo.clone(),
        },
        execution_state: execution_state.clone(),
        question_state: question_state.clone(),
        plan_branch_repo: plan_branch_repo.clone(),
        events,
        plan_verification_completion: runtime_factory_deps
            .as_ref()
            .and_then(|deps| deps.plan_verification_completion.as_ref().map(Arc::clone)),
        runtime_factory_deps,
        run_chain_id,
        is_retry_attempt: true,
        persona_feature_enabled,
        agent_name_override_set,
        user_message_content: user_message_content.map(str::to_string),
        turn_metadata: None,
        conversation: Some(retry_conv),
        agent_name: agent_name.map(str::to_string),
        assistant_message_attribution: crate::domain::entities::ChatMessageAttribution {
            attribution_source: Some("native_runtime".to_string()),
            provider_harness: Some(recovery_harness),
            provider_session_id: Some(new_session_id.clone()),
            upstream_provider: None,
            provider_profile: None,
            logical_model: None,
            effective_model_id: None,
            logical_effort: None,
            effective_effort: None,
        },
        persist_conversation_provider_session_ref: true,
        cancellation_token: tokio_util::sync::CancellationToken::new(),
        streaming_state_cache: super::StreamingStateCache::new(),
        interactive_process_registry: interactive_process_registry.clone(),
        interactive_process_token: None,
        verification_child_registry: verification_child_registry.clone(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IncompleteReviewAction {
    SkipDuringShutdown,
    Escalate,
    IgnoreAlreadyTransitioned,
}

fn incomplete_review_action(
    current_status: InternalStatus,
    is_shutting_down: bool,
) -> IncompleteReviewAction {
    if current_status != InternalStatus::Reviewing {
        return IncompleteReviewAction::IgnoreAlreadyTransitioned;
    }

    if is_shutting_down {
        IncompleteReviewAction::SkipDuringShutdown
    } else {
        IncompleteReviewAction::Escalate
    }
}

fn provider_pause_metadata(
    task: &Task,
    category: &super::ProviderErrorCategory,
    message: &str,
    retry_after: &Option<String>,
    paused_at: &str,
) -> String {
    let resume_attempts = match super::PauseReason::from_task_metadata(task.metadata.as_deref()) {
        Some(super::PauseReason::ProviderError {
            resume_attempts, ..
        }) => resume_attempts,
        _ => super::ProviderErrorMetadata::from_task_metadata(task.metadata.as_deref())
            .map_or(0, |metadata| metadata.resume_attempts),
    };
    let provider_error = super::ProviderErrorMetadata {
        category: category.clone(),
        message: message.to_string(),
        retry_after: retry_after.clone(),
        previous_status: task.internal_status.to_string(),
        paused_at: paused_at.to_string(),
        auto_resumable: true,
        resume_attempts,
    };
    let pause_reason = super::PauseReason::ProviderError {
        category: category.clone(),
        message: message.to_string(),
        retry_after: retry_after.clone(),
        previous_status: task.internal_status.to_string(),
        paused_at: paused_at.to_string(),
        auto_resumable: true,
        resume_attempts,
    };
    let with_legacy = provider_error.write_to_task_metadata(task.metadata.as_deref());
    pause_reason.write_to_task_metadata(Some(&with_legacy))
}

pub(super) async fn apply_system_wide_provider_pause(
    runtime_factory_deps: Option<&ChatRuntimeFactoryDeps>,
    execution_state: Option<&Arc<ExecutionState>>,
    events: Arc<dyn EventSink>,
    category: &super::ProviderErrorCategory,
    message: &str,
    retry_after: &Option<String>,
    source_context_type: ChatContextType,
    source_context_id: &str,
) -> bool {
    let (Some(deps), Some(execution_state), Some(app_state_repo)) = (
        runtime_factory_deps,
        execution_state,
        runtime_factory_deps.and_then(|deps| deps.app_state_repo.as_ref()),
    ) else {
        return false;
    };

    let source_context = source_context_type.to_string();
    if !provider_pause_targets_execution(source_context_type) {
        tracing::info!(
            source_context = source_context,
            source_context_id = source_context_id,
            category = %category,
            retry_after = ?retry_after,
            "Provider error from non-execution context left execution tasks running"
        );
        return false;
    }

    match app_state_repo.get().await {
        Ok(settings) if settings.execution_halt_mode == ExecutionHaltMode::Paused => {
            tracing::info!(
                category = %category,
                "Provider-triggered global pause already persists; suppressing duplicate delivery"
            );
            return true;
        }
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(error = %error, "Failed to read persisted halt state before provider-triggered pause");
        }
    }

    execution_state.pause();
    apply_global_rate_limit_backpressure(
        &Some(Arc::clone(execution_state)),
        retry_after,
        &source_context,
        source_context_id,
    );

    let pause_committed = match app_state_repo
        .set_execution_halt_mode(ExecutionHaltMode::Paused)
        .await
    {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(error = %error, "Failed to persist provider-triggered global pause");
            false
        }
    };

    deps.running_agent_registry.stop_all().await;
    if let Some(registry) = deps.interactive_process_registry.as_ref() {
        registry.clear().await;
    }

    let runtime_support = RuntimeSupportRepos::new(
        &deps.execution_settings_repo,
        &deps.agent_lane_settings_repo,
        &deps.agent_provider_settings_repo,
        &deps.plan_branch_repo,
        &deps.interactive_process_registry,
        &deps.task_step_repo,
        &deps.validation_run_repo,
    )
    .with_runtime_factory_deps(Some(deps));
    let transition_service = build_transition_service(
        Arc::clone(&deps.task_repo),
        Arc::clone(&deps.task_dependency_repo),
        Arc::clone(&deps.project_repo),
        Arc::clone(&deps.artifact_repo),
        Arc::clone(&deps.chat_message_repo),
        Arc::clone(&deps.chat_attachment_repo),
        Arc::clone(&deps.conversation_repo),
        Arc::clone(&deps.agent_run_repo),
        Arc::clone(&deps.ideation_session_repo),
        Arc::clone(&deps.activity_event_repo),
        Arc::clone(&deps.message_queue),
        Arc::clone(&deps.running_agent_registry),
        Arc::clone(execution_state),
        Arc::clone(&deps.memory_event_repo),
        runtime_support,
    );

    let paused_at = chrono::Utc::now().to_rfc3339();
    // The pause timestamp is generated once per successful global-pause authority commit.
    // Persist it on the source task before using it as the global notification instance key.
    if pause_committed {
        let source_task_id = TaskId::from_string(source_context_id.to_string());
        match deps.task_repo.get_by_id(&source_task_id).await {
            Ok(Some(source_task)) => {
                let mut source_task_with_pause = source_task.clone();
                source_task_with_pause.metadata = Some(provider_pause_metadata(
                    &source_task,
                    category,
                    message,
                    retry_after,
                    &paused_at,
                ));
                source_task_with_pause.touch();
                match deps.task_repo.update(&source_task_with_pause).await {
                    Ok(()) => {
                        if let Some(notification_service) = deps.notification_service.as_ref() {
                            notification_service
                                .record(
                                    TaskPipelineNotificationProducer::provider_paused_notification(
                                        &paused_at,
                                        &category.to_string(),
                                    ),
                                )
                                .await;
                        }
                    }
                    Err(error) => tracing::warn!(
                        source_context_id,
                        error = %error,
                        "Failed to persist provider-pause notification instance"
                    ),
                }
            }
            Ok(None) => tracing::warn!(
                source_context_id,
                "Provider-triggered global pause has no source task for its notification"
            ),
            Err(error) => tracing::warn!(
                source_context_id,
                error = %error,
                "Failed to load provider-pause source task for notification"
            ),
        }
    }
    let projects = match deps.project_repo.get_all().await {
        Ok(projects) => projects,
        Err(error) => {
            tracing::error!(error = %error, "Failed to load projects for provider-triggered pause");
            return false;
        }
    };

    for project in projects {
        let tasks = match deps.task_repo.get_by_project(&project.id).await {
            Ok(tasks) => tasks,
            Err(error) => {
                tracing::warn!(
                    project_id = project.id.as_str(),
                    error = %error,
                    "Failed to load project tasks during provider-triggered pause"
                );
                continue;
            }
        };

        for task in tasks {
            if !AGENT_ACTIVE_STATUSES.contains(&task.internal_status) {
                continue;
            }

            let mut updated_task = task.clone();
            updated_task.metadata = Some(provider_pause_metadata(
                &task,
                category,
                message,
                retry_after,
                &paused_at,
            ));
            updated_task.touch();
            let _ = deps.task_repo.update(&updated_task).await;

            if let Err(error) = transition_service
                .transition_task(&task.id, InternalStatus::Paused)
                .await
            {
                tracing::warn!(
                    task_id = task.id.as_str(),
                    error = %error,
                    "Failed to transition task to Paused during provider-triggered global pause"
                );
            }
        }
    }

    events.emit(
        "execution:status_changed",
        serde_json::json!({
            "isPaused": execution_state.is_paused(),
            "haltMode": "paused",
            "runningCount": execution_state.running_count(),
            "maxConcurrent": execution_state.max_concurrent(),
            "reason": "provider_error",
            "providerCategory": category.to_string(),
            "providerMessage": message,
            "providerRetryAfter": retry_after,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        }),
    );

    true
}

/// Read existing message content, tool_calls, and content_blocks from the database.
///
/// Used before error finalization to preserve any content that was flushed
/// during streaming, so the error note is appended rather than overwriting.
async fn read_existing_message_content(
    chat_message_repo: &Arc<dyn ChatMessageRepository>,
    message_id: &str,
) -> (String, Option<String>, Option<String>) {
    match chat_message_repo
        .get_by_id(&ChatMessageId::from_string(message_id.to_string()))
        .await
    {
        Ok(Some(msg)) => (msg.content, msg.tool_calls, msg.content_blocks),
        _ => (String::new(), None, None),
    }
}

fn terminal_tool_result(reason: &str) -> serde_json::Value {
    serde_json::json!({
        "status": "aborted",
        "reason": reason,
    })
}

fn seal_unresolved_tool_calls_json(
    tool_calls_json: Option<String>,
    reason: &str,
) -> Option<String> {
    let raw = tool_calls_json?;
    let mut tool_calls: Vec<ToolCall> = match serde_json::from_str(&raw) {
        Ok(parsed) => parsed,
        Err(_) => return Some(raw),
    };

    let sealed_result = terminal_tool_result(reason);
    let mut changed = false;
    for tool_call in &mut tool_calls {
        if tool_call.result.is_none() {
            tool_call.result = Some(sealed_result.clone());
            changed = true;
        }
    }

    if !changed {
        return Some(raw);
    }

    serde_json::to_string(&tool_calls).ok().or(Some(raw))
}

fn seal_unresolved_content_blocks_json(
    content_blocks_json: Option<String>,
    reason: &str,
) -> Option<String> {
    let raw = content_blocks_json?;
    let mut content_blocks: Vec<ContentBlockItem> = match serde_json::from_str(&raw) {
        Ok(parsed) => parsed,
        Err(_) => return Some(raw),
    };

    let sealed_result = terminal_tool_result(reason);
    let mut changed = false;
    for block in &mut content_blocks {
        if let ContentBlockItem::ToolUse { result, .. } = block {
            if result.is_none() {
                *result = Some(sealed_result.clone());
                changed = true;
            }
        }
    }

    if !changed {
        return Some(raw);
    }

    serde_json::to_string(&content_blocks).ok().or(Some(raw))
}

fn terminal_timeline_content_blocks(
    content: &str,
    content_blocks_json: Option<&str>,
) -> Vec<ContentBlockItem> {
    if let Some(raw) = content_blocks_json {
        if let Ok(blocks) = serde_json::from_str::<Vec<ContentBlockItem>>(raw) {
            if !blocks.is_empty() {
                return blocks;
            }
        }
    }

    if content.is_empty() {
        Vec::new()
    } else {
        vec![ContentBlockItem::Text {
            text: content.to_string(),
        }]
    }
}

async fn finalize_assistant_message_with_terminal_tool_state(
    chat_message_repo: &Arc<dyn ChatMessageRepository>,
    chat_timeline_repo: &Option<Arc<dyn ChatTimelineRepository>>,
    events: &dyn EventSink,
    event_ctx: &EventContextPayload,
    conversation_id: &ChatConversationId,
    message_id: &str,
    role: &str,
    content: &str,
    tool_calls_json: Option<String>,
    content_blocks_json: Option<String>,
    reason: &str,
    agent_run_id: Option<&str>,
) {
    let sealed_tool_calls = seal_unresolved_tool_calls_json(tool_calls_json, reason);
    let sealed_content_blocks = seal_unresolved_content_blocks_json(content_blocks_json, reason);
    let terminal_content_blocks =
        terminal_timeline_content_blocks(content, sealed_content_blocks.as_deref());
    let timeline_items = super::chat_service_streaming::persist_timeline_snapshot_for_run(
        chat_timeline_repo,
        &conversation_id.as_str(),
        &Some(message_id.to_string()),
        &terminal_content_blocks,
        crate::domain::entities::ChatTimelineItemStatus::Finalized,
        agent_run_id,
    )
    .await;
    let _ = super::chat_service_send_background::finalize_assistant_message(
        chat_message_repo,
        events,
        event_ctx,
        message_id,
        role,
        content,
        sealed_tool_calls.as_deref(),
        sealed_content_blocks.as_deref(),
        timeline_items,
    )
    .await;
}

/// Handle successful stream completion: task state transitions and merge auto-completion.
///
/// For TaskExecution context:
/// - If all task steps are completed → transition to PendingReview
/// - If no steps are tracked but output exists → transition to PendingReview
/// - If a HEAD-matched green validation cache exists → transition to PendingReview
/// - Otherwise → transition to Failed (text output alone is not sufficient)
///
/// For Merge context:
/// - Attempts merge auto-completion via git state inspection
async fn persist_shutdown_interrupted_metadata(
    task_repo: &Arc<dyn TaskRepository>,
    task: &crate::domain::entities::Task,
    context: &'static str,
    last_agent_error: Option<&str>,
) {
    let mut metadata_obj = task
        .metadata
        .as_deref()
        .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    if let Some(obj) = metadata_obj.as_object_mut() {
        obj.insert("shutdown_interrupted".to_string(), serde_json::json!(true));
        obj.insert(
            "last_agent_error_context".to_string(),
            serde_json::json!(context),
        );
        if let Some(error) = last_agent_error {
            obj.insert("last_agent_error".to_string(), serde_json::json!(error));
            obj.insert(
                "last_agent_error_at".to_string(),
                serde_json::json!(chrono::Utc::now().to_rfc3339()),
            );
        }
    }

    let updated_metadata = serde_json::to_string(&metadata_obj).unwrap_or_default();
    let _ = task_repo
        .update_metadata(&task.id, Some(updated_metadata))
        .await;
}

fn stream_error_recovery_reason_code(
    stream_error: &StreamError,
) -> crate::domain::entities::ExecutionRecoveryReasonCode {
    use crate::domain::entities::ExecutionRecoveryReasonCode;
    match stream_error {
        StreamError::Timeout { .. } => ExecutionRecoveryReasonCode::Timeout,
        StreamError::ParseStall { .. } => ExecutionRecoveryReasonCode::ParseStall,
        StreamError::AgentExit { .. } => ExecutionRecoveryReasonCode::AgentExit,
        StreamError::LocalToolFailed { .. } => ExecutionRecoveryReasonCode::LocalToolFailed,
        StreamError::ValidationFailed { .. } => ExecutionRecoveryReasonCode::ValidationFailed,
        _ => ExecutionRecoveryReasonCode::Unknown,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_stream_success(
    agent_run_id: &str,
    context_type: ChatContextType,
    context_id: &str,
    has_output: bool,
    completion_tool_called: bool,
    execution_slot_held: bool,
    execution_state: &Option<Arc<ExecutionState>>,
    task_repo: &Arc<dyn TaskRepository>,
    task_dependency_repo: &Arc<dyn TaskDependencyRepository>,
    project_repo: &Arc<dyn ProjectRepository>,
    artifact_repo: &Arc<dyn ArtifactRepository>,
    chat_message_repo: &Arc<dyn ChatMessageRepository>,
    chat_attachment_repo: &Arc<dyn ChatAttachmentRepository>,
    conversation_repo: &Arc<dyn ChatConversationRepository>,
    agent_run_repo: &Arc<dyn AgentRunRepository>,
    ideation_session_repo: &Arc<dyn IdeationSessionRepository>,
    activity_event_repo: &Arc<dyn ActivityEventRepository>,
    message_queue: &Arc<MessageQueue>,
    running_agent_registry: &Arc<dyn RunningAgentRegistry>,
    memory_event_repo: &Arc<dyn MemoryEventRepository>,
    plan_branch_repo: &Option<Arc<dyn PlanBranchRepository>>,
    task_step_repo: &Option<Arc<dyn TaskStepRepository>>,
    validation_run_repo: &Option<Arc<dyn ValidationRunRepository>>,
    external_events_repo: &Option<Arc<dyn ExternalEventsRepository>>,
    webhook_publisher: &Option<Arc<dyn WebhookPublisher>>,
    execution_settings_repo: &Option<Arc<dyn ExecutionSettingsRepository>>,
    agent_lane_settings_repo: &Option<Arc<dyn AgentLaneSettingsRepository>>,
    agent_provider_settings_repo: &Option<Arc<dyn AgentProviderSettingsRepository>>,
    events: &Arc<dyn EventSink>,
    runtime_factory_deps: Option<&ChatRuntimeFactoryDeps>,
    interactive_process_registry: &Option<Arc<InteractiveProcessRegistry>>,
    review_repo: &Option<Arc<dyn ReviewRepository>>,
    verification_child_registry: &Option<
        Arc<super::verification_child_process_registry::VerificationChildProcessRegistry>,
    >,
) {
    let runtime_support = RuntimeSupportRepos::new(
        execution_settings_repo,
        agent_lane_settings_repo,
        agent_provider_settings_repo,
        plan_branch_repo,
        interactive_process_registry,
        task_step_repo,
        validation_run_repo,
    )
    .with_runtime_factory_deps(runtime_factory_deps)
    .with_completion_event_delivery(external_events_repo, webhook_publisher);

    // Handle task state transition (only for TaskExecution)
    if context_type == ChatContextType::TaskExecution {
        if let Some(ref exec_state) = execution_state {
            let task_id = TaskId::from_string(context_id.to_string());
            if let Ok(Some(task)) = task_repo.get_by_id(&task_id).await {
                if task.internal_status == InternalStatus::Executing
                    || task.internal_status == InternalStatus::ReExecuting
                {
                    // L1 shutdown guard: skip transitions during clean shutdown.
                    // Task stays in Executing/ReExecuting so Phase 2 of StartupJobRunner can resume it.
                    if exec_state.is_shutting_down.load(Ordering::SeqCst) {
                        tracing::info!(
                            task_id = task_id.as_str(),
                            "Shutdown detected — skipping task execution transition; task stays in Executing for auto-recovery"
                        );
                        persist_shutdown_interrupted_metadata(task_repo, &task, "execution", None)
                            .await;
                        return;
                    }

                    let attempt_resolution = resolve_current_execution_attempt(
                        &task_id,
                        agent_run_id,
                        task_repo,
                        agent_run_repo,
                    )
                    .await;
                    let (current_task_for_gate, episode_entered_at) = match attempt_resolution {
                        AttemptResolution::Current {
                            task,
                            episode_entered_at,
                        } => (*task, Some(episode_entered_at)),
                        AttemptResolution::IdentityUnknown => {
                            tracing::warn!(
                                task_id = task_id.as_str(),
                                agent_run_id,
                                "Execution attempt identity unknown; disabling validation-cache rescue and using step-gated completion"
                            );
                            (task.clone(), None)
                        }
                        AttemptResolution::Stale => {
                            tracing::info!(
                                task_id = task_id.as_str(),
                                agent_run_id,
                                "Skipping stale task-execution completion for an older attempt"
                            );
                            return;
                        }
                    };

                    // Create scheduler for auto-scheduling next Ready task
                    let scheduler_svc = build_task_scheduler_service(
                        Arc::clone(project_repo),
                        Arc::clone(task_repo),
                        Arc::clone(task_dependency_repo),
                        Arc::clone(artifact_repo),
                        Arc::clone(chat_message_repo),
                        Arc::clone(chat_attachment_repo),
                        Arc::clone(conversation_repo),
                        Arc::clone(agent_run_repo),
                        Arc::clone(ideation_session_repo),
                        Arc::clone(activity_event_repo),
                        Arc::clone(message_queue),
                        Arc::clone(running_agent_registry),
                        Arc::clone(exec_state),
                        Arc::clone(memory_event_repo),
                        runtime_support.clone(),
                    );
                    let scheduler_concrete = Arc::new(scheduler_svc);
                    scheduler_concrete
                        .set_self_ref(Arc::clone(&scheduler_concrete) as Arc<dyn TaskScheduler>);
                    let task_scheduler: Arc<dyn TaskScheduler> = scheduler_concrete;

                    let transition_service = build_transition_service(
                        Arc::clone(task_repo),
                        Arc::clone(task_dependency_repo),
                        Arc::clone(project_repo),
                        Arc::clone(artifact_repo),
                        Arc::clone(chat_message_repo),
                        Arc::clone(chat_attachment_repo),
                        Arc::clone(conversation_repo),
                        Arc::clone(agent_run_repo),
                        Arc::clone(ideation_session_repo),
                        Arc::clone(activity_event_repo),
                        Arc::clone(message_queue),
                        Arc::clone(running_agent_registry),
                        Arc::clone(exec_state),
                        Arc::clone(memory_event_repo),
                        runtime_support.clone(),
                    )
                    .with_task_scheduler(task_scheduler);
                    let step_state = fetch_step_completion_state(task_step_repo, &task_id).await;
                    let validation_complete = if let Some(episode_entered_at) = episode_entered_at {
                        validated_completion_override(
                            &current_task_for_gate,
                            episode_entered_at,
                            validation_run_repo,
                        )
                        .await
                    } else {
                        false
                    };
                    let mut completion_action = execution_completion_action(
                        has_output,
                        step_state,
                        completion_tool_called,
                        validation_complete,
                    );
                    let mut completion_blocked_error: Option<String> = None;
                    if completion_action == ExecutionCompletionAction::PendingReview {
                        let project_for_gate = project_repo
                            .get_by_id(&current_task_for_gate.project_id)
                            .await;
                        let diff_guard_result = match project_for_gate {
                            Ok(Some(project)) => {
                                ensure_task_has_non_empty_captured_diff(
                                    &current_task_for_gate,
                                    &project,
                                    "stream_success_completion",
                                )
                                .await
                                .map_err(|error| error.to_string())
                            }
                            Ok(None) => Err(format!(
                                "empty_task_diff_guard: project {} for task {} was not found during stream_success_completion",
                                current_task_for_gate.project_id.as_str(),
                                task_id.as_str()
                            )),
                            Err(error) => Err(format!(
                                "empty_task_diff_guard: failed to load project {} for task {} during stream_success_completion: {}",
                                current_task_for_gate.project_id.as_str(),
                                task_id.as_str(),
                                error
                            )),
                        };
                        if let Err(error) = diff_guard_result {
                            tracing::warn!(
                                task_id = task_id.as_str(),
                                error = %error,
                                "Worker completion downgraded to failure because task-owned diff is empty or unavailable"
                            );
                            completion_blocked_error = Some(error);
                            completion_action = ExecutionCompletionAction::Failed;
                        }
                    }

                    if completion_action == ExecutionCompletionAction::PendingReview
                        && step_state == StepCompletionState::AllComplete
                    {
                        tracing::info!(
                                task_id = task_id.as_str(),
                                "Worker run ended with all steps completed; transitioning to PendingReview"
                            );
                        if let Err(e) = transition_service
                            .transition_execution_completed_to_review(&task_id, agent_run_id)
                            .await
                        {
                            tracing::error!(
                                "Failed to transition all-steps-done task {} to PendingReview: {}",
                                task_id.as_str(),
                                e
                            );
                        }
                    } else if completion_action == ExecutionCompletionAction::PendingReview {
                        if validation_complete {
                            tracing::info!(
                                task_id = task_id.as_str(),
                                "Worker run ended without all steps completed but a HEAD-matched green validation cache proves completion; transitioning to PendingReview"
                            );
                        } else if completion_tool_called {
                            tracing::info!(
                                task_id = task_id.as_str(),
                                "Worker run called execution_complete with no tracked steps; transitioning to PendingReview"
                            );
                        } else {
                            tracing::info!(
                                task_id = task_id.as_str(),
                                "Worker run reached completion gate; transitioning to PendingReview"
                            );
                        }
                        if let Err(e) = transition_service
                            .transition_execution_completed_to_review(&task_id, agent_run_id)
                            .await
                        {
                            tracing::error!(
                                "Failed to transition task {} to PendingReview: {}",
                                task_id.as_str(),
                                e
                            );
                        }
                    } else {
                        let current_task = match resolve_current_execution_attempt(
                            &task_id,
                            agent_run_id,
                            task_repo,
                            agent_run_repo,
                        )
                        .await
                        {
                            AttemptResolution::Current { task, .. } => *task,
                            AttemptResolution::IdentityUnknown => current_task_for_gate.clone(),
                            AttemptResolution::Stale => {
                                tracing::info!(
                                    task_id = task_id.as_str(),
                                    agent_run_id,
                                    "Skipping stale incomplete execution finalizer; task is no longer in the same execution attempt"
                                );
                                return;
                            }
                        };

                        // Store last_agent_error for empty-output failure
                        let mut metadata_obj = current_task
                            .metadata
                            .as_deref()
                            .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
                            .unwrap_or_else(|| serde_json::json!({}));
                        if let Some(obj) = metadata_obj.as_object_mut() {
                            let incomplete_message = completion_blocked_error
                                .as_deref()
                                .unwrap_or("Agent ended without completing all task steps");
                            obj.insert(
                                "last_agent_error".to_string(),
                                serde_json::json!(incomplete_message),
                            );
                            obj.insert(
                                "last_agent_error_context".to_string(),
                                serde_json::json!("execution"),
                            );
                            obj.insert(
                                "last_agent_error_at".to_string(),
                                serde_json::json!(chrono::Utc::now().to_rfc3339()),
                            );

                            use crate::domain::entities::{
                                ExecutionFailureSource, ExecutionRecoveryEvent,
                                ExecutionRecoveryEventKind, ExecutionRecoveryMetadata,
                                ExecutionRecoveryReasonCode, ExecutionRecoverySource,
                                ExecutionRecoveryState,
                            };
                            let recovery_event = ExecutionRecoveryEvent::new(
                                ExecutionRecoveryEventKind::Failed,
                                ExecutionRecoverySource::System,
                                ExecutionRecoveryReasonCode::IncompleteSteps,
                                incomplete_message,
                            )
                            .with_failure_source(ExecutionFailureSource::AgentIncomplete);
                            let mut recovery = ExecutionRecoveryMetadata::from_task_metadata(
                                current_task.metadata.as_deref(),
                            )
                            .unwrap_or(None)
                            .unwrap_or_default();
                            recovery.append_event_with_state(
                                recovery_event,
                                ExecutionRecoveryState::Failed,
                            );
                            if let Ok(recovery_value) = serde_json::to_value(&recovery) {
                                obj.insert("execution_recovery".to_string(), recovery_value);
                            }
                        }
                        let updated_metadata =
                            serde_json::to_string(&metadata_obj).unwrap_or_default();
                        let _ = task_repo
                            .update_metadata(&task_id, Some(updated_metadata))
                            .await;

                        if let Err(e) = transition_service
                            .transition_task(&task_id, InternalStatus::Failed)
                            .await
                        {
                            tracing::error!(
                                "Failed to transition empty-output task {} to Failed: {}",
                                task_id.as_str(),
                                e
                            );
                        } else {
                            tracing::warn!(
                                task_id = task_id.as_str(),
                                "Task execution produced no output; transitioned to Failed"
                            );
                        }
                    }
                }
            }
        } else {
            tracing::warn!(
                "Cannot transition task {} - no execution_state available",
                context_id
            );
        }
    }

    // Handle review completion without complete_review call (task still in Reviewing)
    if context_type == ChatContextType::Review {
        if let Some(ref exec_state) = execution_state {
            let task_id = TaskId::from_string(context_id.to_string());
            if let Ok(Some(task)) = task_repo.get_by_id(&task_id).await {
                match incomplete_review_action(
                    task.internal_status,
                    exec_state.is_shutting_down.load(Ordering::SeqCst),
                ) {
                    IncompleteReviewAction::SkipDuringShutdown => {
                        // L1 shutdown guard: skip escalation during clean app shutdown.
                        // The task stays in Reviewing so StartupJobRunner Phase 2 can respawn it.
                        tracing::info!(
                            task_id = task_id.as_str(),
                            "Shutdown detected — skipping review escalation; task stays in Reviewing for auto-recovery"
                        );
                        persist_shutdown_interrupted_metadata(task_repo, &task, "review", None)
                            .await;
                        return;
                    }
                    IncompleteReviewAction::Escalate => {
                        tracing::info!(
                            task_id = task_id.as_str(),
                            "Review agent completed without calling complete_review; escalating"
                        );

                        // Store info in metadata for UI visibility
                        let mut metadata_obj = task
                            .metadata
                            .as_deref()
                            .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
                            .unwrap_or_else(|| serde_json::json!({}));
                        if let Some(obj) = metadata_obj.as_object_mut() {
                            obj.insert(
                                "last_agent_error".to_string(),
                                serde_json::json!(
                                    "Review agent completed without calling complete_review"
                                ),
                            );
                            obj.insert(
                                "last_agent_error_context".to_string(),
                                serde_json::json!("review"),
                            );
                            obj.insert(
                                "last_agent_error_at".to_string(),
                                serde_json::json!(chrono::Utc::now().to_rfc3339()),
                            );
                        }
                        let mut updated_task = task.clone();
                        updated_task.metadata =
                            Some(serde_json::to_string(&metadata_obj).unwrap_or_default());
                        updated_task.touch();
                        let _ = task_repo.update(&updated_task).await;

                        // Store a ReviewNote so the frontend can display why the task was escalated.
                        if let Some(ref repo) = review_repo {
                            let reason = "Review agent exited without calling complete_review";
                            let note = ReviewNote::with_notes(
                                task_id.clone(),
                                ReviewerType::System,
                                ReviewOutcome::Rejected,
                                reason.to_string(),
                            );
                            if let Err(e) = repo.add_note(&note).await {
                                tracing::warn!(
                                    task_id = task_id.as_str(),
                                    error = %e,
                                    "Failed to store escalation ReviewNote after incomplete review"
                                );
                            }
                        }

                        // Transition to Escalated (no scheduler needed)
                        let transition_service = build_transition_service(
                            Arc::clone(task_repo),
                            Arc::clone(task_dependency_repo),
                            Arc::clone(project_repo),
                            Arc::clone(artifact_repo),
                            Arc::clone(chat_message_repo),
                            Arc::clone(chat_attachment_repo),
                            Arc::clone(conversation_repo),
                            Arc::clone(agent_run_repo),
                            Arc::clone(ideation_session_repo),
                            Arc::clone(activity_event_repo),
                            Arc::clone(message_queue),
                            Arc::clone(running_agent_registry),
                            Arc::clone(exec_state),
                            Arc::clone(memory_event_repo),
                            runtime_support.clone(),
                        );

                        if let Err(e) = transition_service
                            .transition_task(&task_id, InternalStatus::Escalated)
                            .await
                        {
                            tracing::error!(
                                task_id = task_id.as_str(),
                                error = %e,
                                "Failed to transition reviewing task to Escalated after incomplete review"
                            );
                        }
                    }
                    IncompleteReviewAction::IgnoreAlreadyTransitioned => {
                        // Task has already transitioned past Reviewing (e.g. PendingMerge, Merging).
                        // chat_service_send_background.rs re-incremented running_count before this
                        // handler ran IFF execution_slot_held == false (interactive mode where
                        // TurnComplete freed the slot mid-stream). Negate that re-increment to
                        // prevent a running_count leak that would cause merge deferral checks to
                        // incorrectly see count=1.
                        //
                        // Guard: when execution_slot_held == true (autonomous review), TurnComplete
                        // never freed the slot, so no re-increment happened in send_background.rs.
                        // Decrementing here would cause a spurious underflow (running_count below 0).
                        if !execution_slot_held {
                            let count_before = exec_state.running_count();
                            let count_after = exec_state.decrement_running();
                            tracing::info!(
                                task_id = task_id.as_str(),
                                status = ?task.internal_status,
                                count_before,
                                count_after,
                                "Review context: task already past Reviewing — negating re-increment to prevent running_count leak"
                            );
                        } else {
                            tracing::debug!(
                                task_id = task_id.as_str(),
                                status = ?task.internal_status,
                                "Review context: task past Reviewing but execution_slot_held=true — skipping decrement (no re-increment occurred)"
                            );
                        }
                    }
                }
            }
        } else {
            tracing::warn!(
                "Cannot handle review completion for task {} - no execution_state available",
                context_id
            );
        }
    }

    // Handle merge auto-completion (only for Merge context)
    if context_type == ChatContextType::Merge {
        if let Some(ref exec_state) = execution_state {
            // L1 shutdown guard: skip merge auto-complete during clean shutdown.
            // Task stays in Merging so Phase 2 of StartupJobRunner can resume it.
            if exec_state.is_shutting_down.load(Ordering::SeqCst) {
                tracing::info!(
                    task_id = context_id,
                    "Shutdown detected — skipping merge auto-complete; task stays in Merging for auto-recovery"
                );
                let task_id = TaskId::from_string(context_id.to_string());
                if let Ok(Some(task)) = task_repo.get_by_id(&task_id).await {
                    persist_shutdown_interrupted_metadata(task_repo, &task, "merge", None).await;
                }
                return;
            }

            let merge_ctx = super::chat_service_merge::MergeAutoCompleteContext {
                task_id_str: context_id,
                task_id: TaskId::from_string(context_id.to_string()),
                task_repo,
                task_dependency_repo,
                project_repo,
                artifact_repo,
                chat_message_repo,
                chat_attachment_repo,
                conversation_repo,
                agent_run_repo,
                ideation_session_repo,
                activity_event_repo,
                message_queue,
                running_agent_registry,
                memory_event_repo,
                execution_state: exec_state,
                execution_settings_repo: execution_settings_repo.as_ref(),
                plan_branch_repo,
                events,
                runtime_factory_deps,
                interactive_process_registry,
            };
            super::chat_service_merge::attempt_merge_auto_complete(&merge_ctx).await;
        } else {
            tracing::warn!(
                "Cannot auto-complete merge for task {} - no execution_state available",
                context_id
            );
        }
    }

    // Path A: Handle verification child completion (only for Ideation context)
    if context_type == ChatContextType::Ideation {
        let child_id = IdeationSessionId::from_string(context_id.to_string());
        match ideation_session_repo.get_by_id(&child_id).await {
            Ok(Some(child_session)) => {
                if child_session.session_purpose == SessionPurpose::Verification {
                    if let Some(parent_id) = child_session.parent_session_id {
                        handle_verification_child_completion(
                            &child_id,
                            &parent_id,
                            ideation_session_repo,
                            conversation_repo,
                            chat_message_repo,
                            message_queue,
                            runtime_factory_deps.and_then(|deps| deps.queued_message_repo.as_ref()),
                            events.as_ref(),
                            verification_child_registry,
                        )
                        .await;
                    }
                }
            }
            Ok(None) => {
                tracing::debug!(
                    context_id,
                    "Ideation session not found for verification reconciliation check"
                );
            }
            Err(e) => {
                tracing::warn!(
                    context_id,
                    error = %e,
                    "Failed to fetch ideation session for verification reconciliation check"
                );
            }
        }
    }
}

/// Check whether a task is still in an active execution state that needs recovery.
///
/// Returns `true` if the task is in `Executing` or `ReExecuting` — the "stuck" states that
/// warrant a transition retry. Returns `false` if the task has already transitioned (e.g.,
/// auto-complete resolved it), or if the task was not found. Returns `true` on repo errors
/// so the retry is attempted defensively rather than silently dropped.
pub(super) async fn task_still_needs_execution_recovery(
    task_id: &TaskId,
    task_repo: &Arc<dyn TaskRepository>,
) -> bool {
    match task_repo.get_by_id(task_id).await {
        Ok(Some(refreshed)) => {
            refreshed.internal_status == InternalStatus::Executing
                || refreshed.internal_status == InternalStatus::ReExecuting
        }
        Ok(None) => false,
        Err(_) => true,
    }
}

#[derive(Debug)]
enum AttemptResolution {
    Current {
        task: Box<Task>,
        episode_entered_at: chrono::DateTime<chrono::Utc>,
    },
    Stale,
    IdentityUnknown,
}

fn execution_attempt_start_tolerance() -> chrono::Duration {
    let secs = stream_timeouts().execution_attempt_start_tolerance_secs;
    chrono::Duration::seconds(i64::try_from(secs).unwrap_or(i64::MAX))
}

async fn resolve_current_execution_attempt(
    task_id: &TaskId,
    agent_run_id: &str,
    task_repo: &Arc<dyn TaskRepository>,
    agent_run_repo: &Arc<dyn AgentRunRepository>,
) -> AttemptResolution {
    let task = match task_repo.get_by_id(task_id).await {
        Ok(Some(task)) => task,
        Ok(None) => return AttemptResolution::Stale,
        Err(e) => {
            tracing::warn!(
                task_id = task_id.as_str(),
                error = %e,
                "Could not load task while resolving execution attempt"
            );
            return AttemptResolution::IdentityUnknown;
        }
    };

    if !matches!(
        task.internal_status,
        InternalStatus::Executing | InternalStatus::ReExecuting
    ) {
        return AttemptResolution::Stale;
    }

    let status_entered_at = match task_repo
        .get_status_last_entered_at(task_id, task.internal_status)
        .await
    {
        Ok(Some(entered_at)) => entered_at,
        Ok(None) => return AttemptResolution::IdentityUnknown,
        Err(e) => {
            tracing::warn!(
                task_id = task_id.as_str(),
                error = %e,
                "Could not load latest execution status entry"
            );
            return AttemptResolution::IdentityUnknown;
        }
    };

    let agent_run = match agent_run_repo
        .get_by_id(&AgentRunId::from_string(agent_run_id))
        .await
    {
        Ok(Some(run)) => run,
        Ok(None) => return AttemptResolution::IdentityUnknown,
        Err(e) => {
            tracing::warn!(
                task_id = task_id.as_str(),
                agent_run_id,
                error = %e,
                "Could not load agent run while resolving execution attempt"
            );
            return AttemptResolution::IdentityUnknown;
        }
    };

    if let Ok(Some(active_run)) = agent_run_repo
        .get_active_for_conversation(&agent_run.conversation_id)
        .await
    {
        if active_run.id != agent_run.id
            && active_run.run_chain_id.is_some()
            && agent_run.run_chain_id.is_some()
            && active_run.run_chain_id != agent_run.run_chain_id
        {
            return AttemptResolution::Stale;
        }
    }

    if agent_run.started_at + execution_attempt_start_tolerance() >= status_entered_at {
        AttemptResolution::Current {
            task: Box::new(task),
            episode_entered_at: status_entered_at,
        }
    } else {
        AttemptResolution::Stale
    }
}

#[cfg(test)]
async fn task_execution_attempt_matches_current_status(
    task_id: &TaskId,
    agent_run_id: &str,
    task_repo: &Arc<dyn TaskRepository>,
    agent_run_repo: &Arc<dyn AgentRunRepository>,
) -> bool {
    matches!(
        resolve_current_execution_attempt(task_id, agent_run_id, task_repo, agent_run_repo).await,
        AttemptResolution::Current { .. } | AttemptResolution::IdentityUnknown
    )
}

#[cfg(test)]
async fn load_current_task_execution_attempt(
    task_id: &TaskId,
    agent_run_id: &str,
    task_repo: &Arc<dyn TaskRepository>,
    agent_run_repo: &Arc<dyn AgentRunRepository>,
) -> Option<crate::domain::entities::Task> {
    match resolve_current_execution_attempt(task_id, agent_run_id, task_repo, agent_run_repo).await
    {
        AttemptResolution::Current { task, .. } => Some(*task),
        AttemptResolution::Stale | AttemptResolution::IdentityUnknown => None,
    }
}

/// Handle stream error: classify error, attempt stale session recovery,
/// fail agent run, finalize message, emit error event, and transition task to Failed.
///
/// Accepts both the typed `StreamError` (for precise matching) and a pre-formatted
/// error string (for backward-compatible logging and message storage).
///
/// Returns `true` if recovery was successful and a retry was spawned (caller should return early).
/// Returns `false` if normal error handling was performed.
#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_stream_error(
    error: &str,
    stream_error: Option<&StreamError>,
    context_type: ChatContextType,
    context_id: &str,
    conversation_id: ChatConversationId,
    agent_run_id: &str,
    pre_assistant_msg_id: &str,
    event_ctx: &EventContextPayload,
    stored_session_id: Option<&str>,
    effective_harness: AgentHarnessKind,
    is_retry_attempt: bool,
    persona_feature_enabled: bool,
    agent_name_override_set: bool,
    user_message_content: Option<&str>,
    conversation: Option<&ChatConversation>,
    resolved_project_id: Option<String>,
    cli_path: &Path,
    plugin_dir: &Path,
    working_directory: &Path,
    chat_message_repo: &Arc<dyn ChatMessageRepository>,
    chat_timeline_repo: &Option<Arc<dyn ChatTimelineRepository>>,
    chat_attachment_repo: &Arc<dyn ChatAttachmentRepository>,
    artifact_repo: &Arc<dyn ArtifactRepository>,
    conversation_repo: &Arc<dyn ChatConversationRepository>,
    agent_run_repo: &Arc<dyn AgentRunRepository>,
    task_repo: &Arc<dyn TaskRepository>,
    task_dependency_repo: &Arc<dyn TaskDependencyRepository>,
    project_repo: &Arc<dyn ProjectRepository>,
    ideation_session_repo: &Arc<dyn IdeationSessionRepository>,
    task_proposal_repo: &Option<Arc<dyn TaskProposalRepository>>,
    activity_event_repo: &Arc<dyn ActivityEventRepository>,
    message_queue: &Arc<MessageQueue>,
    running_agent_registry: &Arc<dyn RunningAgentRegistry>,
    memory_event_repo: &Arc<dyn MemoryEventRepository>,
    execution_state: &Option<Arc<ExecutionState>>,
    question_state: &Option<Arc<QuestionState>>,
    plan_branch_repo: &Option<Arc<dyn PlanBranchRepository>>,
    execution_settings_repo: &Option<Arc<dyn ExecutionSettingsRepository>>,
    agent_lane_settings_repo: &Option<Arc<dyn AgentLaneSettingsRepository>>,
    agent_provider_settings_repo: &Option<Arc<dyn AgentProviderSettingsRepository>>,
    events: Arc<dyn EventSink>,
    _plan_verification_completion: Option<
        &Arc<crate::application::plan_verification_service::PlanVerificationCompletionAdapter>,
    >,
    runtime_factory_deps: Option<&ChatRuntimeFactoryDeps>,
    agent_name: Option<&str>,
    run_chain_id: Option<String>,
    interactive_process_registry: &Option<Arc<InteractiveProcessRegistry>>,
    review_repo: &Option<Arc<dyn ReviewRepository>>,
    task_step_repo: &Option<Arc<dyn TaskStepRepository>>,
    validation_run_repo: &Option<Arc<dyn ValidationRunRepository>>,
    external_events_repo: &Option<Arc<dyn ExternalEventsRepository>>,
    webhook_publisher: &Option<Arc<dyn WebhookPublisher>>,
    verification_child_registry: &Option<
        Arc<super::verification_child_process_registry::VerificationChildProcessRegistry>,
    >,
    notification_service: &Option<Arc<NotificationService>>,
) -> bool {
    let runtime_support = RuntimeSupportRepos::new(
        execution_settings_repo,
        agent_lane_settings_repo,
        agent_provider_settings_repo,
        plan_branch_repo,
        interactive_process_registry,
        task_step_repo,
        validation_run_repo,
    )
    .with_events(Arc::clone(&events))
    .with_runtime_factory_deps(runtime_factory_deps)
    .with_completion_event_delivery(external_events_repo, webhook_publisher);
    let conversation_provider_session_ref =
        conversation.and_then(|conv| conv.provider_session_ref());
    let stored_provider_harness = conversation_provider_session_ref
        .as_ref()
        .map(|session_ref| session_ref.harness)
        .or_else(|| stored_session_id.map(|_| effective_harness));
    let stored_provider_session_id = stored_session_id
        .map(|session_id| session_id.to_string())
        .or_else(|| {
            conversation_provider_session_ref
                .as_ref()
                .map(|session_ref| session_ref.provider_session_id.clone())
        });

    // Handle cancellation — distinguish "cancelled after normal completion" from "user stop"
    if let Some(StreamError::Cancelled {
        turns_finalized,
        completion_tool_called,
    }) = stream_error
    {
        if *turns_finalized > 0 {
            // Agent completed at least one turn (TurnComplete received) before the
            // prune engine or other system cancellation killed the stream. The work
            // is done — honour the completion by running the normal success path.
            tracing::info!(
                conversation_id = conversation_id.as_str(),
                context_type = %context_type,
                context_id,
                turns_finalized,
                "Stream cancelled after TurnComplete — treating as normal completion"
            );
            let _ = agent_run_repo
                .complete(&AgentRunId::from_string(agent_run_id))
                .await;

            // Re-increment to counteract double-decrement (TurnComplete released slot, on_exit will release again)
            if super::uses_execution_slot(context_type) {
                if let Some(ref exec) = execution_state {
                    exec.increment_running();
                    tracing::debug!(
                        %context_type,
                        context_id,
                        "Re-incremented before state transition to prevent double-decrement (cancellation path)"
                    );
                }
            }

            handle_stream_success(
                agent_run_id,
                context_type,
                context_id,
                true, // effective_has_output: turns were finalized → agent produced output
                *completion_tool_called,
                false, // execution_slot_held=false: re-increment happened above at line ~570
                execution_state,
                task_repo,
                task_dependency_repo,
                project_repo,
                artifact_repo,
                chat_message_repo,
                chat_attachment_repo,
                conversation_repo,
                agent_run_repo,
                ideation_session_repo,
                activity_event_repo,
                message_queue,
                running_agent_registry,
                memory_event_repo,
                plan_branch_repo,
                task_step_repo,
                validation_run_repo,
                external_events_repo,
                webhook_publisher,
                execution_settings_repo,
                agent_lane_settings_repo,
                agent_provider_settings_repo,
                &events,
                runtime_factory_deps,
                interactive_process_registry,
                review_repo,
                verification_child_registry,
            )
            .await;

            // Emit run_completed to reset frontend from "generating" → "idle".
            // This is a success path — the agent completed work before the stream
            // was cancelled. Without this emission, the UI stays stuck in "generating".
            // Do NOT emit agent:error here — that would destroy pending plans.
            tracing::info!(
                context_type = %context_type,
                context_id,
                turns_finalized,
                "[LIFECYCLE] Cancelled+turns_finalized>0 — emitting run_completed (success path)"
            );
            let _ = emit_serialized(
                events.as_ref(),
                "agent:run_completed",
                &AgentRunCompletedPayload::with_provider_session_and_run_id(
                    Some(agent_run_id.to_string()),
                    conversation_id.as_str().to_string(),
                    context_type.to_string(),
                    context_id.to_string(),
                    stored_provider_harness,
                    stored_provider_session_id.clone(),
                    run_chain_id.clone(),
                ),
            );
            return false;
        }

        // Sub-branch B: completion tool was called but TurnComplete never arrived.
        // This happens when finalize_proposals (or equivalent) calls execution_complete
        // and the 200ms cleanup delay fires running_agent_registry.stop() before the
        // TurnComplete event is emitted. The agent finished its work — treat as success.
        if *completion_tool_called {
            debug_assert!(
                matches!(context_type, ChatContextType::Ideation),
                "completion_tool_called=true with turns_finalized=0 is only expected for Ideation context; got {:?}",
                context_type
            );
            tracing::info!(
                conversation_id = conversation_id.as_str(),
                context_type = %context_type,
                context_id,
                "[LIFECYCLE] Cancelled+completion_tool_called=true+turns_finalized=0 — routing to success path"
            );
            let _ = agent_run_repo
                .complete(&AgentRunId::from_string(agent_run_id))
                .await;

            // Skip execution slot re-increment: no TurnComplete was fired, so no prior
            // decrement happened that we need to compensate for.

            handle_stream_success(
                agent_run_id,
                context_type,
                context_id,
                true, // effective_has_output: completion tool was called → agent produced output
                true,
                false, // execution_slot_held=false: no TurnComplete decrement to compensate
                execution_state,
                task_repo,
                task_dependency_repo,
                project_repo,
                artifact_repo,
                chat_message_repo,
                chat_attachment_repo,
                conversation_repo,
                agent_run_repo,
                ideation_session_repo,
                activity_event_repo,
                message_queue,
                running_agent_registry,
                memory_event_repo,
                plan_branch_repo,
                task_step_repo,
                validation_run_repo,
                external_events_repo,
                webhook_publisher,
                execution_settings_repo,
                agent_lane_settings_repo,
                agent_provider_settings_repo,
                &events,
                runtime_factory_deps,
                interactive_process_registry,
                review_repo,
                verification_child_registry,
            )
            .await;

            // Emit run_completed to reset frontend from "generating" → "idle".
            let _ = emit_serialized(
                events.as_ref(),
                "agent:run_completed",
                &AgentRunCompletedPayload::with_provider_session_and_run_id(
                    Some(agent_run_id.to_string()),
                    conversation_id.as_str().to_string(),
                    context_type.to_string(),
                    context_id.to_string(),
                    stored_provider_harness,
                    stored_provider_session_id.clone(),
                    run_chain_id.clone(),
                ),
            );
            return false;
        }

        // turns_finalized == 0 && !completion_tool_called: genuine user-initiated stop or
        // system cancel before completion.
        tracing::info!(
            conversation_id = conversation_id.as_str(),
            context_type = %context_type,
            context_id,
            "Stream cancelled — skipping error recovery and fallback transitions"
        );
        if effective_harness == AgentHarnessKind::Codex {
            if let Err(error) = conversation_repo
                .clear_provider_session_ref(&conversation_id)
                .await
            {
                tracing::warn!(
                    conversation_id = conversation_id.as_str(),
                    %error,
                    "Failed to clear provider session after an incomplete Codex turn was cancelled"
                );
            } else {
                tracing::info!(
                    conversation_id = conversation_id.as_str(),
                    "Cleared provider session after an incomplete Codex turn was cancelled"
                );
            }
        }
        mark_cancelled_stream_as_cancelled(
            agent_run_repo,
            agent_run_id,
            context_type,
            context_id,
            task_repo,
        )
        .await;

        // Update pre-created message — append stop note to any content already flushed
        let (existing_content, existing_tool_calls, existing_content_blocks) =
            read_existing_message_content(chat_message_repo, pre_assistant_msg_id).await;
        let stop_note = if existing_content.is_empty() {
            "[Agent stopped]".to_string()
        } else {
            format!("{}\n\n[Agent stopped]", existing_content)
        };
        finalize_assistant_message_with_terminal_tool_state(
            chat_message_repo,
            chat_timeline_repo,
            events.as_ref(),
            event_ctx,
            &conversation_id,
            pre_assistant_msg_id,
            &get_assistant_role(&context_type).to_string(),
            &stop_note,
            existing_tool_calls,
            existing_content_blocks,
            "stopped",
            Some(agent_run_id),
        )
        .await;

        events.emit(
            "agent:stopped",
            serde_json::json!({
                "conversation_id": conversation_id.as_str(),
                "agent_run_id": agent_run_id,
                "context_type": context_type.to_string(),
                "context_id": context_id,
            }),
        );

        // Path C: Reset verification state when a verification child is stopped by user
        if context_type == ChatContextType::Ideation {
            let child_id = IdeationSessionId::from_string(context_id.to_string());
            crate::application::reconciliation::verification_reconciliation::reset_verification_on_child_error::<tauri::Wry>(
                &child_id,
                ideation_session_repo,
                None,
                "user_stopped",
            )
            .await;
        }

        return false;
    }

    let mut terminal_error_override = None;

    // Classify error to detect stale session
    let classified_error = classify_agent_error(error, &conversation_id, stored_session_id);

    match classified_error {
        AppError::StaleSession { session_id, .. } => {
            tracing::warn!(
                event = "stale_session_detected",
                session_id = %session_id,
                conversation_id = conversation_id.as_str(),
                context_type = %context_type,
                context_id = %context_id,
                "Detected stale provider session"
            );

            // Feature flag check
            let recovery_enabled = std::env::var("ENABLE_SESSION_RECOVERY")
                .map(|v| v.to_lowercase() == "true")
                .unwrap_or(false);

            // Check retry flag (prevent infinite loop)
            if is_retry_attempt {
                tracing::error!(
                    conversation_id = conversation_id.as_str(),
                    "Session recovery failed on retry, aborting"
                );
                // Fall through to normal error handling below
            } else if !recovery_enabled {
                tracing::info!(
                    "Session recovery disabled by ENABLE_SESSION_RECOVERY flag, falling back to clear"
                );
                // Fall through to clear session
            } else if let (Some(msg), Some(conv)) = (user_message_content, conversation) {
                let recovery_harness = conv
                    .provider_session_ref()
                    .map(|session_ref| session_ref.harness)
                    .unwrap_or(effective_harness);
                // Attempt recovery
                match super::chat_service_recovery::attempt_session_recovery(
                    &conversation_id,
                    conv,
                    recovery_harness,
                    context_type,
                    context_id,
                    msg,
                    cli_path,
                    plugin_dir,
                    working_directory,
                    resolved_project_id.clone(),
                    Arc::clone(chat_message_repo),
                    Arc::clone(conversation_repo),
                    Arc::clone(chat_attachment_repo),
                    Arc::clone(artifact_repo),
                    Some(Arc::clone(ideation_session_repo)),
                    task_proposal_repo.clone(),
                    Arc::clone(agent_run_repo),
                    agent_run_id,
                    agent_provider_settings_repo.as_ref().map(Arc::clone),
                    persona_feature_enabled,
                    agent_name_override_set,
                    &session_id,
                    runtime_factory_deps,
                    events.as_ref(),
                )
                .await
                {
                    Ok(new_session_id) => {
                        tracing::info!(
                            event = "rehydrate_success",
                            old_session = %session_id,
                            new_session = %new_session_id,
                            "Session recovery successful, retrying send"
                        );

                        // Emit non-blocking banner event
                        events.emit(
                            "agent:session_recovered",
                            serde_json::json!({
                                "conversation_id": conversation_id.as_str(),
                                "message": "Session restored from local history"
                            }),
                        );

                        // Retry send with fresh session (set is_retry=true)
                        let mut retry_conv = conv.clone();
                        retry_conv.set_provider_session_ref(
                            crate::domain::agents::ProviderSessionRef {
                                harness: recovery_harness,
                                provider_session_id: new_session_id.clone(),
                            },
                        );
                        let retry_app_repos =
                            RecoveryRetryAppRepos::from_runtime_factory_deps(runtime_factory_deps);
                        let retry_agent_lane_settings_repo =
                            agent_lane_settings_repo.as_ref().map(Arc::clone);
                        let retry_agent_provider_settings_repo =
                            agent_provider_settings_repo.as_ref().map(Arc::clone);
                        let retry_persona = resolve_recovery_retry_persona(
                            runtime_factory_deps,
                            persona_feature_enabled,
                            conv,
                            conv.context_type,
                            agent_name_override_set,
                        )
                        .await;
                        let retry_persona_for_attribution = retry_persona
                            .as_ref()
                            .ok()
                            .and_then(|persona| persona.clone());
                        let retry_folder_refs = recovery_retry_folder_refs_context(
                            runtime_factory_deps,
                            conv,
                            resolved_project_id.as_deref(),
                            working_directory,
                        )
                        .await;
                        let retry_agent_runtime_context = if let Some(deps) = runtime_factory_deps {
                            let workspace = match deps.agent_conversation_workspace_repo.as_ref() {
                                Some(repo) => match conv.context_type {
                                    ChatContextType::Project | ChatContextType::Standalone => {
                                        repo.get_by_conversation_id(&conv.id).await
                                    }
                                    ChatContextType::Ideation => {
                                        repo.get_by_linked_ideation_session_id(
                                            &IdeationSessionId::from_string(
                                                conv.context_id.to_string(),
                                            ),
                                        )
                                        .await
                                    }
                                    _ => Ok(None),
                                },
                                None => Ok(None),
                            };
                            match workspace {
                                Ok(workspace) => match deps.agent_runtime_context_deps() {
                                    Some(context_deps) => {
                                        compose_agent_runtime_context(
                                            &AgentRuntimeContextScope {
                                                conversation_id: &conv.id,
                                                context_type: conv.context_type,
                                                context_id: conv.context_id.as_str(),
                                                project_id: resolved_project_id.as_deref(),
                                                workspace: workspace.as_ref(),
                                                working_directory,
                                                entity_status: None,
                                            },
                                            &context_deps,
                                        )
                                        .await
                                    }
                                    None => None,
                                },
                                Err(error) => {
                                    tracing::warn!(
                                        conversation_id = %conv.id,
                                        error = %error,
                                        "agent runtime workspace context unavailable during stream retry"
                                    );
                                    None
                                }
                            }
                        } else {
                            None
                        };

                        let retry_agent_name =
                            super::chat_service_helpers::resolve_agent(&context_type, None);
                        let external_readiness = chat_service_context::await_required_external_mcp(
                            None,
                            recovery_harness,
                            plugin_dir,
                            retry_agent_name,
                            None,
                        )
                        .await;
                        let retry_provider_spawnable =
                            match (retry_persona, retry_folder_refs, external_readiness) {
                            (Ok(persona), Ok((folder_refs_block, filesystem_read_roots)), Ok(())) => {
                                chat_service_context::build_resume_command_for_harness_with_folder_refs(
                                    recovery_harness,
                                    cli_path,
                                    plugin_dir,
                                    conv.context_type,
                                    conv.context_id.as_str(),
                                    conv.coordination_mode,
                                    &conversation_id.as_str(),
                                    conv.agent_mode,
                                    Some(agent_run_id),
                                    msg,
                                    persona,
                                    folder_refs_block.as_deref(),
                                    None,
                                    None,
                                    working_directory,
                                    &new_session_id,
                                    resolved_project_id.as_deref(),
                                    &filesystem_read_roots,
                                    if conv.context_type == ChatContextType::Project {
                                        Some(conversation_id.as_str())
                                    } else {
                                        None
                                    },
                                    Arc::clone(chat_attachment_repo),
                                    Arc::clone(artifact_repo),
                                    retry_agent_lane_settings_repo.clone(),
                                    retry_app_repos.ideation_effort_settings_repo.clone(),
                                    retry_app_repos.ideation_model_settings_repo.clone(),
                                    Arc::clone(ideation_session_repo),
                                    Arc::clone(
                                        retry_app_repos
                                            .delegated_session_repo
                                            .as_ref()
                                            .expect("delegated session repo available"),
                                    ),
                                    Arc::clone(task_repo),
                                    &[],
                                    0,
                                    None,
                                    None,
                                    false,
                                    retry_agent_runtime_context.as_deref(),
                                    None,
                                )
                                .await
                            }
                            (Err(error), _, _) => Err(format!("Persona unavailable: {error}")),
                            (_, Err(error), _) => {
                                Err(format!("Folder references unavailable: {error}"))
                            }
                            (_, _, Err(error)) => Err(format!(
                                "External MCP transport is not ready for recovery retry: {error}"
                            )),
                        };
                        let retry_provider_gate = RecoveryRetryProviderGate::new(
                            &retry_agent_provider_settings_repo,
                            recovery_harness,
                            conv.context_type,
                            resolved_project_id.as_deref(),
                            working_directory,
                            runtime_factory_deps,
                        );
                        let retry_spawnable = match resolve_recovery_retry_spawnable(
                            retry_provider_spawnable,
                            retry_provider_gate,
                        )
                        .await
                        {
                            Ok(spawnable) => spawnable,
                            Err(error) => {
                                terminal_error_override = Some(error);
                                None
                            }
                        };

                        if let Some(spawnable) = retry_spawnable {
                            let persona_injected = spawnable.persona_injected();
                            let persona_injection_skipped_reason =
                                spawnable.persona_injection_skipped_reason();
                            if let Ok(retry_child) = spawnable.spawn().await {
                                super::record_persona_run_attribution(
                                    agent_run_repo,
                                    events.as_ref(),
                                    &conversation_id,
                                    agent_run_id,
                                    recovery_harness,
                                    retry_persona_for_attribution.as_ref(),
                                    persona_injected,
                                    persona_injection_skipped_reason,
                                )
                                .await;
                                super::chat_service_send_background::spawn_send_message_background(
                                    build_recovery_retry_background_context(
                                        retry_child,
                                        recovery_harness,
                                        context_type,
                                        context_id,
                                        conversation_id,
                                        agent_run_id,
                                        new_session_id,
                                        working_directory,
                                        cli_path,
                                        plugin_dir,
                                        chat_message_repo,
                                        chat_attachment_repo,
                                        artifact_repo,
                                        conversation_repo,
                                        agent_run_repo,
                                        task_repo,
                                        task_dependency_repo,
                                        project_repo,
                                        ideation_session_repo,
                                        retry_app_repos
                                            .delegated_session_repo
                                            .as_ref()
                                            .expect("delegated session repo available"),
                                        execution_settings_repo,
                                        &retry_agent_lane_settings_repo,
                                        &retry_agent_provider_settings_repo,
                                        task_proposal_repo,
                                        activity_event_repo,
                                        memory_event_repo,
                                        message_queue,
                                        running_agent_registry,
                                        execution_state,
                                        question_state,
                                        plan_branch_repo,
                                        Arc::clone(&events),
                                        runtime_factory_deps.cloned(),
                                        run_chain_id.clone(),
                                        persona_feature_enabled,
                                        agent_name_override_set,
                                        user_message_content,
                                        retry_conv,
                                        agent_name,
                                        review_repo,
                                        task_step_repo,
                                        validation_run_repo,
                                        external_events_repo,
                                        webhook_publisher,
                                        interactive_process_registry,
                                        verification_child_registry,
                                    ),
                                );

                                return true;
                            }
                        }

                        if terminal_error_override.is_none() {
                            tracing::error!("Failed to spawn retry after recovery");
                        }
                        // Fall through to error handling
                    }
                    Err(recovery_err) => {
                        tracing::error!(
                            error = %recovery_err,
                            "Session recovery failed, falling back to clear"
                        );
                        // Fall through to normal error handling
                    }
                }
            }

            // Clear stale provider session reference as fallback
            let _ = conversation_repo
                .clear_provider_session_ref(&conversation_id)
                .await;
        }
        _ => {
            // Non-stale-session errors: clear session if typed error requires it
            if let Some(se) = stream_error {
                if se.requires_session_clear() {
                    tracing::info!(
                        conversation_id = conversation_id.as_str(),
                        error_type = %se,
                        "Clearing provider session due to stream error requiring session reset"
                    );
                    let _ = conversation_repo
                        .clear_provider_session_ref(&conversation_id)
                        .await;
                }
            }
        }
    }

    // Standard error handling (reached if recovery not attempted or failed)
    // Redact secrets from error string before propagating to non-tracing sinks
    let redacted_error = redact(terminal_error_override.as_deref().unwrap_or(error));

    // A late agent-exit or local-tool diagnostic where the work is actually complete: the agent called
    // execution_complete successfully, green validation was cached for the
    // current attempt/HEAD, and the provider process exited before the normal
    // success finalizer ran. Treat this as a successful execution completion
    // before the generic failure path can persist stale stderr or emit
    // agent:error.
    if context_type == ChatContextType::TaskExecution
        && matches!(
            stream_error,
            Some(StreamError::AgentExit { .. } | StreamError::LocalToolFailed { .. })
        )
    {
        if let Some(ref exec_state) = execution_state {
            let task_id = TaskId::from_string(context_id.to_string());
            let completion_proven = if exec_state.is_shutting_down.load(Ordering::SeqCst) {
                false
            } else {
                match resolve_current_execution_attempt(
                    &task_id,
                    agent_run_id,
                    task_repo,
                    agent_run_repo,
                )
                .await
                {
                    AttemptResolution::Current {
                        task,
                        episode_entered_at,
                    } => {
                        validated_completion_override(
                            task.as_ref(),
                            episode_entered_at,
                            validation_run_repo,
                        )
                        .await
                    }
                    _ => false,
                }
            };

            if completion_proven {
                let transition_service = build_transition_service(
                    Arc::clone(task_repo),
                    Arc::clone(task_dependency_repo),
                    Arc::clone(project_repo),
                    Arc::clone(artifact_repo),
                    Arc::clone(chat_message_repo),
                    Arc::clone(chat_attachment_repo),
                    Arc::clone(conversation_repo),
                    Arc::clone(agent_run_repo),
                    Arc::clone(ideation_session_repo),
                    Arc::clone(activity_event_repo),
                    Arc::clone(message_queue),
                    Arc::clone(running_agent_registry),
                    Arc::clone(exec_state),
                    Arc::clone(memory_event_repo),
                    runtime_support.clone(),
                );

                if transition_service
                    .transition_execution_completed_to_review(&task_id, agent_run_id)
                    .await
                    .is_ok()
                {
                    let _ = agent_run_repo
                        .complete(&AgentRunId::from_string(agent_run_id))
                        .await;
                    let (existing_content, existing_tool_calls, existing_content_blocks) =
                        read_existing_message_content(chat_message_repo, pre_assistant_msg_id)
                            .await;
                    finalize_assistant_message_with_terminal_tool_state(
                        chat_message_repo,
                        chat_timeline_repo,
                        events.as_ref(),
                        event_ctx,
                        &conversation_id,
                        pre_assistant_msg_id,
                        &get_assistant_role(&context_type).to_string(),
                        &existing_content,
                        existing_tool_calls,
                        existing_content_blocks,
                        "validation_complete",
                        Some(agent_run_id),
                    )
                    .await;

                    {
                        let _ = emit_serialized(
                            events.as_ref(),
                            "agent:run_completed",
                            &AgentRunCompletedPayload::with_provider_session_and_run_id(
                                Some(agent_run_id.to_string()),
                                conversation_id.as_str().to_string(),
                                context_type.to_string(),
                                context_id.to_string(),
                                stored_provider_harness,
                                stored_provider_session_id.clone(),
                                run_chain_id.clone(),
                            ),
                        );
                    }
                    return false;
                }
            }
        }
    }

    // Fail the agent run
    let _ = agent_run_repo
        .fail(&AgentRunId::from_string(agent_run_id), &redacted_error)
        .await;

    // Gate B+C: If this is a verification child with an already-terminal parent, suppress
    // transcript-suffix append and agent:error emission. Inject handoff if missing.
    // On any DB failure in the gate check, None fallthrough → normal agent:error path.
    if context_type == ChatContextType::Ideation
        && is_verification_child(context_id, ideation_session_repo).await
    {
        if let Some(parent_state) =
            fetch_parent_verification_state(context_id, ideation_session_repo, conversation_repo)
                .await
        {
            if !parent_state.in_progress
                && matches!(
                    parent_state.terminal_status,
                    VerificationStatus::Verified
                        | VerificationStatus::NeedsRevision
                        | VerificationStatus::Skipped
                )
            {
                tracing::info!(
                    context_id,
                    terminal_status = ?parent_state.terminal_status,
                    "Gate B+C: suppressing agent:error for terminal verification child"
                );
                let (existing_content, existing_tool_calls, existing_content_blocks) =
                    read_existing_message_content(chat_message_repo, pre_assistant_msg_id).await;
                finalize_assistant_message_with_terminal_tool_state(
                    chat_message_repo,
                    chat_timeline_repo,
                    events.as_ref(),
                    event_ctx,
                    &conversation_id,
                    pre_assistant_msg_id,
                    &get_assistant_role(&context_type).to_string(),
                    &existing_content,
                    existing_tool_calls,
                    existing_content_blocks,
                    "verification_parent_resolved",
                    Some(agent_run_id),
                )
                .await;
                verification_handoff::inject_verification_handoff_if_missing(
                    &parent_state.parent_id,
                    &parent_state.parent_conversation_id,
                    parent_state.terminal_status,
                    &parent_state.current_gaps,
                    parent_state.convergence_reason.as_deref(),
                    conversation_repo,
                    chat_message_repo,
                    message_queue,
                )
                .await;
                return false;
            }
        }
    }

    if context_type == ChatContextType::Ideation {
        let child_id = IdeationSessionId::from_string(context_id.to_string());
        if let Some(ReconcileVerificationChildCompletion::AutoContinue(request)) =
            crate::application::reconciliation::verification_reconciliation::reset_verification_on_child_error::<tauri::Wry>(
                &child_id,
                ideation_session_repo,
                None,
                "agent_error",
            )
            .await
        {
            queue_verification_auto_continue(
                message_queue,
                &child_id,
                request.continuation_message,
            );
            tracing::info!(
                context_id = child_id.as_str(),
                current_round = request.snapshot.current_round,
                max_rounds = request.snapshot.max_rounds,
                gap_count = request.snapshot.current_gaps.len(),
                "Queued hidden resume-in-place continuation after verification child error"
            );

            let (existing_content, existing_tool_calls, existing_content_blocks) =
                read_existing_message_content(chat_message_repo, pre_assistant_msg_id).await;
            finalize_assistant_message_with_terminal_tool_state(
                chat_message_repo,
                chat_timeline_repo,
                events.as_ref(),
                event_ctx,
                &conversation_id,
                pre_assistant_msg_id,
                &get_assistant_role(&context_type).to_string(),
                &existing_content,
                existing_tool_calls,
                existing_content_blocks,
                "verification_auto_continue",
                Some(agent_run_id),
            )
            .await;

            return false;
        }
    }

    // Read existing content before overwriting — append error to any content already flushed
    let (existing_content, existing_tool_calls, existing_content_blocks) =
        read_existing_message_content(chat_message_repo, pre_assistant_msg_id).await;
    let suppress_transcript_error_note = matches!(
        stream_error,
        Some(StreamError::AgentExit { stderr, .. }) if is_nonfatal_mcp_tool_cancellation(stderr)
    );
    let error_note = if suppress_transcript_error_note {
        tracing::info!(
            conversation_id = conversation_id.as_str(),
            context_type = %context_type,
            context_id,
            "Suppressing non-fatal MCP cancellation note from persisted assistant transcript"
        );
        existing_content
    } else if existing_content.is_empty() {
        format!("{} {}]", super::AGENT_ERROR_PREFIX, redacted_error)
    } else {
        format!(
            "{}\n\n{} {}]",
            existing_content,
            super::AGENT_ERROR_PREFIX,
            redacted_error
        )
    };
    finalize_assistant_message_with_terminal_tool_state(
        chat_message_repo,
        chat_timeline_repo,
        events.as_ref(),
        event_ctx,
        &conversation_id,
        pre_assistant_msg_id,
        &get_assistant_role(&context_type).to_string(),
        &error_note,
        existing_tool_calls,
        existing_content_blocks,
        "interrupted",
        Some(agent_run_id),
    )
    .await;

    if let Some(StreamError::ProviderError {
        category,
        message,
        retry_after,
    }) = stream_error
    {
        let pause_applied = apply_system_wide_provider_pause(
            runtime_factory_deps,
            execution_state.as_ref(),
            Arc::clone(&events),
            category,
            message,
            retry_after,
            context_type,
            context_id,
        )
        .await;

        if pause_applied && should_requeue_after_provider_pause(context_type) {
            if let Some(msg) = user_message_content {
                let queued = message_queue.queue_with_overrides(
                    context_type,
                    context_id.to_string(),
                    msg.to_string(),
                    Some(r#"{"resume_in_place":true}"#.to_string()),
                    None,
                    Some(effective_harness),
                );
                if let Some(repo) =
                    runtime_factory_deps.and_then(|deps| deps.queued_message_repo.as_ref())
                {
                    let key = QueueKey::new(context_type, context_id);
                    if let Err(error) = repo.enqueue_back(&key, &queued).await {
                        tracing::warn!(
                            %context_type,
                            context_id,
                            queued_message_id = queued.id.as_str(),
                            error = %error,
                            "Failed to persist provider-pause queued message"
                        );
                    }
                }
            }
        }
    }

    // For worker execution failures, transition task out of active execution
    // Use StreamError::suggested_task_status() for precise transition when available
    // For ProviderErrors, store metadata and pause instead of failing
    if context_type == ChatContextType::TaskExecution {
        if let Some(ref exec_state) = execution_state {
            let task_id = TaskId::from_string(context_id.to_string());
            let target_status = stream_error
                .and_then(|se| se.suggested_task_status())
                .unwrap_or(InternalStatus::Failed);
            match task_repo.get_by_id(&task_id).await {
                Ok(Some(task))
                    if task.internal_status == InternalStatus::Executing
                        || task.internal_status == InternalStatus::ReExecuting =>
                {
                    // L1 shutdown guard: skip transitions during clean shutdown.
                    // Task stays in Executing/ReExecuting so Phase 2 of StartupJobRunner can resume it.
                    if exec_state.is_shutting_down.load(Ordering::SeqCst) {
                        tracing::info!(
                            task_id = task_id.as_str(),
                            "Shutdown detected — skipping task execution error transition; task stays in Executing for auto-recovery"
                        );
                        persist_shutdown_interrupted_metadata(
                            task_repo,
                            &task,
                            "execution",
                            Some(&redacted_error),
                        )
                        .await;
                        return false;
                    }

                    let attempt_resolution = resolve_current_execution_attempt(
                        &task_id,
                        agent_run_id,
                        task_repo,
                        agent_run_repo,
                    )
                    .await;
                    let (current_task, episode_entered_at) = match attempt_resolution {
                        AttemptResolution::Current {
                            task,
                            episode_entered_at,
                        } => (*task, Some(episode_entered_at)),
                        AttemptResolution::IdentityUnknown => {
                            tracing::warn!(
                                task_id = task_id.as_str(),
                                agent_run_id,
                                "Execution attempt identity unknown during error handling; disabling validation-cache rescue"
                            );
                            (task.clone(), None)
                        }
                        AttemptResolution::Stale => {
                            tracing::info!(
                                task_id = task_id.as_str(),
                                agent_run_id,
                                "Skipping stale task-execution failure for an older attempt"
                            );
                            return false;
                        }
                    };

                    // Store last_agent_error in metadata (mirrors review pattern)
                    {
                        let mut metadata_obj = current_task
                            .metadata
                            .as_deref()
                            .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
                            .unwrap_or_else(|| serde_json::json!({}));
                        if let Some(obj) = metadata_obj.as_object_mut() {
                            obj.insert(
                                "last_agent_error".to_string(),
                                serde_json::json!(redacted_error),
                            );
                            obj.insert(
                                "last_agent_error_context".to_string(),
                                serde_json::json!("execution"),
                            );
                            obj.insert(
                                "last_agent_error_at".to_string(),
                                serde_json::json!(chrono::Utc::now().to_rfc3339()),
                            );

                            // Pre-compute failure metadata for task execution failures so
                            // on_enter(Failed) does not replace the worker error with the
                            // status-only transition's default empty FailedData.
                            if target_status == InternalStatus::Failed
                                && stream_error
                                    .map(|se| !se.is_provider_error())
                                    .unwrap_or(true)
                            {
                                obj.insert(
                                    "failure_error".to_string(),
                                    serde_json::json!(redacted_error),
                                );
                                obj.insert(
                                    "is_timeout".to_string(),
                                    serde_json::json!(matches!(
                                        stream_error,
                                        Some(StreamError::Timeout { .. })
                                    )),
                                );
                            }

                            // Classify failure and write ExecutionRecoveryMetadata alongside
                            // the flat metadata. Provider errors are handled separately
                            // (they → Paused, not Failed) so we skip them here.
                            if let Some(se) = stream_error {
                                if !se.is_provider_error() {
                                    use crate::domain::entities::{
                                        ExecutionRecoveryEvent, ExecutionRecoveryEventKind,
                                        ExecutionRecoveryMetadata, ExecutionRecoverySource,
                                        ExecutionRecoveryState,
                                    };
                                    let failure_source = se.to_execution_failure_source();
                                    let reason_code = stream_error_recovery_reason_code(se);
                                    let recovery_event = ExecutionRecoveryEvent::new(
                                        ExecutionRecoveryEventKind::Failed,
                                        ExecutionRecoverySource::System,
                                        reason_code,
                                        redacted_error.chars().take(500).collect::<String>(),
                                    )
                                    .with_failure_source(failure_source);
                                    let mut recovery =
                                        ExecutionRecoveryMetadata::from_task_metadata(
                                            current_task.metadata.as_deref(),
                                        )
                                        .unwrap_or(None)
                                        .unwrap_or_default();
                                    let recovery_state = if failure_source.is_transient() {
                                        ExecutionRecoveryState::Retrying
                                    } else {
                                        recovery.stop_retrying = true;
                                        ExecutionRecoveryState::Failed
                                    };
                                    recovery
                                        .append_event_with_state(recovery_event, recovery_state);
                                    if let Ok(recovery_value) = serde_json::to_value(&recovery) {
                                        obj.insert(
                                            "execution_recovery".to_string(),
                                            recovery_value,
                                        );
                                    }
                                }
                            }
                        }
                        let updated_metadata =
                            serde_json::to_string(&metadata_obj).unwrap_or_default();
                        let _ = task_repo
                            .update_metadata(&task_id, Some(updated_metadata))
                            .await;
                    }

                    // If this is a provider error → store metadata before pausing
                    if let Some(se) = stream_error {
                        if se.is_provider_error() {
                            if let Some(mut meta) =
                                se.provider_error_metadata(current_task.internal_status)
                            {
                                // Carry forward resume_attempts from existing metadata
                                // so the MAX_RESUME_ATTEMPTS limit works across re-pause cycles
                                if let Some(existing) = super::PauseReason::from_task_metadata(
                                    current_task.metadata.as_deref(),
                                ) {
                                    if let super::PauseReason::ProviderError {
                                        resume_attempts,
                                        ..
                                    } = existing
                                    {
                                        meta.resume_attempts = resume_attempts;
                                    }
                                } else if let Some(existing) =
                                    super::ProviderErrorMetadata::from_task_metadata(
                                        current_task.metadata.as_deref(),
                                    )
                                {
                                    meta.resume_attempts = existing.resume_attempts;
                                }

                                // Redact secrets from provider error message before storing/emitting
                                meta.message = redact(&meta.message);

                                // Write both legacy provider_error and new pause_reason keys
                                let pause_reason = super::PauseReason::ProviderError {
                                    category: meta.category.clone(),
                                    message: meta.message.clone(),
                                    retry_after: meta.retry_after.clone(),
                                    previous_status: meta.previous_status.clone(),
                                    paused_at: meta.paused_at.clone(),
                                    auto_resumable: meta.auto_resumable,
                                    resume_attempts: meta.resume_attempts,
                                };
                                let with_legacy =
                                    meta.write_to_task_metadata(current_task.metadata.as_deref());
                                let updated_metadata =
                                    pause_reason.write_to_task_metadata(Some(&with_legacy));
                                if let Err(e) = task_repo
                                    .update_metadata(&task_id, Some(updated_metadata))
                                    .await
                                {
                                    tracing::error!(
                                        task_id = task_id.as_str(),
                                        error = %e,
                                        "Failed to store provider error metadata"
                                    );
                                } else {
                                    tracing::info!(
                                        task_id = task_id.as_str(),
                                        category = %meta.category,
                                        retry_after = ?meta.retry_after,
                                        "Stored provider error metadata, will pause task"
                                    );
                                }

                                // Emit provider error event for frontend
                                {
                                    events.emit(
                                        "task:provider_error_paused",
                                        serde_json::json!({
                                            "task_id": task_id.as_str(),
                                            "category": meta.category.to_string(),
                                            "message": meta.message,
                                            "retry_after": meta.retry_after,
                                            "previous_status": meta.previous_status,
                                            "auto_resumable": meta.auto_resumable,
                                        }),
                                    );
                                }

                                // Set global rate limit backpressure so ALL spawns are blocked
                                apply_global_rate_limit_backpressure(
                                    execution_state,
                                    &meta.retry_after,
                                    "task_execution",
                                    context_id,
                                );
                            }
                        }
                    }

                    // Late agent-exit/local-tool diagnostics where the work is actually complete → agent called
                    // execution_complete successfully but exited with signal (code=None).
                    // Override to PendingReview only when a current-attempt, HEAD-matched green
                    // validation cache proves completion. Completed steps alone are not enough:
                    // a failed agent can mark steps done before leaving uncommitted or invalid
                    // working-tree changes behind.
                    let target_status = if target_status == InternalStatus::Failed
                        && matches!(
                            stream_error,
                            Some(
                                StreamError::AgentExit { .. } | StreamError::LocalToolFailed { .. }
                            )
                        ) {
                        let validation_complete =
                            if let Some(episode_entered_at) = episode_entered_at {
                                validated_completion_override(
                                    &current_task,
                                    episode_entered_at,
                                    validation_run_repo,
                                )
                                .await
                            } else {
                                false
                            };

                        if validation_complete {
                            let all_steps_done =
                                all_steps_completed(task_step_repo, &task_id).await;
                            tracing::info!(
                                task_id = task_id.as_str(),
                                all_steps_done,
                                validation_complete,
                                "Late execution diagnostic with current green validation cache — overriding Failed → PendingReview"
                            );
                            InternalStatus::PendingReview
                        } else {
                            target_status
                        }
                    } else {
                        target_status
                    };

                    let transition_service = build_transition_service(
                        Arc::clone(task_repo),
                        Arc::clone(task_dependency_repo),
                        Arc::clone(project_repo),
                        Arc::clone(artifact_repo),
                        Arc::clone(chat_message_repo),
                        Arc::clone(chat_attachment_repo),
                        Arc::clone(conversation_repo),
                        Arc::clone(agent_run_repo),
                        Arc::clone(ideation_session_repo),
                        Arc::clone(activity_event_repo),
                        Arc::clone(message_queue),
                        Arc::clone(running_agent_registry),
                        Arc::clone(exec_state),
                        Arc::clone(memory_event_repo),
                        runtime_support.clone(),
                    );

                    if let Err(transition_err) = transition_service
                        .transition_task(&task_id, target_status)
                        .await
                    {
                        tracing::warn!(
                            task_id = task_id.as_str(),
                            original_error = %error,
                            transition_error = %transition_err,
                            target_status = %target_status,
                            "Worker failed and fallback transition also failed — retrying after 500ms"
                        );
                        // D4: Retry once after 500ms delay
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        // Pre-check: re-fetch task state to avoid double-transition if
                        // auto-complete already resolved the task during the 500ms window.
                        let still_stuck =
                            task_still_needs_execution_recovery(&task_id, task_repo).await;
                        if !still_stuck {
                            tracing::debug!(
                                task_id = task_id.as_str(),
                                "Skipping merge retry — task already transitioned before retry fired"
                            );
                        } else if let Err(retry_err) = transition_service
                            .transition_task(&task_id, target_status)
                            .await
                        {
                            tracing::error!(
                                task_id = task_id.as_str(),
                                original_error = %error,
                                retry_error = %retry_err,
                                target_status = %target_status,
                                "Worker failed and fallback transition retry also failed — task may be stuck"
                            );
                            // Emit event so reconciliation can pick it up
                            {
                                events.emit(
                                    "task:recovery_failed",
                                    serde_json::json!({
                                        "task_id": task_id.as_str(),
                                        "original_error": error,
                                        "transition_error": retry_err.to_string(),
                                        "target_status": target_status.to_string(),
                                    }),
                                );
                            }
                            if let Some(notification_service) = notification_service {
                                notification_service
                                    .record(
                                        TaskPipelineNotificationProducer::task_stuck_notification(
                                            &current_task,
                                            agent_run_id,
                                            format!(
                                                "The automatic recovery transition failed: {retry_err}"
                                            ),
                                        ),
                                    )
                                    .await;
                            }
                        }
                    } else {
                        tracing::warn!(
                            task_id = task_id.as_str(),
                            error = %error,
                            target_status = %target_status,
                            "Worker failed; transitioned task"
                        );
                    }
                }
                Ok(Some(_)) => {}
                Ok(None) => {
                    tracing::warn!(
                        task_id = context_id,
                        error = %error,
                        "Worker failed but task was not found for fallback transition"
                    );
                }
                Err(repo_err) => {
                    tracing::error!(
                        task_id = context_id,
                        error = %error,
                        repo_error = %repo_err,
                        "Worker failed and task lookup failed for fallback transition"
                    );
                }
            }
        } else {
            tracing::warn!(
                task_id = context_id,
                error = %error,
                "Worker failed but no execution_state available for fallback transition"
            );
        }
    }

    // Handle merge auto-completion even on agent error
    if context_type == ChatContextType::Merge {
        // L1 shutdown guard: skip merge auto-complete during clean shutdown.
        // Task stays in Merging so Phase 2 of StartupJobRunner can resume it.
        if let Some(ref exec_state) = execution_state {
            if exec_state.is_shutting_down.load(Ordering::SeqCst) {
                tracing::info!(
                    task_id = context_id,
                    "Shutdown detected — skipping merge error auto-complete; task stays in Merging for auto-recovery"
                );
                let task_id = TaskId::from_string(context_id.to_string());
                if let Ok(Some(task)) = task_repo.get_by_id(&task_id).await {
                    persist_shutdown_interrupted_metadata(task_repo, &task, "merge", Some(error))
                        .await;
                }
                return false;
            }
        }

        // Phase 1.5: Store last_agent_error_context: "merge" for L2 crash recovery.
        // Without this, startup crash recovery (Phase 0.8) cannot identify merge tasks
        // when transitioning Escalated tasks back to PendingMerge. Mirrors the triple-insert
        // pattern used by review and execution escalation paths.
        {
            let task_id = TaskId::from_string(context_id.to_string());
            if let Ok(Some(task)) = task_repo.get_by_id(&task_id).await {
                let mut metadata_obj = task
                    .metadata
                    .as_deref()
                    .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
                    .unwrap_or_else(|| serde_json::json!({}));
                if let Some(obj) = metadata_obj.as_object_mut() {
                    obj.insert("last_agent_error".to_string(), serde_json::json!(error));
                    obj.insert(
                        "last_agent_error_context".to_string(),
                        serde_json::json!("merge"),
                    );
                    obj.insert(
                        "last_agent_error_at".to_string(),
                        serde_json::json!(chrono::Utc::now().to_rfc3339()),
                    );
                }
                let mut updated_task = task.clone();
                updated_task.metadata =
                    Some(serde_json::to_string(&metadata_obj).unwrap_or_default());
                updated_task.touch();
                if let Err(e) = task_repo.update(&updated_task).await {
                    tracing::warn!(
                        task_id = context_id,
                        error = %e,
                        "Failed to store merge last_agent_error metadata"
                    );
                }
            }
        }

        // Check for provider rate limit errors BEFORE attempting auto-complete.
        // If rate-limited, store retry_after in MergeRecoveryMetadata so the reconciler
        // can skip retries until the limit clears (without burning retry budget).
        let is_rate_limited = if let Some(se) = stream_error {
            if se.is_provider_error() {
                let task_id = TaskId::from_string(context_id.to_string());
                if let Ok(Some(task)) = task_repo.get_by_id(&task_id).await {
                    let retry_after = match se {
                        StreamError::ProviderError { retry_after, .. } => retry_after.clone(),
                        _ => None,
                    };

                    let mut recovery =
                        MergeRecoveryMetadata::from_task_metadata(task.metadata.as_deref())
                            .unwrap_or(None)
                            .unwrap_or_default();

                    recovery.rate_limit_retry_after = retry_after.clone();
                    recovery.append_event_with_state(
                        MergeRecoveryEvent::new(
                            MergeRecoveryEventKind::AttemptFailed,
                            MergeRecoverySource::System,
                            MergeRecoveryReasonCode::ProviderRateLimited,
                            format!("Merge agent hit provider rate limit: {}", error),
                        )
                        .with_failure_source(MergeFailureSource::RateLimited),
                        MergeRecoveryState::RateLimited,
                    );

                    let mut updated_task = task.clone();
                    match recovery.update_task_metadata(updated_task.metadata.as_deref()) {
                        Ok(metadata_json) => {
                            updated_task.metadata = Some(metadata_json);
                            updated_task.touch();
                            if let Err(e) = task_repo.update(&updated_task).await {
                                tracing::error!(
                                    task_id = context_id,
                                    error = %e,
                                    "Failed to store merge rate limit metadata"
                                );
                            } else {
                                tracing::info!(
                                    task_id = context_id,
                                    retry_after = ?retry_after,
                                    "Stored rate limit in merge recovery metadata"
                                );
                            }
                        }
                        Err(e) => {
                            tracing::error!(
                                task_id = context_id,
                                error = %e,
                                "Failed to serialize merge rate limit metadata"
                            );
                        }
                    }

                    // Set global rate limit backpressure so ALL spawns are blocked
                    apply_global_rate_limit_backpressure(
                        execution_state,
                        &retry_after,
                        "merge",
                        context_id,
                    );

                    true
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };

        // Only attempt merge auto-complete if NOT rate limited
        // (rate-limited merges should wait for reconciler to retry after cooldown)
        if !is_rate_limited {
            if let Some(ref exec_state) = execution_state {
                let merge_ctx = super::chat_service_merge::MergeAutoCompleteContext {
                    task_id_str: context_id,
                    task_id: TaskId::from_string(context_id.to_string()),
                    task_repo,
                    task_dependency_repo,
                    project_repo,
                    artifact_repo,
                    chat_message_repo,
                    chat_attachment_repo,
                    conversation_repo,
                    agent_run_repo,
                    ideation_session_repo,
                    activity_event_repo,
                    message_queue,
                    running_agent_registry,
                    memory_event_repo,
                    execution_state: exec_state,
                    execution_settings_repo: execution_settings_repo.as_ref(),
                    plan_branch_repo,
                    events: &events,
                    runtime_factory_deps,
                    interactive_process_registry,
                };
                super::chat_service_merge::attempt_merge_auto_complete(&merge_ctx).await;
            } else {
                tracing::warn!(
                    "Cannot auto-complete merge for task {} on error - no execution_state available",
                    context_id
                );
            }
        }
    }

    // Handle review agent errors — transition stuck Reviewing tasks to Escalated
    if context_type == ChatContextType::Review {
        if let Some(ref exec_state) = execution_state {
            let task_id = TaskId::from_string(context_id.to_string());
            match task_repo.get_by_id(&task_id).await {
                Ok(Some(task)) if task.internal_status == InternalStatus::Reviewing => {
                    // L1 shutdown guard: skip escalation during clean app shutdown.
                    // The task stays in Reviewing so StartupJobRunner Phase 2 can respawn it.
                    if exec_state.is_shutting_down.load(Ordering::SeqCst) {
                        tracing::info!(
                            task_id = task_id.as_str(),
                            "Shutdown detected — skipping review error escalation; task stays in Reviewing for auto-recovery"
                        );
                        persist_shutdown_interrupted_metadata(
                            task_repo,
                            &task,
                            "review",
                            Some(error),
                        )
                        .await;
                        return false;
                    }

                    // Store last_agent_error in metadata for UI visibility
                    let mut metadata_obj = task
                        .metadata
                        .as_deref()
                        .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
                        .unwrap_or_else(|| serde_json::json!({}));
                    if let Some(obj) = metadata_obj.as_object_mut() {
                        obj.insert("last_agent_error".to_string(), serde_json::json!(error));
                        obj.insert(
                            "last_agent_error_context".to_string(),
                            serde_json::json!("review"),
                        );
                        obj.insert(
                            "last_agent_error_at".to_string(),
                            serde_json::json!(chrono::Utc::now().to_rfc3339()),
                        );
                    }
                    let mut updated_task = task.clone();
                    updated_task.metadata =
                        Some(serde_json::to_string(&metadata_obj).unwrap_or_default());
                    updated_task.touch();
                    let _ = task_repo.update(&updated_task).await;

                    // If this is a provider error, set global backpressure
                    if let Some(se) = stream_error {
                        if se.is_provider_error() {
                            let retry_after = match se {
                                StreamError::ProviderError { retry_after, .. } => {
                                    retry_after.clone()
                                }
                                _ => None,
                            };
                            apply_global_rate_limit_backpressure(
                                execution_state,
                                &retry_after,
                                "review",
                                context_id,
                            );
                        }
                    }

                    // Store a ReviewNote so the frontend can display why the task was escalated.
                    if let Some(ref repo) = review_repo {
                        let reason = format!("Review agent crashed: {}", error);
                        let note = ReviewNote::with_notes(
                            task_id.clone(),
                            ReviewerType::System,
                            ReviewOutcome::Rejected,
                            reason,
                        );
                        if let Err(e) = repo.add_note(&note).await {
                            tracing::warn!(
                                task_id = task_id.as_str(),
                                error = %e,
                                "Failed to store escalation ReviewNote after agent error"
                            );
                        }
                    }

                    // Transition to Escalated
                    let transition_service = build_transition_service(
                        Arc::clone(task_repo),
                        Arc::clone(task_dependency_repo),
                        Arc::clone(project_repo),
                        Arc::clone(artifact_repo),
                        Arc::clone(chat_message_repo),
                        Arc::clone(chat_attachment_repo),
                        Arc::clone(conversation_repo),
                        Arc::clone(agent_run_repo),
                        Arc::clone(ideation_session_repo),
                        Arc::clone(activity_event_repo),
                        Arc::clone(message_queue),
                        Arc::clone(running_agent_registry),
                        Arc::clone(exec_state),
                        Arc::clone(memory_event_repo),
                        runtime_support.clone(),
                    );

                    if let Err(e) = transition_service
                        .transition_task(&task_id, InternalStatus::Escalated)
                        .await
                    {
                        tracing::error!(
                            task_id = task_id.as_str(),
                            error = %e,
                            "Failed to transition reviewing task to Escalated after agent error"
                        );
                    } else {
                        tracing::warn!(
                            task_id = task_id.as_str(),
                            error = %error,
                            "Review agent failed; transitioned task to Escalated"
                        );
                    }
                }
                Ok(Some(_)) => {
                    // Task not in Reviewing — already transitioned, no action needed
                }
                Ok(None) => {
                    tracing::warn!(
                        task_id = context_id,
                        error = %error,
                        "Review agent failed but task was not found for fallback transition"
                    );
                }
                Err(repo_err) => {
                    tracing::error!(
                        task_id = context_id,
                        error = %error,
                        repo_error = %repo_err,
                        "Review agent failed and task lookup failed for fallback transition"
                    );
                }
            }
        } else {
            tracing::warn!(
                task_id = context_id,
                error = %error,
                "Review agent failed but no execution_state available for fallback transition"
            );
        }
    }

    // Emit error event AFTER all state transitions are complete so the UI reflects
    // the final task state (Failed/Escalated/etc.) rather than showing idle while
    // the backend is still processing the error state change.
    {
        let _ = emit_serialized(
            events.as_ref(),
            "agent:error",
            &AgentErrorPayload {
                conversation_id: Some(conversation_id.as_str().to_string()),
                context_type: context_type.to_string(),
                context_id: context_id.to_string(),
                agent_run_id: Some(agent_run_id.to_string()),
                error: redacted_error.clone(),
                stderr: Some(redacted_error.clone()),
            },
        );
    }

    false // Normal error handling performed, no retry spawned
}

/// Pre-fetched state of the parent verification session, used by Gate B+C in
/// `handle_stream_error` to determine whether to suppress `agent:error` emission.
struct ParentVerificationState {
    /// Parent ideation session ID
    parent_id: IdeationSessionId,
    /// Active conversation ID for the parent session (used for dedup check)
    parent_conversation_id: ChatConversationId,
    /// Whether a verification loop is currently running on the parent
    in_progress: bool,
    /// The terminal verification status reached by the parent session
    terminal_status: VerificationStatus,
    /// Convergence reason from the parent's verification metadata (if any)
    convergence_reason: Option<String>,
    /// Current gaps from the parent's verification metadata (if any)
    current_gaps: Vec<VerificationGap>,
}

/// Returns `true` if `context_id` is an ideation session with `session_purpose == Verification`.
///
/// Used as the first gate in the B+C suppression check. Returns `false` on any DB error
/// so that the normal `agent:error` path remains the safe default.
pub(crate) async fn is_verification_child(
    context_id: &str,
    ideation_session_repo: &Arc<dyn IdeationSessionRepository>,
) -> bool {
    let session_id = IdeationSessionId::from_string(context_id.to_string());
    match ideation_session_repo.get_by_id(&session_id).await {
        Ok(Some(session)) => session.session_purpose == SessionPurpose::Verification,
        Ok(None) => false,
        Err(e) => {
            tracing::warn!(
                context_id,
                error = %e,
                "Gate B: failed to fetch session for verification child check"
            );
            false
        }
    }
}

/// Fetches the verification state of the parent session for a verification child.
///
/// Returns `None` if:
/// - The child session has no `parent_session_id`
/// - The parent session cannot be found
/// - Any DB error occurs (safe fallthrough to normal `agent:error`)
///
/// Returns `Some(ParentVerificationState)` with the parent's current verification state.
async fn fetch_parent_verification_state(
    child_context_id: &str,
    ideation_session_repo: &Arc<dyn IdeationSessionRepository>,
    conversation_repo: &Arc<dyn ChatConversationRepository>,
) -> Option<ParentVerificationState> {
    let child_id = IdeationSessionId::from_string(child_context_id.to_string());

    // Load the child session to get parent_session_id
    let child_session = match ideation_session_repo.get_by_id(&child_id).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            tracing::warn!(
                context_id = child_context_id,
                "Gate B: child session not found"
            );
            return None;
        }
        Err(e) => {
            tracing::warn!(
                context_id = child_context_id,
                error = %e,
                "Gate B: failed to fetch child session"
            );
            return None;
        }
    };

    let parent_session_id = child_session.parent_session_id?;

    // Load the parent session
    let parent_session = match ideation_session_repo.get_by_id(&parent_session_id).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            tracing::warn!(
                parent_id = %parent_session_id.as_str(),
                "Gate B: parent session not found"
            );
            return None;
        }
        Err(e) => {
            tracing::warn!(
                parent_id = %parent_session_id.as_str(),
                error = %e,
                "Gate B: failed to fetch parent session"
            );
            return None;
        }
    };

    // Fetch or create the parent's active conversation ID for the dedup check
    let parent_conversation_id = match conversation_repo
        .get_active_for_context(ChatContextType::Ideation, parent_session_id.as_str())
        .await
    {
        Ok(Some(conv)) => conv.id,
        Ok(None) => {
            tracing::debug!(
                parent_id = %parent_session_id.as_str(),
                "Gate B: no active conversation for parent session — dedup check will find nothing"
            );
            // Use a fresh random ID as sentinel — dedup check will find nothing, injection proceeds
            ChatConversationId::new()
        }
        Err(e) => {
            tracing::warn!(
                parent_id = %parent_session_id.as_str(),
                error = %e,
                "Gate B: failed to fetch parent conversation"
            );
            return None;
        }
    };

    let (terminal_status, in_progress, convergence_reason, current_gaps) =
        match ideation_session_repo
            .get_verification_run_snapshot(
                &parent_session_id,
                parent_session.verification_generation,
            )
            .await
        {
            Ok(Some(snapshot)) => (
                snapshot.status,
                snapshot.in_progress,
                snapshot.convergence_reason.clone(),
                snapshot.current_gaps.clone(),
            ),
            Ok(None) => (
                parent_session.verification_status,
                parent_session.verification_in_progress,
                parent_session.verification_convergence_reason.clone(),
                vec![],
            ),
            Err(e) => {
                tracing::warn!(
                    parent_id = %parent_session_id.as_str(),
                    error = %e,
                    "Gate B: failed to fetch native verification snapshot"
                );
                (
                    parent_session.verification_status,
                    parent_session.verification_in_progress,
                    parent_session.verification_convergence_reason.clone(),
                    vec![],
                )
            }
        };

    Some(ParentVerificationState {
        parent_id: parent_session_id,
        parent_conversation_id,
        in_progress,
        terminal_status,
        convergence_reason,
        current_gaps,
    })
}

#[cfg(test)]
#[path = "chat_service_handlers_tests.rs"]
mod tests;
