// Success/error handler logic for background send processing.
//
// Extracted from chat_service_send_background.rs to reduce file size.
// Contains:
// - handle_stream_success: task transitions (TaskExecution → PendingReview/Failed)
//   and merge auto-completion after successful stream processing
// - handle_stream_error: error classification, stale session recovery retry,
//   agent run failure recording, message finalization, and fallback task transitions

use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::application::question_state::QuestionState;
use crate::application::runtime_factory::{
    build_task_scheduler_with_fallback, build_transition_service_with_fallback, RuntimeFactoryDeps,
};
use crate::application::task_scheduler_service::TaskSchedulerService;
use crate::application::task_transition_service::TaskTransitionService;
use crate::application::AppState;
use crate::application::InteractiveProcessRegistry;
use crate::commands::{execution_commands::AGENT_ACTIVE_STATUSES, ExecutionState};
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{
    app_state::ExecutionHaltMode, AgentRunId, ChatContextType, ChatConversation,
    ChatConversationId, ChatMessageId, IdeationSessionId, InternalStatus, MergeFailureSource,
    MergeRecoveryEvent, MergeRecoveryEventKind, MergeRecoveryMetadata, MergeRecoveryReasonCode,
    MergeRecoverySource, MergeRecoveryState, ReviewNote, ReviewOutcome, ReviewerType,
    SessionPurpose, TaskId, TaskStepStatus, VerificationGap, VerificationStatus,
};
use crate::domain::repositories::{
    ActivityEventRepository, AgentRunRepository, ArtifactRepository, ChatAttachmentRepository,
    ChatConversationRepository, ChatMessageRepository, ExecutionSettingsRepository,
    IdeationSessionRepository, MemoryEventRepository, PlanBranchRepository, ProjectRepository,
    ReviewRepository, TaskDependencyRepository, TaskProposalRepository, TaskRepository,
    TaskStepRepository,
};
use crate::domain::services::{MessageQueue, RunningAgentRegistry};
use crate::domain::state_machine::services::TaskScheduler;
use crate::error::AppError;
use crate::infrastructure::agents::claude::{ContentBlockItem, ToolCall};

use super::chat_service_context;
use super::chat_service_errors::{
    classify_agent_error, is_nonfatal_mcp_tool_cancellation, StreamError,
};
use super::chat_service_helpers::get_assistant_role;
use super::chat_service_types::{AgentErrorPayload, AgentRunCompletedPayload};
use super::EventContextPayload;
use crate::application::reconciliation::verification_handoff;
use crate::application::reconciliation::verification_reconciliation::ReconcileVerificationChildCompletion;
use crate::utils::secret_redactor::redact;

fn should_requeue_after_provider_pause(context_type: ChatContextType) -> bool {
    matches!(
        context_type,
        ChatContextType::Ideation | ChatContextType::Task | ChatContextType::Project
    )
}

const VERIFICATION_AUTO_CONTINUE_METADATA: &str = r#"{"resume_in_place":true}"#;

fn queue_verification_auto_continue(
    message_queue: &Arc<MessageQueue>,
    child_id: &IdeationSessionId,
    continuation_message: String,
) {
    let mut queued = crate::domain::services::QueuedMessage::new(continuation_message);
    queued.metadata_override = Some(VERIFICATION_AUTO_CONTINUE_METADATA.to_string());
    message_queue.queue_front_existing(ChatContextType::Ideation, child_id.as_str(), queued);
}

