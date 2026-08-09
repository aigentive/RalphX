// Background processing for send_message
//
// Extracted from chat_service/mod.rs to reduce file size.
// Handles stream processing, task transitions, queue processing, and event emissions.

use ralphx_events::{emit_serialized, EventSink};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::process::Child;
use tracing::Instrument;

use super::chat_service_context;
use super::chat_service_helpers::get_assistant_role;
use super::chat_service_run_finalization::{
    finalize_run_completed_by_id, queue_run_completed_event_authority as queue_authority,
    run_completed_event_is_authorized, run_completed_without_queue_is_authorized,
    terminal_failure_reason,
};
use super::chat_service_streaming::{
    completion_tool_result_accepted, is_completion_tool_name, process_stream_background,
};
use super::chat_service_types::{
    AgentErrorPayload, AgentMessageCreatedPayload, AgentMessageRenderReadyPayload,
    AgentRunCompletedPayload,
};
use super::{event_context, has_meaningful_output, EventContextPayload, StreamingStateCache};
use crate::application::interactive_process_registry::{
    InteractiveProcess, InteractiveProcessKey, InteractiveProcessRegistry, InteractiveProcessToken,
};
use crate::application::memory_orchestration::trigger_memory_pipelines;
use crate::application::notification_service::NotificationService;
use crate::application::plan_verification_service::PlanVerificationCompletionAdapter;
use crate::application::question_state::QuestionState;
use crate::application::runtime_factory::{build_chat_service_from_deps, ChatRuntimeFactoryDeps};
use crate::commands::ExecutionState;
use crate::domain::agents::{AgentHarnessKind, ProviderSessionRef};
use crate::domain::entities::{
    AgentRunId, ChatContextType, ChatConversationId, ChatMessageAttribution, InternalStatus,
    SessionPurpose, TaskId,
};
use crate::domain::entities::{ChatConversation, ChatTimelineItem};
use crate::domain::repositories::{
    ActivityEventRepository, AgentLaneSettingsRepository, AgentProviderSettingsRepository,
    AgentRunRepository, ArtifactRepository, ChatAttachmentRepository, ChatConversationRepository,
    ChatMessageRepository, ChatTimelineRepository, DelegatedSessionRepository,
    ExecutionSettingsRepository, ExternalEventsRepository, IdeationSessionRepository,
    MemoryEventRepository, PlanBranchRepository, ProjectRepository, QueuedMessageRepository,
    ReviewRepository, TaskDependencyRepository, TaskProposalRepository, TaskRepository,
    TaskStepRepository, ValidationRunRepository,
};
use crate::domain::services::{
    MessageQueue, QueueKey, QueuedMessage, RunningAgentKey, RunningAgentRegistry,
};
use crate::domain::state_machine::services::WebhookPublisher;
use crate::infrastructure::agents::claude::{ContentBlockItem, ToolCall};
use tokio_util::sync::CancellationToken;

/// All repository and service dependencies grouped together.
pub(super) struct BackgroundRunRepos {
    pub chat_message_repo: Arc<dyn ChatMessageRepository>,
    pub chat_timeline_repo: Option<Arc<dyn ChatTimelineRepository>>,
    pub chat_attachment_repo: Arc<dyn ChatAttachmentRepository>,
    pub artifact_repo: Arc<dyn ArtifactRepository>,
    pub conversation_repo: Arc<dyn ChatConversationRepository>,
    pub agent_run_repo: Arc<dyn AgentRunRepository>,
    pub task_repo: Arc<dyn TaskRepository>,
    pub task_dependency_repo: Arc<dyn TaskDependencyRepository>,
    pub project_repo: Arc<dyn ProjectRepository>,
    pub ideation_session_repo: Arc<dyn IdeationSessionRepository>,
    pub delegated_session_repo: Arc<dyn DelegatedSessionRepository>,
    pub execution_settings_repo: Option<Arc<dyn ExecutionSettingsRepository>>,
    pub agent_lane_settings_repo: Option<Arc<dyn AgentLaneSettingsRepository>>,
    pub agent_provider_settings_repo: Option<Arc<dyn AgentProviderSettingsRepository>>,
    pub task_proposal_repo: Option<Arc<dyn TaskProposalRepository>>,
    pub activity_event_repo: Arc<dyn ActivityEventRepository>,
    pub memory_event_repo: Arc<dyn MemoryEventRepository>,
    pub notification_service: Option<Arc<NotificationService>>,
    pub message_queue: Arc<MessageQueue>,
    pub queued_message_repo: Option<Arc<dyn QueuedMessageRepository>>,
    pub running_agent_registry: Arc<dyn RunningAgentRegistry>,
    pub task_step_repo: Option<Arc<dyn TaskStepRepository>>,
    pub validation_run_repo: Option<Arc<dyn ValidationRunRepository>>,
    pub external_events_repo: Option<Arc<dyn ExternalEventsRepository>>,
    pub webhook_publisher: Option<Arc<dyn WebhookPublisher>>,
    pub review_repo: Option<Arc<dyn ReviewRepository>>,
}

/// Full context for a background agent run, replacing 29 individual parameters.
pub(super) struct BackgroundRunContext {
    // Process
    pub child: Child,
    pub harness: AgentHarnessKind,
    // Context identification
    pub context_type: ChatContextType,
    pub context_id: String,
    pub runtime_context_id: String,
    pub conversation_id: ChatConversationId,
    pub agent_run_id: String,
    pub stored_session_id: Option<String>,
    // Paths
    pub working_directory: PathBuf,
    pub cli_path: PathBuf,
    pub plugin_dir: PathBuf,
    // Repositories and services
    pub repos: BackgroundRunRepos,
    // State
    pub execution_state: Option<Arc<ExecutionState>>,
    pub question_state: Option<Arc<QuestionState>>,
    pub plan_branch_repo: Option<Arc<dyn PlanBranchRepository>>,
    /// Cross-transport event publication is supplied by composition.
    pub events: Arc<dyn EventSink>,
    /// Typed terminal verification and approval capability.
    pub plan_verification_completion: Option<Arc<PlanVerificationCompletionAdapter>>,
    /// Complete dependency snapshot for queue replays and automatic verification turns.
    pub runtime_factory_deps: Option<ChatRuntimeFactoryDeps>,
    // Run chain correlation
    pub run_chain_id: Option<String>,
    // Run metadata
    pub is_retry_attempt: bool,
    pub persona_feature_enabled: bool,
    pub agent_name_override_set: bool,
    pub user_message_content: Option<String>,
    pub turn_metadata: Option<String>,
    pub conversation: Option<ChatConversation>,
    pub agent_name: Option<String>,
    pub assistant_message_attribution: ChatMessageAttribution,
    pub persist_conversation_provider_session_ref: bool,
    // Cancellation
    pub cancellation_token: CancellationToken,
    // Streaming state cache for frontend hydration
    pub streaming_state_cache: StreamingStateCache,
    // Interactive process registry for stdin cleanup on process exit
    pub interactive_process_registry: Option<Arc<InteractiveProcessRegistry>>,
    // Entry identity captured at registration; prevents an old stream exit from
    // deleting a newer process that replaced the same context key.
    pub interactive_process_token: Option<InteractiveProcessToken>,
    // Verification child process registry for PID-based cleanup after reconciliation
    pub verification_child_registry:
        Option<Arc<super::verification_child_process_registry::VerificationChildProcessRegistry>>,
}

/// Returns true when `--resume` was used (stored is Some) AND the stream returned a different
/// session ID (new_id is Some and differs from stored). False in all other cases.
fn session_changed_after_resume(stored: Option<&str>, new_id: Option<&str>) -> bool {
    match (stored, new_id) {
        (Some(s), Some(n)) => s != n,
        _ => false,
    }
}

pub(super) fn should_process_stream_queue(
    initial_queue_count: usize,
    has_session_for_queue: bool,
    silent_interactive_exit: bool,
    cancellation_requested: bool,
) -> bool {
    initial_queue_count > 0
        && has_session_for_queue
        && !(silent_interactive_exit && cancellation_requested)
}

const AGENT_TASK_LEDGER_SUBSTANTIAL_TOOL_CALL_COUNT: usize = 3;
const AGENT_TASK_LEDGER_TOOL_NAMES: &[&str] = &[
    "create_agent_task",
    "get_agent_task",
    "list_agent_tasks",
    "update_agent_task",
    "claim_agent_task",
    "complete_agent_task",
];
const AGENT_TASK_LEDGER_MUTATING_WORK_TOOL_NAMES: &[&str] = &[
    "Bash",
    "Edit",
    "MultiEdit",
    "Write",
    "NotebookEdit",
    "bash",
    "edit",
    "write",
    "apply_patch",
    "exec_command",
    "write_stdin",
];

const SILENT_COMPLETION_RECOVERY_REASON: &str = "silent_completion_after_tool_activity";
const SILENT_COMPLETION_RECOVERY_MAX_ATTEMPTS: u32 = 3;
const SILENT_COMPLETION_RECOVERY_INITIAL_BACKOFF_MS: u64 = 1_000;
const SILENT_COMPLETION_RECOVERY_MAX_BACKOFF_MS: u64 = 8_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SilentCompletionRecoveryEnqueue {
    NotNeeded,
    Queued { attempt: u32, backoff_ms: u64 },
    Exhausted { attempts: u32 },
}