async fn handle_verification_child_completion<R: Runtime>(
    child_id: &IdeationSessionId,
    parent_id: &IdeationSessionId,
    ideation_session_repo: &Arc<dyn IdeationSessionRepository>,
    conversation_repo: &Arc<dyn ChatConversationRepository>,
    chat_message_repo: &Arc<dyn ChatMessageRepository>,
    message_queue: &Arc<MessageQueue>,
    app_handle: &Option<AppHandle<R>>,
    verification_child_registry: &Option<
        Arc<super::verification_child_process_registry::VerificationChildProcessRegistry>,
    >,
) {
    let reconcile_result = crate::application::reconciliation::verification_reconciliation::reconcile_verification_on_child_complete(
        parent_id,
        child_id,
        ideation_session_repo,
        app_handle.as_ref(),
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
            queue_verification_auto_continue(
                message_queue,
                child_id,
                request.continuation_message,
            );
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

/// Parse an ISO 8601 retry_after string and set the global provider rate limit gate
/// on ExecutionState. Called from all agent error contexts (TaskExecution, Merge, Review)
/// so a single rate limit detection blocks ALL subsequent spawns.
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

fn should_transition_task_execution_to_pending_review(
    has_output: bool,
    steps_tracked: bool,
    all_steps_done: bool,
) -> bool {
    if steps_tracked {
        all_steps_done
    } else {
        has_output
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExecutionCompletionAction {
    PendingReview,
    Failed,
}

fn execution_completion_action(
    has_output: bool,
    steps_tracked: bool,
    all_steps_done: bool,
) -> ExecutionCompletionAction {
    if should_transition_task_execution_to_pending_review(has_output, steps_tracked, all_steps_done)
    {
        ExecutionCompletionAction::PendingReview
    } else {
        ExecutionCompletionAction::Failed
    }
}

fn build_transition_service<R: Runtime>(
    app_handle: &Option<AppHandle<R>>,
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
) -> TaskTransitionService<R> {
    let deps = build_runtime_factory_deps(
        app_handle,
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
        None,
        None,
        None,
    );
    build_transition_service_with_fallback(app_handle, execution_state, &deps)
}

#[allow(clippy::too_many_arguments)]
fn build_task_scheduler_service<R: Runtime>(
    app_handle: &Option<AppHandle<R>>,
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
    execution_settings_repo: Option<Arc<dyn ExecutionSettingsRepository>>,
    plan_branch_repo: Option<Arc<dyn PlanBranchRepository>>,
    interactive_process_registry: Option<Arc<InteractiveProcessRegistry>>,
) -> TaskSchedulerService<R> {
    let deps = build_runtime_factory_deps(
        app_handle,
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
        execution_settings_repo,
        plan_branch_repo,
        interactive_process_registry,
    );
    build_task_scheduler_with_fallback(app_handle, execution_state, &deps)
}

#[allow(clippy::too_many_arguments)]
fn build_runtime_factory_deps<R: Runtime>(
    app_handle: &Option<AppHandle<R>>,
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
    execution_settings_repo: Option<Arc<dyn ExecutionSettingsRepository>>,
    plan_branch_repo: Option<Arc<dyn PlanBranchRepository>>,
    interactive_process_registry: Option<Arc<InteractiveProcessRegistry>>,
) -> RuntimeFactoryDeps {
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
    .with_runtime_support(
        execution_settings_repo,
        app_handle
            .as_ref()
            .and_then(|handle| handle.try_state::<AppState>())
            .map(|app_state| Arc::clone(&app_state.agent_lane_settings_repo)),
        app_handle
            .as_ref()
            .and_then(|handle| handle.try_state::<AppState>())
            .map(|app_state| Arc::clone(&app_state.agent_provider_settings_repo)),
        plan_branch_repo,
        interactive_process_registry,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_recovery_retry_background_context<R: Runtime>(
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
    agent_lane_settings_repo: &Option<
        Arc<dyn crate::domain::repositories::AgentLaneSettingsRepository>,
    >,
    ideation_effort_settings_repo: &Option<
        Arc<dyn crate::domain::repositories::IdeationEffortSettingsRepository>,
    >,
    ideation_model_settings_repo: &Option<
        Arc<dyn crate::domain::repositories::IdeationModelSettingsRepository>,
    >,
    task_proposal_repo: &Option<Arc<dyn TaskProposalRepository>>,
    activity_event_repo: &Arc<dyn ActivityEventRepository>,
    memory_event_repo: &Arc<dyn MemoryEventRepository>,
    message_queue: &Arc<MessageQueue>,
    running_agent_registry: &Arc<dyn RunningAgentRegistry>,
    execution_state: &Option<Arc<ExecutionState>>,
    question_state: &Option<Arc<QuestionState>>,
    plan_branch_repo: &Option<Arc<dyn PlanBranchRepository>>,
    app_handle: &Option<AppHandle<R>>,
    run_chain_id: Option<String>,
    user_message_content: Option<&str>,
    retry_conv: ChatConversation,
    agent_name: Option<&str>,
    team_mode: bool,
    review_repo: &Option<Arc<dyn ReviewRepository>>,
    task_step_repo: &Option<Arc<dyn TaskStepRepository>>,
    interactive_process_registry: &Option<Arc<InteractiveProcessRegistry>>,
    verification_child_registry: &Option<
        Arc<super::verification_child_process_registry::VerificationChildProcessRegistry>,
    >,
) -> super::chat_service_send_background::BackgroundRunContext<R> {
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
            ideation_effort_settings_repo: ideation_effort_settings_repo.clone(),
            ideation_model_settings_repo: ideation_model_settings_repo.clone(),
            agent_conversation_workspace_repo: None,
            task_proposal_repo: task_proposal_repo.clone(),
            activity_event_repo: Arc::clone(activity_event_repo),
            memory_event_repo: Arc::clone(memory_event_repo),
            message_queue: Arc::clone(message_queue),
            running_agent_registry: Arc::clone(running_agent_registry),
            task_step_repo: task_step_repo.clone(),
            review_repo: review_repo.clone(),
        },
        execution_state: execution_state.clone(),
        question_state: question_state.clone(),
        plan_branch_repo: plan_branch_repo.clone(),
        app_handle: app_handle.clone(),
        run_chain_id,
        is_retry_attempt: true,
        user_message_content: user_message_content.map(str::to_string),
        conversation: Some(retry_conv),
        agent_name: agent_name.map(str::to_string),
        team_mode,
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
        team_service: None,
        streaming_state_cache: super::StreamingStateCache::new(),
        interactive_process_registry: interactive_process_registry.clone(),
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

pub(super) async fn apply_system_wide_provider_pause<R: Runtime>(
    app_handle: &Option<AppHandle<R>>,
    category: &super::ProviderErrorCategory,
    message: &str,
    retry_after: &Option<String>,
    source_context: &str,
    source_context_id: &str,
) {
    let Some(handle) = app_handle else {
        return;
    };

    let app_state = handle.state::<AppState>();
    let execution_state = handle.state::<Arc<ExecutionState>>();

    execution_state.pause();
    apply_global_rate_limit_backpressure(
        &Some(Arc::clone(execution_state.inner())),
        retry_after,
        source_context,
        source_context_id,
    );

    if let Err(error) = app_state
        .app_state_repo
        .set_execution_halt_mode(ExecutionHaltMode::Paused)
        .await
    {
        tracing::warn!(error = %error, "Failed to persist provider-triggered global pause");
    }

    app_state.running_agent_registry.stop_all().await;
    app_state.interactive_process_registry.clear().await;

    let transition_service = app_state
        .build_transition_service_with_execution_state(Arc::clone(execution_state.inner()));

    let paused_at = chrono::Utc::now().to_rfc3339();
    let projects = match app_state.project_repo.get_all().await {
        Ok(projects) => projects,
        Err(error) => {
            tracing::error!(error = %error, "Failed to load projects for provider-triggered pause");
            return;
        }
    };

    for project in projects {
        let tasks = match app_state.task_repo.get_by_project(&project.id).await {
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

            let pause_reason = super::PauseReason::ProviderError {
                category: category.clone(),
                message: message.to_string(),
                retry_after: retry_after.clone(),
                previous_status: task.internal_status.to_string(),
                paused_at: paused_at.clone(),
                auto_resumable: true,
                resume_attempts: 0,
            };

            let mut updated_task = task.clone();
            updated_task.metadata =
                Some(pause_reason.write_to_task_metadata(updated_task.metadata.as_deref()));
            updated_task.touch();
            let _ = app_state.task_repo.update(&updated_task).await;

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

    let _ = handle.emit(
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

fn seal_unresolved_tool_calls_json(tool_calls_json: Option<String>, reason: &str) -> Option<String> {
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

async fn finalize_assistant_message_with_terminal_tool_state<R: Runtime>(
    chat_message_repo: &Arc<dyn ChatMessageRepository>,
    app_handle: Option<&AppHandle<R>>,
    event_ctx: &EventContextPayload,
    message_id: &str,
    role: &str,
    content: &str,
    tool_calls_json: Option<String>,
    content_blocks_json: Option<String>,
    reason: &str,
) {
    let sealed_tool_calls = seal_unresolved_tool_calls_json(tool_calls_json, reason);
    let sealed_content_blocks = seal_unresolved_content_blocks_json(content_blocks_json, reason);
    super::chat_service_send_background::finalize_assistant_message(
        chat_message_repo,
        app_handle,
        event_ctx,
        message_id,
        role,
        content,
        sealed_tool_calls.as_deref(),
        sealed_content_blocks.as_deref(),
    )
    .await;
}

/// Handle successful stream completion: task state transitions and merge auto-completion.
///
/// For TaskExecution context:
/// - If all task steps are completed → transition to PendingReview
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

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_stream_success<R: Runtime>(
    agent_run_id: &str,
    context_type: ChatContextType,
    context_id: &str,
    has_output: bool,
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
    execution_settings_repo: &Option<Arc<dyn ExecutionSettingsRepository>>,
    app_handle: &Option<AppHandle<R>>,
    interactive_process_registry: &Option<Arc<InteractiveProcessRegistry>>,
    review_repo: &Option<Arc<dyn ReviewRepository>>,
    verification_child_registry: &Option<
        Arc<super::verification_child_process_registry::VerificationChildProcessRegistry>,
    >,
) {
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
                        persist_shutdown_interrupted_metadata(
                            task_repo,
                            &task,
                            "execution",
                            None,
                        )
                        .await;
                        return;
                    }

                    if !task_execution_attempt_matches_current_status(
                        &task_id,
                        agent_run_id,
                        task_repo,
                        agent_run_repo,
                    )
                    .await
                    {
                        tracing::info!(
                            task_id = task_id.as_str(),
                            agent_run_id,
                            "Skipping stale task-execution completion for an older attempt"
                        );
                        return;
                    }

                    // Create scheduler for auto-scheduling next Ready task
                    let scheduler_svc = build_task_scheduler_service(
                        app_handle,
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
                        execution_settings_repo.clone(),
                        plan_branch_repo.clone(),
                        interactive_process_registry.clone(),
                    );
                    let scheduler_concrete = Arc::new(scheduler_svc);
                    scheduler_concrete
                        .set_self_ref(Arc::clone(&scheduler_concrete) as Arc<dyn TaskScheduler>);
                    let task_scheduler: Arc<dyn TaskScheduler> = scheduler_concrete;

                    let transition_service = build_transition_service(
                        app_handle,
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
                    )
                    .with_task_scheduler(task_scheduler);
                    let transition_service = if let Some(ref repo) = execution_settings_repo {
                        transition_service.with_execution_settings_repo(Arc::clone(repo))
                    } else {
                        transition_service
                    };
                    let transition_service = if let Some(ref repo) = plan_branch_repo {
                        transition_service.with_plan_branch_repo(Arc::clone(repo))
                    } else {
                        transition_service
                    };
                    let transition_service = if let Some(ref ipr) = interactive_process_registry {
                        transition_service.with_interactive_process_registry(Arc::clone(ipr))
                    } else {
                        transition_service
                    };
                    let all_steps_done = all_steps_completed(task_step_repo, &task_id).await;
                    let completion_action = execution_completion_action(
                        has_output,
                        task_step_repo.is_some(),
                        all_steps_done,
                    );

                    if completion_action == ExecutionCompletionAction::PendingReview
                        && all_steps_done
                    {
                        tracing::info!(
                                task_id = task_id.as_str(),
                                "Worker run ended with all steps completed; transitioning to PendingReview"
                            );
                        if let Err(e) = transition_service
                            .transition_task(&task_id, InternalStatus::PendingReview)
                            .await
                        {
                            tracing::error!(
                                "Failed to transition all-steps-done task {} to PendingReview: {}",
                                task_id.as_str(),
                                e
                            );
                        }
                    } else if completion_action == ExecutionCompletionAction::PendingReview {
                        if let Err(e) = transition_service
                            .transition_task(&task_id, InternalStatus::PendingReview)
                            .await
                        {
                            tracing::error!(
                                "Failed to transition task {} to PendingReview: {}",
                                task_id.as_str(),
                                e
                            );
                        }
                    } else {
                        let Some(current_task) = load_current_task_execution_attempt(
                            &task_id,
                            agent_run_id,
                            task_repo,
                            agent_run_repo,
                        )
                        .await
                        else {
                            tracing::info!(
                                task_id = task_id.as_str(),
                                agent_run_id,
                                "Skipping stale incomplete execution finalizer; task is no longer in the same execution attempt"
                            );
                            return;
                        };

                        // Store last_agent_error for empty-output failure
                        let mut metadata_obj = current_task
                            .metadata
                            .as_deref()
                            .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
                            .unwrap_or_else(|| serde_json::json!({}));
                        if let Some(obj) = metadata_obj.as_object_mut() {
                            obj.insert(
                                "last_agent_error".to_string(),
                                serde_json::json!("Agent ended without completing all task steps"),
                            );
                            obj.insert(
                                "last_agent_error_context".to_string(),
                                serde_json::json!("execution"),
                            );
                            obj.insert(
                                "last_agent_error_at".to_string(),
                                serde_json::json!(chrono::Utc::now().to_rfc3339()),
                            );
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
                            app_handle,
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
                        );
                        let transition_service = if let Some(ref repo) = execution_settings_repo {
                            transition_service.with_execution_settings_repo(Arc::clone(repo))
                        } else {
                            transition_service
                        };
                        let transition_service = if let Some(ref repo) = plan_branch_repo {
                            transition_service.with_plan_branch_repo(Arc::clone(repo))
                        } else {
                            transition_service
                        };
                        let transition_service = if let Some(ref ipr) = interactive_process_registry
                        {
                            transition_service.with_interactive_process_registry(Arc::clone(ipr))
                        } else {
                            transition_service
                        };

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
                app_handle: app_handle.as_ref(),
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
                            app_handle,
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

async fn task_execution_attempt_matches_current_status(
    task_id: &TaskId,
    agent_run_id: &str,
    task_repo: &Arc<dyn TaskRepository>,
    agent_run_repo: &Arc<dyn AgentRunRepository>,
) -> bool {
    let task = match task_repo.get_by_id(task_id).await {
        Ok(Some(task)) => task,
        Ok(None) => return false,
        Err(_) => return true,
    };

    if !matches!(
        task.internal_status,
        InternalStatus::Executing | InternalStatus::ReExecuting
    ) {
        return false;
    }

    let Some(status_entered_at) = task_repo
        .get_status_entered_at(task_id, task.internal_status)
        .await
        .ok()
        .flatten()
    else {
        return true;
    };

    let Some(agent_run) = agent_run_repo
        .get_by_id(&AgentRunId::from_string(agent_run_id))
        .await
        .ok()
        .flatten()
    else {
        return true;
    };

    agent_run.started_at + chrono::Duration::seconds(1) >= status_entered_at
}

async fn load_current_task_execution_attempt(
    task_id: &TaskId,
    agent_run_id: &str,
    task_repo: &Arc<dyn TaskRepository>,
    agent_run_repo: &Arc<dyn AgentRunRepository>,
) -> Option<crate::domain::entities::Task> {
    let task = match task_repo.get_by_id(task_id).await {
        Ok(Some(task)) => task,
        Ok(None) | Err(_) => return None,
    };

    if !matches!(
        task.internal_status,
        InternalStatus::Executing | InternalStatus::ReExecuting
    ) {
        return None;
    }

    let Some(status_entered_at) = task_repo
        .get_status_entered_at(task_id, task.internal_status)
        .await
        .ok()
        .flatten()
    else {
        return Some(task);
    };

    let Some(agent_run) = agent_run_repo
        .get_by_id(&AgentRunId::from_string(agent_run_id))
        .await
        .ok()
        .flatten()
    else {
        return Some(task);
    };

    if agent_run.started_at + chrono::Duration::seconds(1) >= status_entered_at {
        Some(task)
    } else {
        None
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
pub(super) async fn handle_stream_error<R: Runtime + 'static>(
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
    user_message_content: Option<&str>,
    conversation: Option<&ChatConversation>,
    resolved_project_id: Option<String>,
    cli_path: &Path,
    plugin_dir: &Path,
    working_directory: &Path,
    chat_message_repo: &Arc<dyn ChatMessageRepository>,
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
    app_handle: &Option<AppHandle<R>>,
    agent_name: Option<&str>,
    team_mode: bool,
    run_chain_id: Option<String>,
    interactive_process_registry: &Option<Arc<InteractiveProcessRegistry>>,
    review_repo: &Option<Arc<dyn ReviewRepository>>,
    task_step_repo: &Option<Arc<dyn TaskStepRepository>>,
    verification_child_registry: &Option<
        Arc<super::verification_child_process_registry::VerificationChildProcessRegistry>,
    >,
) -> bool {
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
                true,  // effective_has_output: turns were finalized → agent produced output
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
                execution_settings_repo,
                app_handle,
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
            if let Some(ref handle) = app_handle {
                let _ = handle.emit(
                    "agent:run_completed",
                    AgentRunCompletedPayload::with_provider_session(
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
                execution_settings_repo,
                app_handle,
                interactive_process_registry,
                review_repo,
                verification_child_registry,
            )
            .await;

            // Emit run_completed to reset frontend from "generating" → "idle".
            if let Some(ref handle) = app_handle {
                let _ = handle.emit(
                    "agent:run_completed",
                    AgentRunCompletedPayload::with_provider_session(
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

        // turns_finalized == 0 && !completion_tool_called: genuine user-initiated stop or
        // system cancel before completion.
        tracing::info!(
            conversation_id = conversation_id.as_str(),
            context_type = %context_type,
            context_id,
            "Stream cancelled — skipping error recovery and fallback transitions"
        );
        let _ = agent_run_repo
            .fail(
                &AgentRunId::from_string(agent_run_id),
                "Agent stopped by user",
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
            app_handle.as_ref(),
            event_ctx,
            pre_assistant_msg_id,
            &get_assistant_role(&context_type).to_string(),
            &stop_note,
            existing_tool_calls,
            existing_content_blocks,
            "stopped",
        )
        .await;

        if let Some(ref handle) = app_handle {
            let _ = handle.emit(
                "agent:stopped",
                serde_json::json!({
                    "conversation_id": conversation_id.as_str(),
                    "agent_run_id": agent_run_id,
                    "context_type": context_type.to_string(),
                    "context_id": context_id,
                }),
            );
        }

        // Path C: Reset verification state when a verification child is stopped by user
        if context_type == ChatContextType::Ideation {
            let child_id = IdeationSessionId::from_string(context_id.to_string());
            crate::application::reconciliation::verification_reconciliation::reset_verification_on_child_error(
                &child_id,
                ideation_session_repo,
                app_handle.as_ref(),
                "user_stopped",
            )
            .await;
        }

        return false;
    }

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
                    team_mode,
                    Arc::clone(chat_message_repo),
                    Arc::clone(conversation_repo),
                    Arc::clone(chat_attachment_repo),
                    Arc::clone(artifact_repo),
                    Some(Arc::clone(ideation_session_repo)),
                    task_proposal_repo.clone(),
                    &session_id,
                    app_handle.as_ref(),
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
                        if let Some(ref handle) = app_handle {
                            let _ = handle.emit(
                                "agent:session_recovered",
                                serde_json::json!({
                                    "conversation_id": conversation_id.as_str(),
                                    "message": "Session restored from local history"
                                }),
                            );
                        }

                        // Retry send with fresh session (set is_retry=true)
                        let mut retry_conv = conv.clone();
                        retry_conv.set_provider_session_ref(
                            crate::domain::agents::ProviderSessionRef {
                                harness: recovery_harness,
                                provider_session_id: new_session_id.clone(),
                            },
                        );
                        let ideation_effort_settings_repo = app_handle.as_ref().map(|handle| {
                            let app_state = handle.state::<AppState>();
                            Arc::clone(&app_state.ideation_effort_settings_repo)
                        });
                        let agent_lane_settings_repo = app_handle.as_ref().map(|handle| {
                            let app_state = handle.state::<AppState>();
                            Arc::clone(&app_state.agent_lane_settings_repo)
                        });
                        let ideation_model_settings_repo = app_handle.as_ref().map(|handle| {
                            let app_state = handle.state::<AppState>();
                            Arc::clone(&app_state.ideation_model_settings_repo)
                        });
                        let delegated_session_repo = app_handle.as_ref().map(|handle| {
                            let app_state = handle.state::<AppState>();
                            Arc::clone(&app_state.delegated_session_repo)
                        });

                        let retry_spawnable =
                            chat_service_context::build_resume_command_for_harness(
                                recovery_harness,
                                cli_path,
                                plugin_dir,
                                context_type,
                                context_id,
                                msg,
                                working_directory,
                                &new_session_id,
                                resolved_project_id.as_deref(),
                                if context_type == ChatContextType::Project {
                                    Some(conversation_id.as_str())
                                } else {
                                    None
                                },
                                team_mode,
                                Arc::clone(chat_attachment_repo),
                                Arc::clone(artifact_repo),
                                agent_lane_settings_repo.clone(),
                                ideation_effort_settings_repo.clone(),
                                ideation_model_settings_repo.clone(),
                                Arc::clone(ideation_session_repo),
                                Arc::clone(
                                    delegated_session_repo
                                        .as_ref()
                                        .expect("delegated session repo available"),
                                ),
                                Arc::clone(task_repo),
                                &[],
                                0,
                                None,
                                None,
                                false,
                            )
                            .await
                            .map(|provider_spawnable| provider_spawnable.spawnable);

                        if let Ok(spawnable) = retry_spawnable {
                            if let Ok(retry_child) = spawnable.spawn().await {
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
                                        delegated_session_repo
                                            .as_ref()
                                            .expect("delegated session repo available"),
                                        execution_settings_repo,
                                        &agent_lane_settings_repo,
                                        &ideation_effort_settings_repo,
                                        &ideation_model_settings_repo,
                                        task_proposal_repo,
                                        activity_event_repo,
                                        memory_event_repo,
                                        message_queue,
                                        running_agent_registry,
                                        execution_state,
                                        question_state,
                                        plan_branch_repo,
                                        app_handle,
                                        run_chain_id.clone(),
                                        user_message_content,
                                        retry_conv,
                                        agent_name,
                                        team_mode,
                                        review_repo,
                                        task_step_repo,
                                        interactive_process_registry,
                                        verification_child_registry,
                                    ),
                                );

                                return true;
                            }
                        }

                        tracing::error!("Failed to spawn retry after recovery");
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
    let redacted_error = redact(error);

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
                    app_handle.as_ref(),
                    event_ctx,
                    pre_assistant_msg_id,
                    &get_assistant_role(&context_type).to_string(),
                    &existing_content,
                    existing_tool_calls,
                    existing_content_blocks,
                    "verification_parent_resolved",
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
            crate::application::reconciliation::verification_reconciliation::reset_verification_on_child_error(
                &child_id,
                ideation_session_repo,
                app_handle.as_ref(),
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
                app_handle.as_ref(),
                event_ctx,
                pre_assistant_msg_id,
                &get_assistant_role(&context_type).to_string(),
                &existing_content,
                existing_tool_calls,
                existing_content_blocks,
                "verification_auto_continue",
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
        app_handle.as_ref(),
        event_ctx,
        pre_assistant_msg_id,
        &get_assistant_role(&context_type).to_string(),
        &error_note,
        existing_tool_calls,
        existing_content_blocks,
        "interrupted",
    )
    .await;

    if let Some(StreamError::ProviderError {
        category,
        message,
        retry_after,
    }) = stream_error
    {
        apply_system_wide_provider_pause(
            app_handle,
            category,
            message,
            retry_after,
            &context_type.to_string(),
            context_id,
        )
        .await;

        if should_requeue_after_provider_pause(context_type) {
            if let Some(msg) = user_message_content {
                let _ = message_queue.queue_with_overrides(
                    context_type,
                    context_id.to_string(),
                    msg.to_string(),
                    Some(r#"{"resume_in_place":true}"#.to_string()),
                    None,
                    Some(effective_harness),
                );
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

                    if !task_execution_attempt_matches_current_status(
                        &task_id,
                        agent_run_id,
                        task_repo,
                        agent_run_repo,
                    )
                    .await
                    {
                        tracing::info!(
                            task_id = task_id.as_str(),
                            agent_run_id,
                            "Skipping stale task-execution failure for an older attempt"
                        );
                        return false;
                    }

                    let Some(current_task) = load_current_task_execution_attempt(
                        &task_id,
                        agent_run_id,
                        task_repo,
                        agent_run_repo,
                    )
                    .await
                    else {
                        tracing::info!(
                            task_id = task_id.as_str(),
                            agent_run_id,
                            "Skipping stale task-execution failure; task is no longer in the same execution attempt"
                        );
                        return false;
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

                            // Pre-compute failure metadata for timeouts so on_enter(Failed)
                            // skip guard preserves is_timeout=true via the failure_error key
                            if matches!(stream_error, Some(StreamError::Timeout { .. })) {
                                obj.insert(
                                    "failure_error".to_string(),
                                    serde_json::json!(redacted_error),
                                );
                                obj.insert("is_timeout".to_string(), serde_json::json!(true));
                            }

                            // Classify failure and write ExecutionRecoveryMetadata alongside
                            // the flat metadata. Provider errors are handled separately
                            // (they → Paused, not Failed) so we skip them here.
                            if let Some(se) = stream_error {
                                if !se.is_provider_error() {
                                    use crate::domain::entities::{
                                        ExecutionRecoveryEvent, ExecutionRecoveryEventKind,
                                        ExecutionRecoveryMetadata, ExecutionRecoveryReasonCode,
                                        ExecutionRecoverySource, ExecutionRecoveryState,
                                    };
                                    let failure_source = se.to_execution_failure_source();
                                    let reason_code = match se {
                                        StreamError::Timeout { .. } => {
                                            ExecutionRecoveryReasonCode::Timeout
                                        }
                                        StreamError::ParseStall { .. } => {
                                            ExecutionRecoveryReasonCode::ParseStall
                                        }
                                        StreamError::AgentExit { .. } => {
                                            ExecutionRecoveryReasonCode::AgentExit
                                        }
                                        _ => ExecutionRecoveryReasonCode::Unknown,
                                    };
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
                                    recovery.append_event_with_state(
                                        recovery_event,
                                        ExecutionRecoveryState::Retrying,
                                    );
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
                                if let Some(ref handle) = app_handle {
                                    let _ = handle.emit(
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

                    // AgentExit with all steps completed → agent called execution_complete
                    // successfully but exited with signal (code=None). Override to PendingReview.
                    let target_status = if target_status == InternalStatus::Failed
                        && matches!(stream_error, Some(StreamError::AgentExit { .. }))
                    {
                        let all_steps_done = all_steps_completed(task_step_repo, &task_id).await;

                        if all_steps_done {
                            tracing::info!(
                                task_id = task_id.as_str(),
                                "AgentExit with all steps completed — overriding Failed → PendingReview"
                            );
                            InternalStatus::PendingReview
                        } else {
                            target_status
                        }
                    } else {
                        target_status
                    };

                    let transition_service = build_transition_service(
                        app_handle,
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
                    );
                    let transition_service = if let Some(ref repo) = execution_settings_repo {
                        transition_service.with_execution_settings_repo(Arc::clone(repo))
                    } else {
                        transition_service
                    };
                    let transition_service = if let Some(ref repo) = plan_branch_repo {
                        transition_service.with_plan_branch_repo(Arc::clone(repo))
                    } else {
                        transition_service
                    };
                    let transition_service = if let Some(ref ipr) = interactive_process_registry {
                        transition_service.with_interactive_process_registry(Arc::clone(ipr))
                    } else {
                        transition_service
                    };

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
                            if let Some(ref handle) = app_handle {
                                let _ = handle.emit(
                                    "task:recovery_failed",
                                    serde_json::json!({
                                        "task_id": task_id.as_str(),
                                        "original_error": error,
                                        "transition_error": retry_err.to_string(),
                                        "target_status": target_status.to_string(),
                                    }),
                                );
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
                    app_handle: app_handle.as_ref(),
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
                        persist_shutdown_interrupted_metadata(task_repo, &task, "review", Some(error))
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
                        app_handle,
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
                    );
                    let transition_service = if let Some(ref repo) = execution_settings_repo {
                        transition_service.with_execution_settings_repo(Arc::clone(repo))
                    } else {
                        transition_service
                    };
                    let transition_service = if let Some(ref repo) = plan_branch_repo {
                        transition_service.with_plan_branch_repo(Arc::clone(repo))
                    } else {
                        transition_service
                    };
                    let transition_service = if let Some(ref ipr) = interactive_process_registry {
                        transition_service.with_interactive_process_registry(Arc::clone(ipr))
                    } else {
                        transition_service
                    };

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
    if let Some(ref handle) = app_handle {
        let _ = handle.emit(
            "agent:error",
            AgentErrorPayload {
                conversation_id: Some(conversation_id.as_str().to_string()),
                context_type: context_type.to_string(),
                context_id: context_id.to_string(),
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

    let (terminal_status, in_progress, convergence_reason, current_gaps) = match ideation_session_repo
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