fn maybe_warn_missing_agent_task_ledger(
    conversation: Option<&ChatConversation>,
    agent_name: Option<&str>,
    context_type: ChatContextType,
    context_id: &str,
    conversation_id: &ChatConversationId,
    tool_calls: &[ToolCall],
) {
    if !should_warn_missing_agent_task_ledger(conversation, tool_calls) {
        return;
    }

    let agent_mode = conversation
        .and_then(|conversation| conversation.agent_mode)
        .map(|mode| mode.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    tracing::warn!(
        conversation_id = conversation_id.as_str(),
        %context_type,
        context_id = %context_id,
        agent_name = agent_name.unwrap_or("unknown"),
        agent_mode = %agent_mode,
        tool_calls = tool_calls.len(),
        "Agent-mode run completed substantial tool-backed work without using the agent task ledger"
    );
}

fn should_warn_missing_agent_task_ledger(
    conversation: Option<&ChatConversation>,
    tool_calls: &[ToolCall],
) -> bool {
    let Some(conversation) = conversation else {
        return false;
    };
    if conversation.agent_mode.is_none() {
        return false;
    }
    if tool_calls
        .iter()
        .any(|tool_call| is_agent_task_ledger_tool(&tool_call.name))
    {
        return false;
    }

    tool_calls.len() >= AGENT_TASK_LEDGER_SUBSTANTIAL_TOOL_CALL_COUNT
        || tool_calls
            .iter()
            .any(|tool_call| is_mutating_work_tool(&tool_call.name))
}

fn is_agent_task_ledger_tool(tool_name: &str) -> bool {
    AGENT_TASK_LEDGER_TOOL_NAMES.iter().any(|ledger_tool| {
        tool_name == *ledger_tool
            || tool_name.ends_with(&format!("__{ledger_tool}"))
            || tool_name.ends_with(&format!("::{ledger_tool}"))
    })
}

fn is_mutating_work_tool(tool_name: &str) -> bool {
    AGENT_TASK_LEDGER_MUTATING_WORK_TOOL_NAMES.contains(&tool_name)
}

fn is_nonrecoverable_terminal_tool(tool_name: &str, result: Option<&serde_json::Value>) -> bool {
    let normalized = tool_name.trim().to_ascii_lowercase();
    normalized.ends_with("ask_user_question")
        || normalized.ends_with("permission_request")
        || normalized.ends_with("resolve_permission_request")
        || (is_completion_tool_name(tool_name)
            && result.is_some_and(|result| completion_tool_result_accepted(Some(result))))
}

fn is_recoverable_terminal_tool_activity(
    tool_name: &str,
    result: Option<&serde_json::Value>,
) -> bool {
    !is_nonrecoverable_terminal_tool(tool_name, result)
}

fn has_recoverable_tool_activity_after_final_text(
    response_text: &str,
    tool_calls: &[ToolCall],
    content_blocks: &[ContentBlockItem],
) -> bool {
    if content_blocks.is_empty() {
        return response_text.trim().is_empty()
            && tool_calls.last().is_some_and(|tool_call| {
                is_recoverable_terminal_tool_activity(&tool_call.name, tool_call.result.as_ref())
            });
    }

    let mut recoverable_tool_after_last_text = false;
    for block in content_blocks {
        match block {
            ContentBlockItem::Text { text } if !text.trim().is_empty() => {
                recoverable_tool_after_last_text = false;
            }
            ContentBlockItem::Text { .. } => {}
            ContentBlockItem::Thinking { .. } => {}
            ContentBlockItem::ToolUse { name, result, .. } => {
                recoverable_tool_after_last_text =
                    is_recoverable_terminal_tool_activity(name, result.as_ref());
            }
        }
    }

    recoverable_tool_after_last_text
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn should_recover_silent_completion(
    context_type: ChatContextType,
    response_text: &str,
    tool_calls: &[ToolCall],
    content_blocks: &[ContentBlockItem],
    turns_finalized: usize,
    silent_interactive_exit: bool,
    cancellation_requested: bool,
    has_session_for_queue: bool,
) -> bool {
    matches!(
        context_type,
        ChatContextType::Project | ChatContextType::Ideation | ChatContextType::Standalone
    ) && has_session_for_queue
        && turns_finalized == 0
        && !silent_interactive_exit
        && !cancellation_requested
        && has_recoverable_tool_activity_after_final_text(response_text, tool_calls, content_blocks)
}

pub(crate) fn silent_completion_recovery_attempt(metadata_override: Option<&str>) -> u32 {
    metadata_override
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .and_then(|value| {
            if value
                .get("recovery_reason")
                .and_then(|reason| reason.as_str())
                != Some(SILENT_COMPLETION_RECOVERY_REASON)
            {
                return None;
            }
            value
                .get("recovery_attempt")
                .and_then(|attempt| attempt.as_u64())
                .and_then(|attempt| u32::try_from(attempt).ok())
        })
        .unwrap_or(0)
}

pub(crate) fn silent_completion_recovery_max_attempts() -> u32 {
    SILENT_COMPLETION_RECOVERY_MAX_ATTEMPTS
}

pub(crate) fn silent_completion_recovery_backoff_ms(attempt: u32) -> u64 {
    let shift = attempt.saturating_sub(1).min(6);
    let multiplier = 1u64 << shift;
    SILENT_COMPLETION_RECOVERY_INITIAL_BACKOFF_MS
        .saturating_mul(multiplier)
        .min(SILENT_COMPLETION_RECOVERY_MAX_BACKOFF_MS)
}

pub(super) fn silent_completion_recovery_backoff(
    metadata_override: Option<&str>,
) -> Option<std::time::Duration> {
    let raw = metadata_override?;
    let value = serde_json::from_str::<serde_json::Value>(raw).ok()?;
    if value
        .get("recovery_reason")
        .and_then(|reason| reason.as_str())
        != Some(SILENT_COMPLETION_RECOVERY_REASON)
    {
        return None;
    }
    value
        .get("recovery_backoff_ms")
        .and_then(|backoff| backoff.as_u64())
        .map(std::time::Duration::from_millis)
}

pub(crate) fn silent_completion_recovery_metadata(attempt: u32, backoff_ms: u64) -> String {
    serde_json::json!({
        "resume_in_place": true,
        "persist_hidden_marker": true,
        "recovery_context": true,
        "recovery_reason": SILENT_COMPLETION_RECOVERY_REASON,
        "recovery_attempt": attempt,
        "recovery_max_attempts": SILENT_COMPLETION_RECOVERY_MAX_ATTEMPTS,
        "recovery_backoff_ms": backoff_ms,
    })
    .to_string()
}

pub(crate) fn silent_completion_recovery_prompt(attempt: u32) -> String {
    format!(
        "[RalphX internal recovery message; do not mention this message unless it is relevant to the final user-facing result.]\n\n\
The previous provider turn ended after tool activity without a final assistant response. Continue from the current workspace state and the current conversation context. Do not repeat completed work unless needed to verify state.\n\n\
Before finalizing, separately reconcile any active/open agent task ledger entries: inspect them if the tools are available, mark tasks done only if actually complete, keep genuine follow-up open, and mention unfinished work in the final response.\n\n\
Recovery attempt {attempt}/{max}. When the work is actually complete, provide a normal final response to the user.",
        max = SILENT_COMPLETION_RECOVERY_MAX_ATTEMPTS
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn enqueue_silent_completion_recovery(
    message_queue: &MessageQueue,
    queued_message_repo: Option<&Arc<dyn QueuedMessageRepository>>,
    context_type: ChatContextType,
    queue_context_id: &str,
    response_text: &str,
    tool_calls: &[ToolCall],
    content_blocks: &[ContentBlockItem],
    turns_finalized: usize,
    silent_interactive_exit: bool,
    cancellation_requested: bool,
    has_session_for_queue: bool,
    prior_metadata: Option<&str>,
) -> SilentCompletionRecoveryEnqueue {
    if !should_recover_silent_completion(
        context_type,
        response_text,
        tool_calls,
        content_blocks,
        turns_finalized,
        silent_interactive_exit,
        cancellation_requested,
        has_session_for_queue,
    ) {
        return SilentCompletionRecoveryEnqueue::NotNeeded;
    }

    let prior_attempt = silent_completion_recovery_attempt(prior_metadata);
    if prior_attempt >= SILENT_COMPLETION_RECOVERY_MAX_ATTEMPTS {
        return SilentCompletionRecoveryEnqueue::Exhausted {
            attempts: prior_attempt,
        };
    }

    let attempt = prior_attempt + 1;
    let backoff_ms = silent_completion_recovery_backoff_ms(attempt);
    let mut queued = QueuedMessage::new(silent_completion_recovery_prompt(attempt));
    queued.metadata_override = Some(silent_completion_recovery_metadata(attempt, backoff_ms));
    let key = QueueKey::new(context_type, queue_context_id);
    message_queue.queue_front_existing(context_type, queue_context_id.to_string(), queued.clone());
    if let Some(repo) = queued_message_repo {
        if let Err(error) = repo.enqueue_front(&key, &queued).await {
            tracing::warn!(
                %context_type,
                queue_context_id,
                queued_message_id = %queued.id,
                error = %error,
                "[RECOVERY] Failed to persist hidden silent-completion continuation"
            );
        }
    }

    SilentCompletionRecoveryEnqueue::Queued {
        attempt,
        backoff_ms,
    }
}

#[derive(Debug, Clone)]
struct AssistantTranscriptSegment {
    content: String,
    tool_calls: Vec<crate::infrastructure::agents::claude::ToolCall>,
    content_blocks: Vec<crate::infrastructure::agents::claude::ContentBlockItem>,
}

fn build_assistant_transcript_segments(
    tool_calls: &[crate::infrastructure::agents::claude::ToolCall],
    content_blocks: &[crate::infrastructure::agents::claude::ContentBlockItem],
) -> Vec<AssistantTranscriptSegment> {
    let mut segments = Vec::new();
    let mut current = AssistantTranscriptSegment {
        content: String::new(),
        tool_calls: Vec::new(),
        content_blocks: Vec::new(),
    };
    let mut tool_index = 0usize;
    let mut saw_tool_in_current = false;

    for block in content_blocks {
        if matches!(block, ContentBlockItem::Text { .. }) && saw_tool_in_current {
            if !current.content_blocks.is_empty() {
                segments.push(current);
                current = AssistantTranscriptSegment {
                    content: String::new(),
                    tool_calls: Vec::new(),
                    content_blocks: Vec::new(),
                };
            }
            saw_tool_in_current = false;
        }

        match block {
            ContentBlockItem::Text { text } => {
                current.content.push_str(text);
                current.content_blocks.push(block.clone());
            }
            ContentBlockItem::Thinking { .. } => {
                current.content_blocks.push(block.clone());
            }
            ContentBlockItem::ToolUse {
                id,
                name,
                arguments,
                result,
                parent_tool_use_id,
                diff_context,
            } => {
                let tool_call = tool_calls.get(tool_index).cloned().unwrap_or_else(|| {
                    crate::infrastructure::agents::claude::ToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        arguments: arguments.clone(),
                        result: result.clone(),
                        parent_tool_use_id: parent_tool_use_id.clone(),
                        diff_context: diff_context
                            .clone()
                            .and_then(|value| serde_json::from_value(value).ok()),
                        stats: None,
                    }
                });
                tool_index += 1;
                saw_tool_in_current = true;
                current.tool_calls.push(tool_call);
                current.content_blocks.push(block.clone());
            }
        }
    }

    if !current.content_blocks.is_empty() {
        segments.push(current);
    }

    segments
}

fn attribution_from_message(
    message: &crate::domain::entities::ChatMessage,
) -> ChatMessageAttribution {
    ChatMessageAttribution {
        attribution_source: message.attribution_source.clone(),
        provider_harness: message.provider_harness,
        provider_session_id: message.provider_session_id.clone(),
        upstream_provider: message.upstream_provider.clone(),
        provider_profile: message.provider_profile.clone(),
        logical_model: message.logical_model.clone(),
        effective_model_id: message.effective_model_id.clone(),
        logical_effort: message.logical_effort,
        effective_effort: message.effective_effort.clone(),
    }
}

pub(super) async fn should_split_verification_transcript(
    context_type: ChatContextType,
    context_id: &str,
    ideation_session_repo: &Arc<dyn IdeationSessionRepository>,
) -> bool {
    if context_type != ChatContextType::Ideation {
        return false;
    }

    ideation_session_repo
        .get_by_id(&crate::domain::entities::IdeationSessionId::from_string(
            context_id.to_string(),
        ))
        .await
        .ok()
        .flatten()
        .map(|session| session.session_purpose == SessionPurpose::Verification)
        .unwrap_or(false)
}

/// Placeholder text written to chat_messages and chat_message_blocks when the
/// streaming agent produces no output (no text, no tool calls, no stderr).
pub(super) const NO_OUTPUT_NOTE: &str = "[Agent completed with no output]";

/// Finalize an assistant message that produced no streamed content.
///
/// Writes the `NO_OUTPUT_NOTE` placeholder into both stores:
/// 1. `chat_messages` — so the legacy chat_messages-backed UI shows the note.
/// 2. `chat_message_blocks` — so the timeline-backed `IntegratedChatPanel` does
///    not render a blank assistant turn. Without the timeline mirror, the
///    post-loop flush in `process_stream_background` wrote zero blocks (empty
///    content_blocks), so timeline consumers had no row for the turn at all.
pub(super) async fn finalize_no_output_assistant_message(
    chat_message_repo: &Arc<dyn ChatMessageRepository>,
    chat_timeline_repo: &Option<Arc<dyn ChatTimelineRepository>>,
    events: &dyn EventSink,
    event_ctx: &EventContextPayload,
    conversation_id: &ChatConversationId,
    message_id: &str,
    role: &str,
) {
    let placeholder_blocks = vec![
        crate::infrastructure::agents::claude::ContentBlockItem::Text {
            text: NO_OUTPUT_NOTE.to_string(),
        },
    ];
    let timeline_items = super::chat_service_streaming::persist_timeline_snapshot(
        chat_timeline_repo,
        &conversation_id.as_str(),
        &Some(message_id.to_string()),
        &placeholder_blocks,
        crate::domain::entities::ChatTimelineItemStatus::Finalized,
    )
    .await;
    let _ = finalize_assistant_message(
        chat_message_repo,
        events,
        event_ctx,
        message_id,
        role,
        NO_OUTPUT_NOTE,
        None,
        None,
        timeline_items,
    )
    .await;
}

pub(super) async fn finalize_assistant_message(
    chat_message_repo: &Arc<dyn ChatMessageRepository>,
    events: &dyn EventSink,
    event_ctx: &EventContextPayload,
    message_id: &str,
    role: &str,
    content: &str,
    tool_calls_json: Option<&str>,
    content_blocks_json: Option<&str>,
    timeline_items: Vec<ChatTimelineItem>,
) -> bool {
    let message_id_entity =
        crate::domain::entities::ChatMessageId::from_string(message_id.to_string());
    let message_persisted = chat_message_repo
        .update_content(
            &message_id_entity,
            content,
            tool_calls_json,
            content_blocks_json,
        )
        .await
        .is_ok();

    if message_persisted {
        let render_ready = if timeline_items.is_empty() {
            None
        } else {
            chat_message_repo
                .get_by_id(&message_id_entity)
                .await
                .ok()
                .flatten()
                .and_then(|message| {
                    AgentMessageRenderReadyPayload::from_message_and_timeline_items(
                        &message,
                        timeline_items,
                    )
                })
        };
        if let Err(error) = emit_serialized(
            events,
            "agent:message_created",
            &AgentMessageCreatedPayload {
                message_id: message_id.to_string(),
                conversation_id: event_ctx.conversation_id.clone(),
                context_type: event_ctx.context_type.clone(),
                context_id: event_ctx.context_id.clone(),
                role: role.to_string(),
                content: content.to_string(),
                created_at: None,
                metadata: None,
                render_ready,
            },
        ) {
            tracing::warn!(%error, "Failed to serialize finalized assistant message event");
        }
    }

    message_persisted
}

pub(super) async fn finalize_structured_assistant_message(
    chat_message_repo: &Arc<dyn ChatMessageRepository>,
    chat_timeline_repo: &Option<Arc<dyn ChatTimelineRepository>>,
    events: &dyn EventSink,
    context_type: ChatContextType,
    context_id: &str,
    conversation_id: &ChatConversationId,
    message_id: &str,
    role: &str,
    content: &str,
    tool_calls: &[crate::infrastructure::agents::claude::ToolCall],
    content_blocks: &[crate::infrastructure::agents::claude::ContentBlockItem],
    split_verification_transcript: bool,
) -> bool {
    let event_ctx = event_context(conversation_id, &context_type, context_id);
    if split_verification_transcript {
        let segments = build_assistant_transcript_segments(tool_calls, content_blocks);
        if segments.len() > 1 {
            let original_message = chat_message_repo
                .get_by_id(&crate::domain::entities::ChatMessageId::from_string(
                    message_id.to_string(),
                ))
                .await
                .ok()
                .flatten();
            let attribution = original_message.as_ref().map(attribution_from_message);

            let mut messages_persisted = true;
            if let Some(first_segment) = segments.first() {
                let tool_calls_json = serde_json::to_string(&first_segment.tool_calls).ok();
                let content_blocks_json = serde_json::to_string(&first_segment.content_blocks).ok();
                let timeline_items = super::chat_service_streaming::persist_timeline_snapshot(
                    chat_timeline_repo,
                    &conversation_id.as_str(),
                    &Some(message_id.to_string()),
                    &first_segment.content_blocks,
                    crate::domain::entities::ChatTimelineItemStatus::Finalized,
                )
                .await;
                messages_persisted &= finalize_assistant_message(
                    chat_message_repo,
                    events,
                    &event_ctx,
                    message_id,
                    role,
                    &first_segment.content,
                    tool_calls_json.as_deref(),
                    content_blocks_json.as_deref(),
                    timeline_items,
                )
                .await;
            }

            for segment in segments.iter().skip(1) {
                let mut extra_message = chat_service_context::create_assistant_message(
                    context_type,
                    context_id,
                    &segment.content,
                    conversation_id.clone(),
                    &segment.tool_calls,
                    &segment.content_blocks,
                );
                if let Some(attribution) = attribution.clone() {
                    extra_message = extra_message.with_attribution(attribution);
                }

                if let Ok(created_message) = chat_message_repo.create(extra_message).await {
                    let timeline_items = super::chat_service_streaming::persist_timeline_snapshot(
                        chat_timeline_repo,
                        &conversation_id.as_str(),
                        &Some(created_message.id.as_str().to_string()),
                        &segment.content_blocks,
                        crate::domain::entities::ChatTimelineItemStatus::Finalized,
                    )
                    .await;
                    if let Err(error) = emit_serialized(
                        events,
                        "agent:message_created",
                        &AgentMessageCreatedPayload {
                            message_id: created_message.id.as_str().to_string(),
                            conversation_id: event_ctx.conversation_id.clone(),
                            context_type: event_ctx.context_type.clone(),
                            context_id: event_ctx.context_id.clone(),
                            role: role.to_string(),
                            content: created_message.content.clone(),
                            created_at: None,
                            metadata: None,
                            render_ready:
                                AgentMessageRenderReadyPayload::from_message_and_timeline_items(
                                    &created_message,
                                    timeline_items,
                                ),
                        },
                    ) {
                        tracing::warn!(%error, "Failed to serialize split assistant message event");
                    }
                } else {
                    messages_persisted = false;
                }
            }
            return messages_persisted;
        }
    }

    let tool_calls_json = serde_json::to_string(tool_calls).ok();
    let content_blocks_json = serde_json::to_string(content_blocks).ok();
    let timeline_items = super::chat_service_streaming::persist_timeline_snapshot(
        chat_timeline_repo,
        &conversation_id.as_str(),
        &Some(message_id.to_string()),
        content_blocks,
        crate::domain::entities::ChatTimelineItemStatus::Finalized,
    )
    .await;
    finalize_assistant_message(
        chat_message_repo,
        events,
        &event_ctx,
        message_id,
        role,
        content,
        tool_calls_json.as_deref(),
        content_blocks_json.as_deref(),
        timeline_items,
    )
    .await
}

#[doc(hidden)]
pub async fn finalize_no_output_assistant_message_for_test(
    chat_message_repo: &Arc<dyn ChatMessageRepository>,
    chat_timeline_repo: &Option<Arc<dyn ChatTimelineRepository>>,
    events: &dyn EventSink,
    conversation_id: &ChatConversationId,
    context_type: &str,
    context_id: &str,
    message_id: &str,
    role: &str,
) {
    let event_ctx = EventContextPayload {
        conversation_id: conversation_id.as_str().to_string(),
        context_type: context_type.to_string(),
        context_id: context_id.to_string(),
    };
    finalize_no_output_assistant_message(
        chat_message_repo,
        chat_timeline_repo,
        events,
        &event_ctx,
        conversation_id,
        message_id,
        role,
    )
    .await;
}

#[doc(hidden)]
pub async fn finalize_assistant_message_for_test(
    chat_message_repo: &Arc<dyn ChatMessageRepository>,
    events: &dyn EventSink,
    conversation_id: &str,
    context_type: &str,
    context_id: &str,
    message_id: &str,
    role: &str,
    content: &str,
    tool_calls_json: Option<&str>,
    content_blocks_json: Option<&str>,
) {
    let event_ctx = EventContextPayload {
        conversation_id: conversation_id.to_string(),
        context_type: context_type.to_string(),
        context_id: context_id.to_string(),
    };
    finalize_assistant_message(
        chat_message_repo,
        events,
        &event_ctx,
        message_id,
        role,
        content,
        tool_calls_json,
        content_blocks_json,
        Vec::new(),
    )
    .await;
}

#[doc(hidden)]
pub async fn finalize_structured_assistant_message_for_test(
    chat_message_repo: &Arc<dyn ChatMessageRepository>,
    events: &dyn EventSink,
    context_type: ChatContextType,
    context_id: &str,
    conversation_id: &ChatConversationId,
    message_id: &str,
    role: &str,
    content: &str,
    tool_calls: &[ToolCall],
    content_blocks: &[ContentBlockItem],
    split_verification_transcript: bool,
) {
    let _ = finalize_structured_assistant_message(
        chat_message_repo,
        &None,
        events,
        context_type,
        context_id,
        conversation_id,
        message_id,
        role,
        content,
        tool_calls,
        content_blocks,
        split_verification_transcript,
    )
    .await;
}

/// Spawn background task to process agent run, handle stream, transitions, and queue.
///
/// This function encapsulates the entire tokio::spawn background logic from send_message.
/// It processes the agent run stream, handles task state transitions (for TaskExecution),
/// and processes any queued messages using --resume.
pub fn spawn_send_message_background(ctx: BackgroundRunContext) {
    let span = tracing::info_span!(
        "agent_run",
        agent_run_id = %ctx.agent_run_id,
        run_chain_id = ctx.run_chain_id.as_deref().unwrap_or("none"),
        %ctx.context_type,
        context_id = %ctx.context_id,
        runtime_context_id = %ctx.runtime_context_id,
        conversation_id = ctx.conversation_id.as_str(),
    );

    tokio::spawn(async move {
        let BackgroundRunContext {
            child,
            harness,
            context_type,
            context_id,
            runtime_context_id,
            conversation_id,
            agent_run_id,
            stored_session_id,
            working_directory,
            cli_path,
            plugin_dir,
            repos,
            execution_state,
            question_state,
            plan_branch_repo,
            events,
            plan_verification_completion,
            runtime_factory_deps,
            run_chain_id,
            is_retry_attempt,
            persona_feature_enabled,
            agent_name_override_set,
            user_message_content,
            turn_metadata,
            conversation,
            agent_name,
            assistant_message_attribution,
            persist_conversation_provider_session_ref,
            cancellation_token,
            streaming_state_cache,
            interactive_process_registry,
            interactive_process_token,
            verification_child_registry,
        } = ctx;
        let BackgroundRunRepos {
            chat_message_repo,
            chat_timeline_repo,
            chat_attachment_repo,
            artifact_repo,
            conversation_repo,
            agent_run_repo,
            task_repo,
            task_dependency_repo,
            project_repo,
            ideation_session_repo,
            delegated_session_repo,
            execution_settings_repo,
            agent_lane_settings_repo,
            agent_provider_settings_repo,
            task_proposal_repo,
            activity_event_repo,
            memory_event_repo,
            notification_service,
            message_queue,
            queued_message_repo,
            running_agent_registry,
            task_step_repo,
            validation_run_repo,
            external_events_repo,
            webhook_publisher,
            review_repo,
        } = repos;

        tracing::debug!("send_background start");
        let conversation_coordination_mode =
            conversation.as_ref().map(|conversation| conversation.coordination_mode);
        let event_ctx = event_context(&conversation_id, &context_type, &context_id);
        let split_verification_transcript = should_split_verification_transcript(
            context_type,
            &context_id,
            &ideation_session_repo,
        )
        .await;

        // Clone completion signal EARLY for Merge/Review contexts.
        // The HTTP handlers (complete_merge, complete_review) call notify_one() then remove()
        // the IPR entry while the agent is still running. We must clone the Arc<Notify> now,
        // before the stream starts, so the deferral select! at the end of this function can
        // still await the signal even after the HTTP handler removes the IPR entry.
        let completion_signal: Option<Arc<tokio::sync::Notify>> =
            if matches!(context_type, ChatContextType::Merge | ChatContextType::Review) {
                if let Some(ref registry) = interactive_process_registry {
                    let ipr_key =
                        InteractiveProcessKey::new(context_type.to_string(), &runtime_context_id);
                    registry.get_completion_signal(&ipr_key).await
                } else {
                    None
                }
            } else {
                None
            };

        // Resolve project ID for RALPHX_PROJECT_ID env var (used in queue processing)
        let resolved_project_id = chat_service_context::resolve_project_id(
            context_type,
            &context_id,
            Arc::clone(&task_repo),
            Arc::clone(&ideation_session_repo),
            Arc::clone(&delegated_session_repo),
        )
        .await;
        let resolved_project_id_typed = resolved_project_id.as_ref().map(|s| crate::domain::entities::ProjectId::from_string(s.clone()));

        // Create key for unregistering
        let registry_key = RunningAgentKey::new(context_type.to_string(), &runtime_context_id);

        // Create empty assistant message BEFORE streaming starts (crash recovery)
        let pre_assistant_msg = chat_service_context::create_assistant_message(
            context_type, &context_id, "", conversation_id, &[], &[],
        )
        .with_attribution(assistant_message_attribution.clone());
        let pre_assistant_msg_id = pre_assistant_msg.id.as_str().to_string();
        let _ = chat_message_repo.create(pre_assistant_msg).await;

        tracing::debug!(
            conversation_id = conversation_id.as_str(),
            "send_background calling process_stream_background"
        );
        let result = process_stream_background(
            child,
            harness,
            context_type,
            &context_id,
            &conversation_id,
            Arc::clone(&events),
            plan_verification_completion.clone(),
            runtime_factory_deps.clone(),
            Some(Arc::clone(&activity_event_repo)),
            Some(Arc::clone(&task_repo)),
            Some(Arc::clone(&chat_message_repo)),
            chat_timeline_repo.clone(),
            Some(pre_assistant_msg_id.clone()),
            question_state.clone(),
            cancellation_token.clone(),
            streaming_state_cache.clone(),
            Some(Arc::clone(&running_agent_registry)),
            Some(Arc::clone(&agent_run_repo)),
            Some(agent_run_id.clone()),
            execution_state.clone(),
            Some(Arc::clone(&conversation_repo)),
            split_verification_transcript,
            persist_conversation_provider_session_ref,
            interactive_process_registry.clone(),
            interactive_process_token.map(|_| {
                InteractiveProcessKey::new(context_type.to_string(), &runtime_context_id)
            }),
            interactive_process_token,
        )
        .await;

        // Unregister the process when done (ownership check: only removes our own slot)
        running_agent_registry.unregister(&registry_key, &agent_run_id).await;

        // Always remove the IPR entry on stream exit — a dead process's stdin is useless.
        if let Some(ref ipr) = interactive_process_registry {
            let ipr_key = InteractiveProcessKey::new(
                context_type.to_string(),
                &runtime_context_id,
            );

            let mut removed = match interactive_process_token {
                Some(token) => ipr.remove_if_token(&ipr_key, token).await,
                None => ipr.remove(&ipr_key).await,
            };
            let pending_turns = removed
                .as_mut()
                .map(InteractiveProcess::take_pending_stdin_turns)
                .unwrap_or_default();
            let recovery_repositories = queued_message_repo.as_ref().map(|qmr| {
                (
                    Arc::clone(qmr),
                    Arc::clone(&chat_message_repo),
                    chat_timeline_repo.as_ref().map(Arc::clone),
                )
            });
            super::chat_service_queue::requeue_pending_stdin_turns(
                recovery_repositories
                    .as_ref()
                    .map(|(queued_message_repo, _, _)| queued_message_repo),
                &message_queue,
                events.as_ref(),
                context_type,
                &runtime_context_id,
                Some(conversation_id.as_str()),
                pending_turns,
                recovery_repositories.as_ref().and_then(
                    |(_, chat_message_repo, chat_timeline_repo)| {
                        chat_timeline_repo.as_ref().map(|ctr| {
                            super::chat_service_queue::AnsweredTurnEvidence {
                                chat_message_repo,
                                chat_timeline_repo: ctr,
                                conversation_id: &conversation_id,
                            }
                        })
                    },
                ),
            )
            .await;
            if removed.is_none() {
                tracing::debug!(
                    %context_type,
                    context_id = %context_id,
                    runtime_context_id = %runtime_context_id,
                    "[IPR_REMOVE] Stream exit preserved newer interactive process"
                );
            }
            tracing::info!(
                %context_type,
                context_id = %context_id,
                runtime_context_id = %runtime_context_id,
                "[IPR_REMOVE] Removed interactive process stdin on stream exit"
            );
        }

        // Clean up interactive idle slot tracking
        if let Some(ref exec) = execution_state {
            let slot_key = format!("{}/{}", context_type, context_id);
            exec.remove_interactive_slot(&slot_key);
        }

        match result {
            Ok(outcome) => {
                let execution_slot_held = outcome.execution_slot_held;
                let response_text = outcome.response_text;
                let tool_calls = outcome.tool_calls;
                let content_blocks = outcome.content_blocks;
                let provider_session_id = outcome.session_id;
                let stderr_text = crate::utils::secret_redactor::redact(&outcome.stderr_text);
                let turns_finalized = outcome.turns_finalized;
                let turn_completion_applied = outcome.completion_applied;
                let mode_handoff_exit = outcome.mode_handoff_exit;
                // Debug: Log what we got from stream processing
                tracing::info!(
                    "[CHAT_SERVICE] Stream complete: context={}/{}, response_len={}, tool_calls={}, session_id={:?}",
                    context_type,
                    context_id,
                    response_text.len(),
                    tool_calls.len(),
                    provider_session_id
                );
                maybe_warn_missing_agent_task_ledger(
                    conversation.as_ref(),
                    agent_name.as_deref(),
                    context_type,
                    &context_id,
                    &conversation_id,
                    &tool_calls,
                );

                // Update conversation with provider session id
                if let Some(ref sess_id) = provider_session_id {
                    tracing::info!("[CHAT_SERVICE] Updating conversation with session_id={}", sess_id);
                    if persist_conversation_provider_session_ref {
                        if let Err(e) = conversation_repo
                            .update_provider_session_ref(
                                &conversation_id,
                                &ProviderSessionRef {
                                    harness,
                                    provider_session_id: sess_id.clone(),
                                },
                            )
                            .await
                        {
                            tracing::error!(
                                error = %e,
                                conversation_id = conversation_id.as_str(),
                                session_id = %sess_id,
                                "[CHAT_SERVICE] Failed to persist provider_session_id — next resume attempt will use stale session ID"
                            );
                        }
                    }

                    let _ = chat_message_repo
                        .update_provider_session_ref(
                            &crate::domain::entities::ChatMessageId::from_string(
                                pre_assistant_msg_id.clone(),
                            ),
                            &ProviderSessionRef {
                                harness,
                                provider_session_id: sess_id.clone(),
                            },
                        )
                        .await;
                } else {
                    tracing::warn!("[CHAT_SERVICE] No provider session_id captured from stream - queue processing will be skipped!");
                }

                // Detect resume failure: if --resume was used but Claude returned a different session ID,
                // it silently started a fresh session (original session likely expired).
                // Instead of just logging, trigger recovery: rebuild conversation history and
                // enqueue it as a priority message so Claude gets context before any pending user messages.
                if session_changed_after_resume(
                    stored_session_id.as_deref(),
                    provider_session_id.as_deref(),
                ) && !outcome.silent_interactive_exit
                {
                    tracing::warn!(
                        stored_session_id = %stored_session_id.as_deref().unwrap_or(""),
                        new_session_id = %provider_session_id.as_deref().unwrap_or(""),
                        context_type = %context_type,
                        context_id = %context_id,
                        "[RESUME] Session ID changed after --resume — triggering context recovery"
                    );

                    // Build conversation replay to inject history into the new session
                    let replay_builder = super::chat_service_replay::ReplayBuilder::new(100_000);
                    match replay_builder.build_replay(&chat_message_repo, &conversation_id).await {
                        Ok(replay) if !replay.turns.is_empty() => {
                            let rehydration_prompt = super::chat_service_replay::build_rehydration_prompt(
                                &replay,
                                context_type,
                                &context_id,
                                "[System] Your session was silently restarted. The conversation history above has been restored. Briefly confirm you have this context, then wait for the next user message.",
                                None,
                            );

                            // Enqueue at front so history is sent before any pending user messages
                            let queued = message_queue.queue_front(
                                context_type,
                                &context_id,
                                rehydration_prompt,
                            );
                            if let Some(repo) = queued_message_repo.as_ref() {
                                let key = QueueKey::new(context_type, context_id.clone());
                                if let Err(error) = repo.enqueue_front(&key, &queued).await {
                                    tracing::warn!(
                                        %context_type,
                                        context_id = %context_id,
                                        queued_message_id = %queued.id,
                                        error = %error,
                                        "[RESUME] Failed to persist session swap rehydration queued message"
                                    );
                                }
                            }

                            tracing::info!(
                                replay_turns = replay.turns.len(),
                                estimated_tokens = replay.total_tokens,
                                "[RESUME] Enqueued conversation history replay for silent session swap recovery"
                            );
                        }
                        Ok(replay) => {
                            tracing::info!(
                                turns = replay.turns.len(),
                                "[RESUME] No conversation turns to replay, skipping history injection"
                            );
                        }
                        Err(e) => {
                            tracing::error!(
                                error = %e,
                                "[RESUME] Failed to build conversation replay for session swap recovery"
                            );
                        }
                    }

                    // Emit event to frontend so UI can show recovery banner
                    events.emit(
                        "agent:session_recovered",
                        serde_json::json!({
                            "conversation_id": conversation_id.as_str(),
                            "context_type": context_type.to_string(),
                            "context_id": context_id,
                            "message": "Session silently restarted — conversation history restored"
                        }),
                    );
                }

                // Update pre-created assistant message with final content.
                // When turns were finalized during interactive streaming, the original
                // pre_assistant_msg was already finalized in the TurnComplete handler.
                // The processor was reset, so response_text is empty. Skip overwriting.
                let has_output = has_meaningful_output(&response_text, tool_calls.len(), &stderr_text);
                let skip_post_loop_finalization = turns_finalized > 0 && !has_output;

                tracing::info!(
                    context_type = %context_type,
                    context_id = %context_id,
                    turns_finalized,
                    has_output,
                    skip_post_loop_finalization,
                    silent_interactive_exit = outcome.silent_interactive_exit,
                    "[LIFECYCLE] skip_post_loop_finalization decision"
                );

                let assistant_role = get_assistant_role(&context_type).to_string();
                let assistant_message_persisted = if skip_post_loop_finalization {
                    tracing::debug!(
                        turns_finalized,
                        "Skipping post-loop finalization — {} turn(s) already finalized in stream loop",
                        turns_finalized,
                    );
                    false
                } else if has_output {
                    finalize_structured_assistant_message(
                        &chat_message_repo,
                        &chat_timeline_repo,
                        events.as_ref(),
                        context_type,
                        &context_id,
                        &conversation_id,
                        &pre_assistant_msg_id,
                        &assistant_role,
                        &response_text,
                        &tool_calls,
                        &content_blocks,
                        split_verification_transcript,
                    )
                    .await
                } else {
                    // Stream completed with no content — update pre-created message so UI
                    // doesn't show "..." forever, and mirror the placeholder note into the
                    // timeline so the chat UI (which renders from chat_message_blocks)
                    // doesn't show a blank turn either.
                    finalize_no_output_assistant_message(
                        &chat_message_repo,
                        &chat_timeline_repo,
                        events.as_ref(),
                        &event_ctx,
                        &conversation_id,
                        &pre_assistant_msg_id,
                        &assistant_role,
                    )
                    .await;
                    true
                };

                // Treat zero-output runs as failed executions for autonomous task/review flows.
                // Note: when interactive turns were finalized, has_output is false (processor was reset)
                // but the run actually succeeded — override the flag for the run status check.
                let effective_has_output =
                    (has_output && assistant_message_persisted) || turns_finalized > 0;
                // When turns were finalized in the stream loop, agent_run was already
                // completed in the TurnComplete handler — skip duplicate completion.
                let mut completion_applied = turn_completion_applied;
                if !skip_post_loop_finalization {
                    if has_output && !assistant_message_persisted {
                        let _ = agent_run_repo
                            .fail(
                                &AgentRunId::from_string(&agent_run_id),
                                "Failed to persist the final assistant message",
                            )
                            .await;
                    } else if !effective_has_output
                        && (context_type == ChatContextType::TaskExecution
                            || context_type == ChatContextType::Review)
                    {
                        let _ = agent_run_repo
                            .fail(
                                &AgentRunId::from_string(&agent_run_id),
                                "Agent completed with no output",
                            )
                            .await;
                    } else {
                        completion_applied =
                            finalize_run_completed_by_id(&agent_run_repo, &agent_run_id).await;
                    }
                }

                if completion_applied
                    && ((has_output && assistant_message_persisted) || turns_finalized > 0)
                {
                    if let (Some(adapter), Some(deps)) = (
                        plan_verification_completion.as_ref(),
                        runtime_factory_deps.as_ref(),
                    ) {
                        let chat_service = build_chat_service_from_deps(execution_state.clone(), deps);
                        let verification_pending = match adapter
                            .admit_automatic(
                                &chat_service,
                                &conversation_id,
                                &AgentRunId::from_string(&agent_run_id),
                                true,
                            )
                            .await
                        {
                            Ok(disposition) => disposition.verification_pending(),
                            Err(error) => {
                                tracing::error!(
                                    error = %error,
                                    conversation_id = %conversation_id,
                                    run_id = %agent_run_id,
                                    "Stream exit: automatic plan verification admission failed"
                                );
                                false
                            }
                        };
                        if !verification_pending {
                            if let Err(error) = adapter.release_for_conversation(&conversation_id).await {
                                tracing::warn!(error = %error, conversation_id = %conversation_id, "Failed to release deferred plan approval after automatic admission settled");
                            }
                        }
                        if let Err(error) = adapter
                            .release_for_run(&AgentRunId::from_string(&agent_run_id))
                            .await
                        {
                            tracing::warn!(error = %error, run_id = %agent_run_id, "Failed to release deferred plan approval for terminal verification run");
                        }
                    }
                }

                // When TurnComplete freed the execution slot and the process exited
                // while idle, re-increment temporarily so that the state transition's
                // on_exit decrement produces the correct final count (net zero).
                //
                // Defense-in-depth: for Review context, skip re-increment if the task has
                // already transitioned past Reviewing. In that case the transition on_exit
                // won't fire again, so re-incrementing would produce a leaked count=1 that
                // causes false merge deferral. chat_service_handlers.rs catches this too
                // (else-branch), but this guard prevents the increment from firing at all.
                let review_allows_reincrement = if context_type == ChatContextType::Review {
                    let task_id = TaskId::from_string(context_id.clone());
                    match task_repo.get_by_id(&task_id).await {
                        Ok(Some(task)) if task.internal_status != InternalStatus::Reviewing => {
                            tracing::debug!(
                                context_id = %context_id,
                                status = ?task.internal_status,
                                "Skipping re-increment for Review context — task already past Reviewing"
                            );
                            false
                        }
                        _ => true,
                    }
                } else {
                    true
                };

                if !execution_slot_held
                    && super::uses_execution_slot(context_type)
                    && !(outcome.silent_interactive_exit && context_type == ChatContextType::Ideation)
                    && review_allows_reincrement
                {
                    if let Some(ref exec) = execution_state {
                        exec.increment_running();
                        tracing::debug!(
                            %context_type,
                            context_id = %context_id,
                            "Re-incremented before state transition to prevent double-decrement"
                        );
                    }
                }

                // Handle task state transitions and merge auto-completion
                super::chat_service_handlers::handle_stream_success(
                    &agent_run_id,
                    context_type,
                    &context_id,
                    effective_has_output,
                    outcome.completion_tool_called,
                    execution_slot_held,
                    &execution_state,
                    &task_repo,
                    &task_dependency_repo,
                    &project_repo,
                    &artifact_repo,
                    &chat_message_repo,
                    &chat_attachment_repo,
                    &conversation_repo,
                    &agent_run_repo,
                    &ideation_session_repo,
                    &activity_event_repo,
                    &message_queue,
                    &running_agent_registry,
                    &memory_event_repo,
                    &plan_branch_repo,
                    &task_step_repo,
                    &validation_run_repo,
                    &external_events_repo,
                    &webhook_publisher,
                    &execution_settings_repo,
                    &agent_lane_settings_repo,
                    &agent_provider_settings_repo,
                    &events,
                    runtime_factory_deps.as_ref(),
                    &interactive_process_registry,
                    &review_repo,
                    &verification_child_registry,
                )
                .await;

                // Guard: skip auto-archival for verification child sessions.
                // The run_completed hook (Fix 1) handles archival after confirming parent state
                // is reconciled. Auto-archiving here creates a race with the agent's final MCP
                // call (post_verification_status). The periodic reconciler is the fallback for
                // orphaned children if Fix 1's hook fails for any reason.
                if context_type == ChatContextType::Ideation {
                    let session_id = crate::domain::entities::IdeationSessionId::from_string(context_id.clone());
                    match ideation_session_repo.get_by_id(&session_id).await {
                        Ok(Some(session)) if session.session_purpose == crate::domain::entities::ideation::SessionPurpose::Verification => {
                            tracing::debug!(
                                session_id = %context_id,
                                "Skipping auto-archival for verification child session — deferred to run_completed hook"
                            );
                        }
                        Ok(Some(_)) => {} // not a verification session, no action
                        Ok(None) => {}    // session not found, no action
                        Err(e) => {
                            tracing::warn!(
                                session_id = %context_id,
                                error = %e,
                                "Failed to look up ideation session for auto-archival check"
                            );
                        }
                    }
                }

                // When an ideation session completes and frees a slot, check whether any
                // pending sessions are waiting for capacity in this project and launch them.
                if context_type == ChatContextType::Ideation {
                    if let (Some(project_id), Some(exec_state), Some(exec_settings), Some(deps)) = (
                        resolved_project_id.clone(),
                        execution_state.as_ref().cloned(),
                        execution_settings_repo.as_ref().cloned(),
                        runtime_factory_deps.clone(),
                    ) {
                        let chat_svc: Arc<dyn super::ChatService> = Arc::new(
                            build_chat_service_from_deps(Some(Arc::clone(&exec_state)), &deps),
                        );

                        let drain = Arc::new(
                            crate::application::pending_session_drain::PendingSessionDrainService::new(
                                Arc::clone(&ideation_session_repo),
                                Arc::clone(&project_repo),
                                Arc::clone(&task_repo),
                                Arc::clone(&conversation_repo),
                                exec_settings,
                                exec_state,
                                Arc::clone(&running_agent_registry),
                                Arc::clone(&message_queue),
                                chat_svc,
                            ),
                        );
                        tokio::spawn(async move {
                            drain.try_drain_pending_for_project(&project_id).await;
                        });
                    }
                }

                // Detect and log the "Cancelled + turns_finalized > 0" path.
                // In this scenario: agent did useful work (turns finalized in stream loop)
                // but the process was cancelled before returning. The subsequent
                // will_emit_run_completed check depends on silent_interactive_exit;
                // if that flag is false, run_completed may be skipped entirely.
                if cancellation_token.is_cancelled() && turns_finalized > 0 {
                    tracing::info!(
                        context_type = %context_type,
                        context_id = %context_id,
                        turns_finalized,
                        skip_post_loop_finalization,
                        silent_interactive_exit = outcome.silent_interactive_exit,
                        "[LIFECYCLE] Cancelled stream with turns_finalized>0 — run_completed emission depends on silent_interactive_exit"
                    );
                }

                // Staleness guard (defense-in-depth): drop stale queued messages before
                // processing on ANY process exit. Catches OOM/SIGKILL scenarios where
                // silent_interactive_exit flag cannot be set.
                let staleness_threshold_secs: u64 = std::env::var("QUEUE_STALENESS_THRESHOLD_SECS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(300);
                let stale_dropped = message_queue.remove_stale(
                    context_type,
                    &runtime_context_id,
                    staleness_threshold_secs,
                );
                let durable_stale_dropped = if let Some(repo) = queued_message_repo.as_ref() {
                    let key = QueueKey::new(context_type, runtime_context_id.clone());
                    match repo.remove_stale(&key, staleness_threshold_secs).await {
                        Ok(messages) => messages,
                        Err(error) => {
                            tracing::warn!(
                                %context_type,
                                context_id = %context_id,
                                runtime_context_id = %runtime_context_id,
                                error = %error,
                                "[QUEUE] Failed to remove stale durable queued messages"
                            );
                            Vec::new()
                        }
                    }
                } else {
                    Vec::new()
                };
                for msg in &stale_dropped {
                    tracing::warn!(
                        "[QUEUE] Dropped stale hidden recovery queued message (age > {}s) id={} for context {}:{}",
                        staleness_threshold_secs,
                        msg.id,
                        context_type,
                        runtime_context_id,
                    );
                }
                for msg in &durable_stale_dropped {
                    tracing::warn!(
                        "[QUEUE] Dropped stale durable hidden recovery queued message (age > {}s) id={} for context {}:{}",
                        staleness_threshold_secs,
                        msg.id,
                        context_type,
                        runtime_context_id,
                    );
                }

                // Check if there are queued messages to process
                // If yes, DON'T emit run_completed yet - emit it after queue processing
                // Use the stream's session_id if available, otherwise fall back to stored session_id
                let effective_session_id =
                    provider_session_id.clone().or(stored_session_id.clone());
                let has_session_for_queue = effective_session_id.is_some();
                let cancellation_requested = cancellation_token.is_cancelled();
                match enqueue_silent_completion_recovery(
                    message_queue.as_ref(),
                    queued_message_repo.as_ref(),
                    context_type,
                    &runtime_context_id,
                    &response_text,
                    &tool_calls,
                    &content_blocks,
                    turns_finalized,
                    outcome.silent_interactive_exit,
                    cancellation_requested,
                    has_session_for_queue,
                    turn_metadata.as_deref(),
                )
                .await
                {
                    SilentCompletionRecoveryEnqueue::Queued {
                        attempt,
                        backoff_ms,
                    } => {
                        tracing::warn!(
                            %context_type,
                            context_id = %context_id,
                            runtime_context_id = %runtime_context_id,
                            attempt,
                            max_attempts = SILENT_COMPLETION_RECOVERY_MAX_ATTEMPTS,
                            backoff_ms,
                            "[RECOVERY] Queued hidden silent-completion continuation"
                        );
                    }
                    SilentCompletionRecoveryEnqueue::Exhausted { attempts } => {
                        tracing::error!(
                            %context_type,
                            context_id = %context_id,
                            runtime_context_id = %runtime_context_id,
                            attempts,
                            "[RECOVERY] Silent-completion recovery attempts exhausted"
                        );
                    }
                    SilentCompletionRecoveryEnqueue::NotNeeded => {}
                }
                let initial_memory_queue_count = message_queue
                    .get_queued(context_type, &runtime_context_id)
                    .len();
                let initial_durable_queue_count = if initial_memory_queue_count == 0 {
                    if let Some(repo) = queued_message_repo.as_ref() {
                        let key = QueueKey::new(context_type, runtime_context_id.clone());
                        match repo.list(&key).await {
                            Ok(messages) => messages.len(),
                            Err(error) => {
                                tracing::warn!(
                                    %context_type,
                                    context_id = %context_id,
                                    runtime_context_id = %runtime_context_id,
                                    error = %error,
                                    "[QUEUE] Failed to list durable queued messages before drain"
                                );
                                0
                            }
                        }
                    } else {
                        0
                    }
                } else {
                    0
                };
                let initial_queue_count = initial_memory_queue_count + initial_durable_queue_count;
                // A runtime-handoff watchdog cancels only the retiring owner. Its
                // replacement must not inherit that cancelled token or be mistaken
                // for a user-requested stop.
                let queue_cancellation_requested = cancellation_requested && !mode_handoff_exit;
                let will_process_queue = should_process_stream_queue(
                    initial_queue_count,
                    has_session_for_queue,
                    outcome.silent_interactive_exit,
                    queue_cancellation_requested,
                );

                tracing::info!(
                    context_type = %context_type,
                    context_id = %context_id,
                    turns_finalized,
                    skip_post_loop_finalization,
                    silent_interactive_exit = outcome.silent_interactive_exit,
                    mode_handoff_exit,
                    cancellation_requested,
                    initial_queue_count,
                    has_session_for_queue,
                    will_process_queue,
                    "[LIFECYCLE] will_process_queue decision"
                );

                if initial_queue_count > 0
                    && provider_session_id.is_none()
                    && stored_session_id.is_some()
                {
                    tracing::info!(
                        "[QUEUE] Stream had no session_id, using stored session_id from conversation for queue processing"
                    );
                }

                // Only emit run_completed if there's no queue to process.
                // If there IS a queue, we'll emit run_completed after all queue messages are processed.
                // When turns were already finalized in the stream loop, skip the duplicate emission.
                if !will_process_queue {
                    // Clear streaming state cache - stream completed successfully
                    let conv_id_str = conversation_id.as_str();
                    streaming_state_cache.clear(&conv_id_str).await;

                    // Authority is the persisted terminal status, not whether this call
                    // happened to apply the completion write: another writer (TurnComplete
                    // finalizer, HTTP completion handlers) may legitimately own it.
                    let completion_authorized = run_completed_event_is_authorized(
                        &agent_run_repo,
                        &AgentRunId::from_string(&agent_run_id),
                    )
                    .await;
                    let will_emit_run_completed = run_completed_without_queue_is_authorized(
                        completion_authorized,
                        skip_post_loop_finalization,
                        outcome.silent_interactive_exit,
                    );
                    tracing::info!(
                        context_type = %context_type,
                        context_id = %context_id,
                        turns_finalized,
                        skip_post_loop_finalization,
                        silent_interactive_exit = outcome.silent_interactive_exit,
                        will_process_queue,
                        will_emit_run_completed,
                        "[LIFECYCLE] run_completed emission decision (no-queue path)"
                    );

                    if will_emit_run_completed {
                        // Defer run_completed for merge/review until the HTTP handler signals
                        // completion (or 15s timeout). This prevents the premature "previous run"
                        // banner while branch cleanup and notifications are still in progress.
                        if outcome.silent_interactive_exit
                            && matches!(context_type, ChatContextType::Merge | ChatContextType::Review)
                        {
                            if let Some(ref signal) = completion_signal {
                                tracing::info!(
                                    context_type = %context_type,
                                    context_id = %context_id,
                                    "[LIFECYCLE] Deferring run_completed: awaiting CompletionSignal from HTTP handler (15s max)"
                                );
                                tokio::select! {
                                    _ = signal.notified() => {
                                        tracing::info!(
                                            context_type = %context_type,
                                            context_id = %context_id,
                                            "[LIFECYCLE] CompletionSignal received — emitting run_completed"
                                        );
                                    }
                                    _ = tokio::time::sleep(std::time::Duration::from_secs(15)) => {
                                        tracing::warn!(
                                            context_type = %context_type,
                                            context_id = %context_id,
                                            "[LIFECYCLE] CompletionSignal timeout (15s) — emitting run_completed anyway"
                                        );
                                    }
                                }
                            }
                        }

                        let _ = emit_serialized(
                            events.as_ref(),
                            "agent:run_completed",
                            &AgentRunCompletedPayload::with_provider_session_and_run_id(
                                Some(agent_run_id.clone()),
                                conversation_id.as_str().to_string(),
                                context_type.to_string(),
                                context_id.clone(),
                                Some(harness),
                                effective_session_id.clone(),
                                run_chain_id.clone(),
                            ),
                        );
                    } else if let Some(reason) =
                        terminal_failure_reason(&agent_run_repo, &AgentRunId::from_string(&agent_run_id))
                            .await
                    {
                        // The stream ended without an error, but the run was classified as
                        // failed afterwards (persist failure / zero-output autonomous run).
                        // handle_stream_error never runs here, so emit the terminal event
                        // ourselves instead of leaving the UI generating until the watchdog.
                        let _ = emit_serialized(
                            events.as_ref(),
                            "agent:error",
                            &AgentErrorPayload {
                                conversation_id: Some(conversation_id.as_str().to_string()),
                                context_type: context_type.to_string(),
                                context_id: context_id.clone(),
                                agent_run_id: Some(agent_run_id.clone()),
                                error: reason.clone(),
                                stderr: Some(reason),
                            },
                        );
                    }

                    // Trigger memory pipelines (no queue processing path)
                    trigger_memory_pipelines(
                        context_type,
                        &context_id,
                        &conversation_id,
                        resolved_project_id_typed.as_ref(),
                        agent_name.as_deref(),
                        &cli_path,
                        &plugin_dir,
                        &working_directory,
                        None,
                        Some(Arc::clone(&memory_event_repo)),
                    )
                    .await;
                } else {
                    tracing::info!(
                        "[QUEUE] Deferring run_completed: {} queued messages to process first",
                        initial_queue_count
                    );
                }

                // Process queued messages via extracted function
                if will_process_queue {
                    let Some(ref sess_id) = effective_session_id else {
                        unreachable!("will_process_queue requires has_session_for_queue=true");
                    };
                    let queue_outcome = super::chat_service_queue::process_queued_messages(
                        context_type,
                        harness,
                        &context_id,
                        &runtime_context_id,
                        conversation_id,
                        sess_id,
                        persona_feature_enabled,
                        &message_queue,
                        queued_message_repo,
                        agent_provider_settings_repo.as_ref().map(Arc::clone),
                        &running_agent_registry,
                        &agent_run_repo,
                        &chat_message_repo,
                        chat_timeline_repo.clone(),
                        &chat_attachment_repo,
                        &artifact_repo,
                        &activity_event_repo,
                        &task_repo,
                        &ideation_session_repo,
                        &cli_path,
                        &plugin_dir,
                        &working_directory,
                        question_state.clone(),
                        execution_state.clone(),
                        Arc::clone(&events),
                        plan_verification_completion.clone(),
                        runtime_factory_deps.clone(),
                        resolved_project_id.as_deref(),
                        conversation_coordination_mode,
                        if mode_handoff_exit {
                            CancellationToken::new()
                        } else {
                            cancellation_token.clone()
                        },
                        run_chain_id.as_deref(),
                        Some(&agent_run_id),
                        streaming_state_cache.clone(),
                    )
                    .await;
                    let total_processed = queue_outcome.total_processed;
                    let (terminal_run_id, will_emit_run_completed) =
                        queue_authority(&agent_run_repo, &queue_outcome, &agent_run_id).await;

                    // After ALL queue processing is done, emit the final run_completed.
                    // Queue counts never grant success authority; the terminal persisted
                    // run must be Completed before this success event is emitted.
                    tracing::info!(
                        context_type = %context_type,
                        context_id = %context_id,
                        turns_finalized,
                        skip_post_loop_finalization,
                        will_process_queue,
                        total_processed,
                        will_emit_run_completed,
                        "[LIFECYCLE] run_completed emission decision (queue path)"
                    );
                    if total_processed == 0 && initial_queue_count > 0 {
                        tracing::warn!(
                            context_type = %context_type,
                            context_id = %context_id,
                            initial_queue_count,
                            "[LIFECYCLE] queue processing ended with total_processed=0 (race/spawn failure/cancellation)"
                        );
                    }

                    // Clear streaming state cache - queue processing completed
                    let conv_id_str = conversation_id.as_str();
                    streaming_state_cache.clear(&conv_id_str).await;

                    if will_emit_run_completed {
                        tracing::info!(
                            total_processed,
                            terminal_run_id,
                            "[QUEUE] Emitting final run_completed after queue processing"
                        );
                        let _ = emit_serialized(
                            events.as_ref(),
                            "agent:run_completed",
                            &AgentRunCompletedPayload::with_provider_session_and_run_id(
                                Some(terminal_run_id),
                                conversation_id.as_str().to_string(),
                                context_type.to_string(),
                                context_id.clone(),
                                Some(harness),
                                Some(sess_id.clone()),
                                run_chain_id.clone(),
                            ),
                        );
                    } else {
                        tracing::warn!(
                            terminal_run_id,
                            "[QUEUE] Suppressing run_completed because persisted terminal authority is not Completed"
                        );
                        // Suppressing the success event must not leave the UI without any
                        // terminal event; surface the persisted failure instead.
                        if let Some(reason) = terminal_failure_reason(
                            &agent_run_repo,
                            &AgentRunId::from_string(&terminal_run_id),
                        )
                        .await
                        {
                            let _ = emit_serialized(
                                events.as_ref(),
                                "agent:error",
                                &AgentErrorPayload {
                                    conversation_id: Some(conversation_id.as_str().to_string()),
                                    context_type: context_type.to_string(),
                                    context_id: context_id.clone(),
                                    agent_run_id: Some(terminal_run_id),
                                    error: reason.clone(),
                                    stderr: Some(reason),
                                },
                            );
                        }
                    }

                    // Trigger memory pipelines after queue processing completes
                    trigger_memory_pipelines(
                        context_type,
                        &context_id,
                        &conversation_id,
                        resolved_project_id_typed.as_ref(),
                        agent_name.as_deref(),
                        &cli_path,
                        &plugin_dir,
                        &working_directory,
                        None,
                        Some(Arc::clone(&memory_event_repo)),
                    )
                    .await;
                } else {
                    let queue_count = message_queue
                        .get_queued(context_type, &runtime_context_id)
                        .len();
                    if effective_session_id.is_none() {
                        tracing::warn!(
                            context_type = %context_type,
                            context_id = %context_id,
                            turns_finalized,
                            skip_post_loop_finalization,
                            queue_count,
                            "[LIFECYCLE] effective_session_id=None: queue processing skipped, run_completed handled by no-queue path"
                        );
                        if queue_count > 0 {
                            tracing::warn!(
                                "[QUEUE] SKIPPING {} queued messages because no session_id available (neither from stream nor stored)!",
                                queue_count
                            );
                        }
                    } else if queue_count > 0 {
                        tracing::info!(
                            context_type = %context_type,
                            context_id = %context_id,
                            turns_finalized,
                            silent_interactive_exit = outcome.silent_interactive_exit,
                            queue_count,
                            "[LIFECYCLE] queue processing skipped by lifecycle gate, run_completed handled by no-queue path"
                        );
                    }
                }
            }
            Err(e) => {
                // Clear streaming state cache - stream errored
                let conv_id_str = conversation_id.as_str();
                streaming_state_cache.clear(&conv_id_str).await;

                // Delegate to error handler: classify, attempt recovery, fail run, emit events.
                // Returns true if recovery spawned a retry (no further action needed here
                // since the Err arm is the last statement in the async block).
                let error_string = e.to_string();
                let recovery_spawned = super::chat_service_handlers::handle_stream_error(
                    &error_string,
                    Some(&e),
                    context_type,
                    &context_id,
                    conversation_id,
                    &agent_run_id,
                    &pre_assistant_msg_id,
                    &event_ctx,
                    stored_session_id.as_deref(),
                    harness,
                    is_retry_attempt,
                    persona_feature_enabled,
                    agent_name_override_set,
                    user_message_content.as_deref(),
                    conversation.as_ref(),
                    resolved_project_id.clone(),
                    &cli_path,
                    &plugin_dir,
                    &working_directory,
                    &chat_message_repo,
                    &chat_timeline_repo,
                    &chat_attachment_repo,
                    &artifact_repo,
                    &conversation_repo,
                    &agent_run_repo,
                    &task_repo,
                    &task_dependency_repo,
                    &project_repo,
                    &ideation_session_repo,
                    &task_proposal_repo,
                    &activity_event_repo,
                    &message_queue,
                    &running_agent_registry,
                    &memory_event_repo,
                    &execution_state,
                    &question_state,
                    &plan_branch_repo,
                    &execution_settings_repo,
                    &agent_lane_settings_repo,
                    &agent_provider_settings_repo,
                    Arc::clone(&events),
                    plan_verification_completion.as_ref(),
                    runtime_factory_deps.as_ref(),
                    agent_name.as_deref(),
                    run_chain_id.clone(),
                    &interactive_process_registry,
                    &review_repo,
                    &task_step_repo,
                    &validation_run_repo,
                    &external_events_repo,
                    &webhook_publisher,
                    &verification_child_registry,
                    &notification_service,
                )
                .await;

                if !recovery_spawned {
                    if let Some(adapter) = plan_verification_completion.as_ref() {
                        if let Err(error) = adapter.release_for_conversation(&conversation_id).await {
                            tracing::warn!(error = %error, conversation_id = %conversation_id, "Failed to release deferred plan approval after terminal stream error");
                        }
                        if let Err(error) = adapter
                            .release_for_run(&AgentRunId::from_string(&agent_run_id))
                            .await
                        {
                            tracing::warn!(error = %error, run_id = %agent_run_id, "Failed to release deferred plan approval for failed verification run");
                        }
                    }
                    let queued_resume_in_place = message_queue
                        .get_queued(context_type, &runtime_context_id)
                        .iter()
                        .any(|queued_msg| {
                            super::chat_service_queue::queued_message_resume_in_place(
                                queued_msg.metadata_override.as_deref(),
                            )
                        });

                    if queued_resume_in_place {
                        let queue_session_id = stored_session_id.clone().or_else(|| {
                            conversation
                                .as_ref()
                                .and_then(|conv| conv.provider_session_ref())
                                .map(|session_ref| session_ref.provider_session_id.clone())
                        });

                        if let Some(ref session_id) = queue_session_id {
                            tracing::info!(
                                context_type = %context_type,
                                context_id = %context_id,
                                session_id,
                                "[QUEUE] Processing resume-in-place verification continuation after handled stream error"
                            );

                            let total_processed = super::chat_service_queue::process_queued_messages(
                                context_type,
                                harness,
                                &context_id,
                                &runtime_context_id,
                                conversation_id,
                                session_id,
                                persona_feature_enabled,
                                &message_queue,
                                queued_message_repo,
                                agent_provider_settings_repo.as_ref().map(Arc::clone),
                                &running_agent_registry,
                                &agent_run_repo,
                                &chat_message_repo,
                                chat_timeline_repo.clone(),
                                &chat_attachment_repo,
                                &artifact_repo,
                                &activity_event_repo,
                                &task_repo,
                                &ideation_session_repo,
                                &cli_path,
                                &plugin_dir,
                                &working_directory,
                                question_state.clone(),
                                execution_state.clone(),
                                Arc::clone(&events),
                                plan_verification_completion.clone(),
                                runtime_factory_deps.clone(),
                                resolved_project_id.as_deref(),
                                conversation_coordination_mode,
                                cancellation_token.clone(),
                                run_chain_id.as_deref(),
                                Some(&agent_run_id),
                                streaming_state_cache.clone(),
                            )
                            .await
                            .total_processed;

                            tracing::info!(
                                context_type = %context_type,
                                context_id = %context_id,
                                total_processed,
                                "[QUEUE] Resume-in-place verification continuation processing finished"
                            );

                            let _ = emit_serialized(
                                events.as_ref(),
                                "agent:run_completed",
                                &AgentRunCompletedPayload::with_provider_session(
                                    conversation_id.as_str().to_string(),
                                    context_type.to_string(),
                                    context_id.clone(),
                                    Some(harness),
                                    Some(session_id.clone()),
                                    run_chain_id.clone(),
                                ),
                            );
                        } else {
                            tracing::warn!(
                                context_type = %context_type,
                                context_id = %context_id,
                                "[QUEUE] Resume-in-place verification continuation queued but no session_id was available"
                            );
                        }
                    }
                }
            }
        }
    }.instrument(span));
}

#[cfg(test)]
#[path = "chat_service_send_background_tests.rs"]
mod tests;
