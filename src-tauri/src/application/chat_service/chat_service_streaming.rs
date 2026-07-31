// Chat Service Streaming Logic
//
// Extracted from chat_service.rs to improve modularity and reduce file size.
// Handles background stream processing and event emission.

use std::collections::HashMap;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::time::{timeout, Duration};
use tracing::info;

use crate::application::interactive_notification_producer::InteractiveNotificationProducer;
use crate::application::interactive_process_registry::{
    InteractiveProcessKey, InteractiveProcessRegistry,
    InteractiveProcessRetireAfterTurnDisposition, InteractiveProcessToken,
    InteractiveProcessTurnCompleteDisposition,
};
use crate::application::question_state::QuestionState;
use crate::domain::agents::{
    standard_harness_behavior, AgentHarnessKind, HarnessStreamMode, ProviderSessionRef,
};
use crate::domain::entities::{
    ActivityEvent, ActivityEventType, AgentRun, AgentRunId, AgentRunUsage, ChatContextType,
    ChatConversationId, ChatMessage, ChatMessageId, ChatTimelineItem, ChatTimelineItemKind,
    ChatTimelineItemStatus, MessageRole, ProviderUsageSnapshot, TaskId, UsageCapture,
    UsageProvenance,
};
use crate::domain::repositories::{
    ActivityEventRepository, AgentRunRepository, ChatConversationRepository, ChatMessageRepository,
    ChatTimelineRepository, TaskRepository,
};
use crate::domain::services::{RunningAgentKey, RunningAgentRegistry};
use crate::infrastructure::agents::claude::stream_timeouts;
use crate::infrastructure::agents::claude::{
    ContentBlockItem, DiffContext, StreamEvent, StreamProcessor, ToolCall, ToolCallStats,
};
use crate::infrastructure::agents::{
    extract_codex_agent_message, extract_codex_command_execution, extract_codex_error,
    extract_codex_file_change_snapshot, extract_codex_reasoning, extract_codex_thread_id,
    extract_codex_tool_call_snapshot, extract_codex_usage, parse_codex_event_line,
    CodexErrorSource, CodexFileChange, CodexFileChangeSnapshot, CodexToolCallPhase,
    CodexToolCallSnapshot, CodexUsage, CodexUsageSource,
};
use tokio_util::sync::CancellationToken;

use super::chat_service_errors::StreamError;
use super::chat_service_types::{retains_full_raw_tool_payload, AgentUsageUpdatedPayload};
use super::streaming_state_cache::{CachedStreamingTask, CachedToolCall, StreamingStateCache};
use super::tool_result_preview::{
    build_live_tool_argument_preview, build_live_tool_result_preview_for_tool_call,
    build_live_tool_result_preview_for_tool_id, live_tool_result_activity_content,
    live_tool_result_activity_metadata, tool_detail_ref,
};
use super::{
    event_context, events, has_meaningful_output, message_metadata_hidden_from_ui,
    AgentChunkPayload, AgentHookPayload, AgentTaskCompletedPayload, AgentTaskStartedPayload,
    AgentThinkingPayload, AgentThinkingProgressPayload, AgentToolCallPayload,
    AgentToolCallPreviewFields,
};
use crate::utils::truncate_str;
use crate::AppState;

/// Returns the index `persist_timeline_snapshot` will assign to the text block
/// this chunk belongs to.
fn current_text_block_position(completed_blocks: &[ContentBlockItem]) -> u64 {
    completed_blocks.len() as u64
}

#[doc(hidden)]
pub(crate) fn stream_mode_for_harness(harness: AgentHarnessKind) -> HarnessStreamMode {
    standard_harness_behavior(harness).stream_mode
}

#[doc(hidden)]
pub(crate) fn provider_session_ref_for_harness(
    harness: AgentHarnessKind,
    provider_session_id: impl Into<String>,
) -> ProviderSessionRef {
    ProviderSessionRef {
        harness,
        provider_session_id: provider_session_id.into(),
    }
}

pub(crate) fn is_user_attended_turn_completion(
    context_type: ChatContextType,
    automation_run_owned: bool,
    ideation_session_has_parent: bool,
    backend_action_owned: bool,
) -> bool {
    !backend_action_owned
        && !automation_run_owned
        && !ideation_session_has_parent
        && matches!(
            context_type,
            ChatContextType::Ideation
                | ChatContextType::Project
                | ChatContextType::Standalone
                | ChatContextType::Task
        )
}

async fn record_agent_waiting_if_user_attended<R: Runtime>(
    app_handle: &AppHandle<R>,
    context_type: ChatContextType,
    context_id: &str,
    conversation_id: &ChatConversationId,
    agent_run_id: Option<&str>,
) -> bool {
    let Some(state) = app_handle.try_state::<AppState>() else {
        tracing::warn!("Agent turn completed without managed AppState; agent_waiting skipped");
        return false;
    };
    let conversation = match state
        .chat_conversation_repo
        .get_by_id(conversation_id)
        .await
    {
        Ok(Some(conversation)) => conversation,
        Ok(None) => return false,
        Err(error) => {
            tracing::warn!(error = %error, conversation_id = %conversation_id, "Failed to load conversation for agent_waiting");
            return false;
        }
    };
    let backend_action_owned = if let Some(run_id) = agent_run_id {
        match state
            .agent_run_repo
            .get_by_id(&AgentRunId::from_string(run_id.to_string()))
            .await
        {
            Ok(Some(run)) => run.action_kind.is_some(),
            Ok(None) => false,
            Err(error) => {
                tracing::warn!(error = %error, run_id, "Failed to load run action authority for agent_waiting");
                return false;
            }
        }
    } else {
        false
    };

    let (project_id, ideation_session_has_parent, context_title) = match context_type {
        ChatContextType::Ideation => {
            let session_id = crate::domain::entities::IdeationSessionId::from_string(context_id);
            match state.ideation_session_repo.get_by_id(&session_id).await {
                Ok(Some(session)) => (
                    Some(session.project_id.to_string()),
                    session.parent_session_id.is_some(),
                    session.title,
                ),
                Ok(None) => return false,
                Err(error) => {
                    tracing::warn!(error = %error, session_id = %session_id, "Failed to load ideation session for agent_waiting");
                    return false;
                }
            }
        }
        ChatContextType::Project => (Some(context_id.to_string()), false, None),
        ChatContextType::Standalone => (None, false, None),
        ChatContextType::Task => {
            let task_id = TaskId::from_string(context_id.to_string());
            match state.task_repo.get_by_id(&task_id).await {
                Ok(Some(task)) => (Some(task.project_id.to_string()), false, Some(task.title)),
                Ok(None) => return false,
                Err(error) => {
                    tracing::warn!(error = %error, task_id = %task_id, "Failed to load task for agent_waiting");
                    return false;
                }
            }
        }
        ChatContextType::Delegation
        | ChatContextType::TaskExecution
        | ChatContextType::Review
        | ChatContextType::Merge
        | ChatContextType::BranchUpdate => return false,
    };

    if !is_user_attended_turn_completion(
        context_type,
        conversation.automation_run_id.is_some(),
        ideation_session_has_parent,
        backend_action_owned,
    ) {
        return false;
    }
    state
        .notification_service()
        .record_ephemeral(InteractiveNotificationProducer::agent_waiting(
            project_id,
            &conversation.id.as_str(),
            conversation.title.as_deref().or(context_title.as_deref()),
        ))
        .await;
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProcessExitDetails {
    pub exit_code: Option<i32>,
    pub exit_signal: Option<i32>,
    pub success: bool,
}

#[doc(hidden)]
pub(crate) fn process_exit_details(status: &std::process::ExitStatus) -> ProcessExitDetails {
    #[cfg(unix)]
    let exit_signal = {
        use std::os::unix::process::ExitStatusExt;
        status.signal()
    };
    #[cfg(not(unix))]
    let exit_signal = None;

    ProcessExitDetails {
        exit_code: status.code(),
        exit_signal,
        success: status.success(),
    }
}

#[cfg(unix)]
fn signal_name(signal: i32) -> Option<&'static str> {
    match signal {
        6 => Some("SIGABRT"),
        9 => Some("SIGKILL"),
        11 => Some("SIGSEGV"),
        15 => Some("SIGTERM"),
        _ => None,
    }
}

#[cfg(not(unix))]
fn signal_name(_signal: i32) -> Option<&'static str> {
    None
}

#[doc(hidden)]
pub(crate) fn format_agent_exit_stderr(details: ProcessExitDetails, stderr: &str) -> String {
    let trimmed = stderr.trim();
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }

    if let Some(signal) = details.exit_signal {
        if let Some(name) = signal_name(signal) {
            return format!("Agent process exited with signal {signal} ({name})");
        }
        return format!("Agent process exited with signal {signal}");
    }

    format!(
        "Agent exited with non-zero status (code={:?})",
        details.exit_code
    )
}

const COMPLETION_TOOL_NAMES: &[&str] = &[
    "mcp__ralphx__execution_complete",
    "mcp__ralphx__complete_review",
    "mcp__ralphx__complete_merge",
    "mcp__ralphx__complete_agent_workspace_repair",
    "mcp__ralphx__complete_workspace_review_run",
    "mcp__ralphx__finalize_proposals",
];

#[doc(hidden)]
pub fn is_completion_tool_name(name: &str) -> bool {
    let normalized = name.trim().to_ascii_lowercase();

    if COMPLETION_TOOL_NAMES.contains(&normalized.as_str()) {
        return true;
    }

    if let Some(tool_name) = normalized.strip_prefix("ralphx::") {
        return matches!(
            tool_name,
            "execution_complete"
                | "complete_review"
                | "complete_merge"
                | "complete_agent_workspace_repair"
                | "complete_workspace_review_run"
                | "finalize_proposals"
        );
    }

    if let Some(tool_name) = normalized.strip_prefix("ralphx:") {
        return matches!(
            tool_name,
            "execution_complete"
                | "complete_review"
                | "complete_merge"
                | "complete_agent_workspace_repair"
                | "complete_workspace_review_run"
                | "finalize_proposals"
        );
    }

    false
}

/// Final flush of accumulated content to DB before returning an error.
///
/// Ensures that any content streamed before timeout/cancellation/parse-stall
/// is persisted, so that the error handler can later append (rather than overwrite).
async fn flush_content_before_error(
    chat_message_repo: &Option<Arc<dyn ChatMessageRepository>>,
    assistant_message_id: &Option<String>,
    response_text: &str,
    tool_calls: &[ToolCall],
    content_blocks: &[ContentBlockItem],
) {
    if let (Some(ref repo), Some(ref msg_id)) = (chat_message_repo, assistant_message_id) {
        let current_tools = serde_json::to_string(tool_calls).ok();
        let current_blocks = serde_json::to_string(content_blocks).ok();
        let _ = repo
            .update_content(
                &ChatMessageId::from_string(msg_id.clone()),
                response_text,
                current_tools.as_deref(),
                current_blocks.as_deref(),
            )
            .await;
    }
}

async fn persist_usage_capture_run_first(
    agent_run_repo: &Option<Arc<dyn AgentRunRepository>>,
    agent_run_id: &Option<String>,
    chat_message_repo: &Option<Arc<dyn ChatMessageRepository>>,
    assistant_message_id: &Option<String>,
    capture: &UsageCapture,
) -> bool {
    let (Some(run_repo), Some(run_id)) = (agent_run_repo.as_ref(), agent_run_id.as_ref()) else {
        return false;
    };
    if let Err(error) = run_repo
        .replace_usage_capture(&AgentRunId::from_string(run_id.clone()), capture)
        .await
    {
        tracing::warn!(
            run_id,
            error = %error,
            "Failed to persist canonical run usage capture"
        );
        return false;
    }

    if let (Some(message_repo), Some(message_id)) =
        (chat_message_repo.as_ref(), assistant_message_id.as_ref())
    {
        if let Err(error) = message_repo
            .replace_usage_capture(&ChatMessageId::from_string(message_id.clone()), capture)
            .await
        {
            tracing::warn!(
                run_id,
                message_id,
                error = %error,
                "Failed to mirror canonical usage capture to assistant message"
            );
        }
    }

    true
}

fn emit_usage_updated_event<R: Runtime>(
    app_handle: &Option<AppHandle<R>>,
    conversation_id: &str,
    context_type: &str,
    context_id: &str,
) {
    if let Some(handle) = app_handle.as_ref() {
        let _ = handle.emit(
            events::AGENT_USAGE_UPDATED,
            AgentUsageUpdatedPayload {
                conversation_id: conversation_id.to_string(),
                context_type: context_type.to_string(),
                context_id: context_id.to_string(),
            },
        );
    }
}

async fn persist_assistant_message_snapshot(
    chat_message_repo: &Option<Arc<dyn ChatMessageRepository>>,
    assistant_message_id: &Option<String>,
    response_text: &str,
    tool_calls: &[ToolCall],
    content_blocks: &[ContentBlockItem],
) {
    if let (Some(repo), Some(message_id)) =
        (chat_message_repo.as_ref(), assistant_message_id.as_ref())
    {
        let tool_calls_json = serde_json::to_string(tool_calls).ok();
        let content_blocks_json = serde_json::to_string(content_blocks).ok();
        let _ = repo
            .update_content(
                &ChatMessageId::from_string(message_id.clone()),
                response_text,
                tool_calls_json.as_deref(),
                content_blocks_json.as_deref(),
            )
            .await;
    }
}

pub(super) async fn persist_timeline_snapshot(
    chat_timeline_repo: &Option<Arc<dyn ChatTimelineRepository>>,
    conversation_id: &str,
    assistant_message_id: &Option<String>,
    content_blocks: &[ContentBlockItem],
    status: ChatTimelineItemStatus,
) -> Vec<ChatTimelineItem> {
    let (Some(repo), Some(message_id)) =
        (chat_timeline_repo.as_ref(), assistant_message_id.as_ref())
    else {
        return Vec::new();
    };

    let conversation_id = ChatConversationId::from_string(conversation_id.to_string());
    let message_id = ChatMessageId::from_string(message_id.clone());
    let role = MessageRole::Orchestrator;
    let mut persisted_items = Vec::new();
    let mut persistence_failed = false;
    let mut retained_block_indices = Vec::new();

    for (index, block) in content_blocks.iter().enumerate() {
        let kind = match block {
            ContentBlockItem::Text { text } if text.is_empty() => continue,
            ContentBlockItem::Text { .. } => ChatTimelineItemKind::Text,
            ContentBlockItem::Thinking { .. } => ChatTimelineItemKind::Thinking,
            ContentBlockItem::ToolUse { .. } => ChatTimelineItemKind::ToolUse,
        };
        retained_block_indices.push(index as i64);

        let mut item = ChatTimelineItem::for_message_block(
            message_id.clone(),
            conversation_id,
            index as i64,
            role,
            kind,
        );
        item.status = status;
        item.updated_at = chrono::Utc::now();
        if status == ChatTimelineItemStatus::Finalized {
            item.finalized_at = Some(item.updated_at);
        }
        match block {
            ContentBlockItem::Text { text } => {
                item.text = Some(text.clone());
            }
            ContentBlockItem::Thinking { text, duration_ms } => {
                item.text = Some(text.clone());
                item.metadata = duration_ms.map(|duration_ms| {
                    serde_json::json!({ "duration_ms": duration_ms }).to_string()
                });
            }
            ContentBlockItem::ToolUse {
                id,
                name,
                arguments,
                result,
                ..
            } => {
                item.tool_call_id = id.clone();
                item.tool_name = Some(name.clone());
                if retains_full_raw_tool_payload(name) {
                    item.raw_block_json = serde_json::to_string(block).ok();
                }
                item.tool_status = Some(
                    if result.is_some() {
                        "completed"
                    } else {
                        "pending"
                    }
                    .to_string(),
                );
                item.tool_input_preview = Some(json_preview(arguments));
                item.input_json = Some(arguments.to_string());
                if let Some(result) = result {
                    item.tool_result_preview = Some(json_preview(result));
                    item.result_json = Some(result.to_string());
                }
            }
        }

        match repo.upsert_item(item).await {
            Ok(item) => persisted_items.push(item),
            Err(_) => {
                persistence_failed = true;
            }
        }
    }

    if persistence_failed {
        Vec::new()
    } else {
        if status != ChatTimelineItemStatus::Streaming {
            let _ = repo
                .delete_message_items_except_block_indices(&message_id, retained_block_indices)
                .await;
        }
        if status == ChatTimelineItemStatus::Finalized {
            let _ = repo.mark_message_items_finalized(&message_id).await;
        }
        persisted_items
    }
}

async fn flush_streaming_persistence_if_dirty(
    dirty: &mut bool,
    last_persisted_at: &mut std::time::Instant,
    chat_message_repo: &Option<Arc<dyn ChatMessageRepository>>,
    chat_timeline_repo: &Option<Arc<dyn ChatTimelineRepository>>,
    conversation_id: &str,
    assistant_message_id: &Option<String>,
    response_text: &str,
    tool_calls: &[ToolCall],
    content_blocks: &[ContentBlockItem],
    status: ChatTimelineItemStatus,
) {
    if !*dirty {
        return;
    }
    // The in-flight text block is not pushed into `content_blocks` until the
    // processor finishes, so a flush can land while the slice is still empty.
    // `persist_timeline_snapshot` deletes every block index it is not asked to
    // retain, so flushing here would wipe rows that are already durable. Stay
    // dirty and let a later flush persist real content.
    if content_blocks.is_empty() {
        return;
    }

    persist_assistant_message_snapshot(
        chat_message_repo,
        assistant_message_id,
        response_text,
        tool_calls,
        content_blocks,
    )
    .await;
    persist_timeline_snapshot(
        chat_timeline_repo,
        conversation_id,
        assistant_message_id,
        content_blocks,
        status,
    )
    .await;
    *dirty = false;
    *last_persisted_at = std::time::Instant::now();
}

pub(super) async fn persist_message_text_timeline_item(
    chat_timeline_repo: &Option<Arc<dyn ChatTimelineRepository>>,
    message: &ChatMessage,
) {
    let (Some(repo), Some(conversation_id)) =
        (chat_timeline_repo.as_ref(), message.conversation_id)
    else {
        return;
    };
    if message.content.is_empty() {
        return;
    }
    if message_metadata_hidden_from_ui(message.metadata.as_deref()) {
        return;
    }

    let mut item = ChatTimelineItem::for_message_block(
        message.id.clone(),
        conversation_id,
        0,
        message.role,
        ChatTimelineItemKind::Text,
    );
    item.status = ChatTimelineItemStatus::Finalized;
    item.text = Some(message.content.clone());
    item.metadata = message.metadata.clone();
    item.provider_harness = message.provider_harness;
    item.provider_session_id = message.provider_session_id.clone();
    item.created_at = message.created_at;
    item.updated_at = message.created_at;
    item.finalized_at = Some(message.created_at);

    let _ = repo.upsert_item(item).await;
}

fn json_preview(value: &serde_json::Value) -> String {
    let raw = value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string());
    truncate_str(&raw, 1_000).to_string()
}

fn codex_tool_call_content_block(tool_call: &ToolCall) -> ContentBlockItem {
    ContentBlockItem::ToolUse {
        id: tool_call.id.clone(),
        name: tool_call.name.clone(),
        arguments: tool_call.arguments.clone(),
        result: tool_call.result.clone(),
        parent_tool_use_id: tool_call.parent_tool_use_id.clone(),
        diff_context: tool_call
            .diff_context
            .as_ref()
            .and_then(|context| serde_json::to_value(context).ok()),
    }
}

fn upsert_codex_tool_call_snapshot(
    tool_calls: &mut Vec<ToolCall>,
    content_blocks: &mut Vec<ContentBlockItem>,
    tool_call: ToolCall,
) {
    if let Some(tool_id) = tool_call.id.as_deref() {
        if let Some(existing) = tool_calls
            .iter_mut()
            .find(|existing| existing.id.as_deref() == Some(tool_id))
        {
            existing.name = tool_call.name.clone();
            existing.arguments = tool_call.arguments.clone();
            if tool_call.result.is_some() || existing.result.is_none() {
                existing.result = tool_call.result.clone();
            }
            if tool_call.diff_context.is_some() || existing.diff_context.is_none() {
                existing.diff_context = tool_call.diff_context.clone();
            }
            if tool_call.stats.is_some() || existing.stats.is_none() {
                existing.stats = tool_call.stats.clone();
            }
        } else {
            tool_calls.push(tool_call.clone());
        }

        if let Some(existing_block) = content_blocks.iter_mut().find(|block| {
            matches!(
                block,
                ContentBlockItem::ToolUse { id, .. } if id.as_deref() == Some(tool_id)
            )
        }) {
            *existing_block = codex_tool_call_content_block(&tool_call);
            return;
        }
    } else if let Some(existing) = tool_calls.iter_mut().find(|existing| {
        existing.name == tool_call.name && existing.arguments == tool_call.arguments
    }) {
        if tool_call.result.is_some() || existing.result.is_none() {
            existing.result = tool_call.result.clone();
        }
        if tool_call.diff_context.is_some() || existing.diff_context.is_none() {
            existing.diff_context = tool_call.diff_context.clone();
        }
        if tool_call.stats.is_some() || existing.stats.is_none() {
            existing.stats = tool_call.stats.clone();
        }
        return;
    } else {
        tool_calls.push(tool_call.clone());
    }

    content_blocks.push(codex_tool_call_content_block(&tool_call));
}

#[derive(Debug, Clone)]
struct PendingCodexFileChange {
    path: String,
    kind: String,
    old_content: Option<String>,
    old_file_exists: Option<bool>,
}

fn capture_file_diff_baseline(path: &str) -> (Option<String>, Option<bool>) {
    match std::fs::read_to_string(path) {
        Ok(content) => (Some(content), Some(true)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (None, Some(false)),
        Err(_) => (None, Some(true)),
    }
}

fn codex_file_change_tool_call_id(item_id: Option<&str>, path: &str, index: usize) -> String {
    item_id
        .map(|id| format!("{id}:{index}"))
        .unwrap_or_else(|| format!("codex-file-change:{path}:{index}"))
}

fn codex_file_change_arguments(change: &CodexFileChange) -> serde_json::Value {
    serde_json::json!({
        "changes": [
            {
                "path": change.path,
                "kind": change.kind,
            }
        ]
    })
}

fn codex_file_change_started_snapshot(
    item_id: Option<&str>,
    change: &CodexFileChange,
    index: usize,
) -> CodexToolCallSnapshot {
    CodexToolCallSnapshot {
        phase: CodexToolCallPhase::Started,
        tool_call: ToolCall {
            id: Some(codex_file_change_tool_call_id(item_id, &change.path, index)),
            name: "file_change".to_string(),
            arguments: codex_file_change_arguments(change),
            result: None,
            parent_tool_use_id: None,
            diff_context: None,
            stats: None,
        },
    }
}

fn codex_file_change_completed_snapshot(
    tool_id: String,
    change: PendingCodexFileChange,
    status: Option<&str>,
) -> CodexToolCallSnapshot {
    let status_result = serde_json::json!({
        "status": status,
        "kind": change.kind,
    });

    let maybe_new_content = std::fs::read_to_string(&change.path).ok();
    let tool_call = match change.kind.as_str() {
        "update" => {
            if let Some(new_content) = maybe_new_content {
                if let Some(old_content) = change.old_content.clone() {
                    ToolCall {
                        id: Some(tool_id),
                        name: "edit".to_string(),
                        arguments: serde_json::json!({
                            "file_path": change.path,
                            "old_string": old_content,
                            "new_string": new_content,
                        }),
                        result: Some(status_result),
                        parent_tool_use_id: None,
                        diff_context: Some(DiffContext {
                            old_content: change.old_content,
                            old_file_exists: Some(true),
                            file_path: change.path,
                        }),
                        stats: None,
                    }
                } else {
                    ToolCall {
                        id: Some(tool_id),
                        name: "write".to_string(),
                        arguments: serde_json::json!({
                            "file_path": change.path,
                            "content": new_content,
                        }),
                        result: Some(status_result),
                        parent_tool_use_id: None,
                        diff_context: Some(DiffContext {
                            old_content: None,
                            old_file_exists: change.old_file_exists,
                            file_path: change.path,
                        }),
                        stats: None,
                    }
                }
            } else {
                ToolCall {
                    id: Some(tool_id),
                    name: "file_change".to_string(),
                    arguments: serde_json::json!({
                        "changes": [
                            {
                                "path": change.path,
                                "kind": change.kind,
                            }
                        ]
                    }),
                    result: Some(status_result),
                    parent_tool_use_id: None,
                    diff_context: None,
                    stats: None,
                }
            }
        }
        "add" | "create" => {
            if let Some(new_content) = maybe_new_content {
                ToolCall {
                    id: Some(tool_id),
                    name: "write".to_string(),
                    arguments: serde_json::json!({
                        "file_path": change.path,
                        "content": new_content,
                    }),
                    result: Some(status_result),
                    parent_tool_use_id: None,
                    diff_context: Some(DiffContext {
                        old_content: None,
                        old_file_exists: Some(false),
                        file_path: change.path,
                    }),
                    stats: None,
                }
            } else {
                ToolCall {
                    id: Some(tool_id),
                    name: "file_change".to_string(),
                    arguments: serde_json::json!({
                        "changes": [
                            {
                                "path": change.path,
                                "kind": change.kind,
                            }
                        ]
                    }),
                    result: Some(status_result),
                    parent_tool_use_id: None,
                    diff_context: None,
                    stats: None,
                }
            }
        }
        _ => ToolCall {
            id: Some(tool_id),
            name: "file_change".to_string(),
            arguments: serde_json::json!({
                "changes": [
                    {
                        "path": change.path,
                        "kind": change.kind,
                    }
                ]
            }),
            result: Some(status_result),
            parent_tool_use_id: None,
            diff_context: None,
            stats: None,
        },
    };

    CodexToolCallSnapshot {
        phase: CodexToolCallPhase::Completed,
        tool_call,
    }
}

fn resolve_codex_file_change_tool_call_snapshots(
    snapshot: CodexFileChangeSnapshot,
    pending_changes: &mut HashMap<String, PendingCodexFileChange>,
) -> Vec<CodexToolCallSnapshot> {
    snapshot
        .changes
        .into_iter()
        .enumerate()
        .map(|(index, change)| {
            let tool_id =
                codex_file_change_tool_call_id(snapshot.id.as_deref(), &change.path, index);
            match snapshot.phase {
                CodexToolCallPhase::Started => {
                    let (old_content, old_file_exists) = capture_file_diff_baseline(&change.path);
                    pending_changes.insert(
                        tool_id.clone(),
                        PendingCodexFileChange {
                            path: change.path.clone(),
                            kind: change.kind.clone(),
                            old_content,
                            old_file_exists,
                        },
                    );
                    codex_file_change_started_snapshot(snapshot.id.as_deref(), &change, index)
                }
                CodexToolCallPhase::Completed => {
                    let pending =
                        pending_changes
                            .remove(&tool_id)
                            .unwrap_or(PendingCodexFileChange {
                                path: change.path,
                                kind: change.kind,
                                old_content: None,
                                old_file_exists: None,
                            });
                    codex_file_change_completed_snapshot(
                        tool_id,
                        pending,
                        snapshot.status.as_deref(),
                    )
                }
            }
        })
        .collect()
}

fn agent_run_usage_from_codex_usage(usage: CodexUsage) -> AgentRunUsage {
    AgentRunUsage {
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cache_creation_tokens: None,
        cache_read_tokens: usage.cached_input_tokens,
        estimated_usd: None,
    }
}

fn cumulative_counter_decreased(previous: Option<u64>, current: Option<u64>) -> bool {
    matches!((previous, current), (Some(_), None))
        || matches!((previous, current), (Some(previous), Some(current)) if current < previous)
}

fn cumulative_cost_decreased(previous: Option<f64>, current: Option<f64>) -> bool {
    matches!((previous, current), (Some(_), None))
        || matches!((previous, current), (Some(previous), Some(current)) if current < previous)
}

fn cumulative_snapshot_decreased(
    previous: &ProviderUsageSnapshot,
    current: &ProviderUsageSnapshot,
) -> bool {
    cumulative_counter_decreased(previous.input_tokens, current.input_tokens)
        || cumulative_counter_decreased(previous.output_tokens, current.output_tokens)
        || cumulative_counter_decreased(
            previous.cache_creation_tokens,
            current.cache_creation_tokens,
        )
        || cumulative_counter_decreased(previous.cache_read_tokens, current.cache_read_tokens)
        || cumulative_cost_decreased(previous.estimated_usd, current.estimated_usd)
}

fn subtract_cumulative_counter(current: Option<u64>, previous: Option<u64>) -> Option<u64> {
    current.and_then(|value| value.checked_sub(previous.unwrap_or(0)))
}

fn subtract_cumulative_cost(current: Option<f64>, previous: Option<f64>) -> Option<f64> {
    current.map(|value| value - previous.unwrap_or(0.0))
}

#[doc(hidden)]
pub(crate) fn normalize_codex_cumulative_usage_for_persistence(
    current: AgentRunUsage,
    prior_runs: &[AgentRun],
    current_run_id: Option<&str>,
    provider_session_id: Option<&str>,
) -> Option<UsageCapture> {
    let provider_session_id = provider_session_id?;
    let raw_snapshot = ProviderUsageSnapshot::from_usage(current);
    let previous = prior_runs
        .iter()
        .filter(|run| run.harness == Some(AgentHarnessKind::Codex))
        .filter(|run| {
            current_run_id
                .map(|id| run.id.as_str() != id)
                .unwrap_or(true)
        })
        .filter(|run| run.provider_session_id.as_deref() == Some(provider_session_id))
        .filter_map(|run| {
            run.raw_usage_snapshot
                .as_ref()
                .map(|snapshot| (run, snapshot))
        })
        .max_by_key(|(run, _)| run.started_at);

    let Some((_, previous)) = previous else {
        return Some(UsageCapture::cumulative_baseline(raw_snapshot));
    };
    if cumulative_snapshot_decreased(previous, &raw_snapshot) {
        return Some(UsageCapture::cumulative_baseline(raw_snapshot));
    }

    Some(
        UsageCapture::normalized(
            AgentRunUsage {
                input_tokens: subtract_cumulative_counter(
                    raw_snapshot.input_tokens,
                    previous.input_tokens,
                ),
                output_tokens: subtract_cumulative_counter(
                    raw_snapshot.output_tokens,
                    previous.output_tokens,
                ),
                cache_creation_tokens: subtract_cumulative_counter(
                    raw_snapshot.cache_creation_tokens,
                    previous.cache_creation_tokens,
                ),
                cache_read_tokens: subtract_cumulative_counter(
                    raw_snapshot.cache_read_tokens,
                    previous.cache_read_tokens,
                ),
                estimated_usd: subtract_cumulative_cost(
                    raw_snapshot.estimated_usd,
                    previous.estimated_usd,
                ),
            },
            UsageProvenance::DerivedCumulativeDelta,
        )
        .with_raw_snapshot(raw_snapshot),
    )
}

async fn normalize_codex_stream_usage_for_persistence(
    event_usage: AgentRunUsage,
    source: CodexUsageSource,
    agent_run_repo: &Option<Arc<dyn AgentRunRepository>>,
    conversation_id: &ChatConversationId,
    agent_run_id: Option<&str>,
    provider_session_id: Option<&str>,
) -> Option<UsageCapture> {
    if source != CodexUsageSource::CumulativeTotal {
        return Some(UsageCapture::normalized(
            event_usage,
            UsageProvenance::ProviderTurnDelta,
        ));
    }

    let repo = agent_run_repo.as_ref()?;
    let provider_session_id = provider_session_id?;

    match repo.get_by_conversation(conversation_id).await {
        Ok(prior_runs) => normalize_codex_cumulative_usage_for_persistence(
            event_usage,
            &prior_runs,
            agent_run_id,
            Some(provider_session_id),
        ),
        Err(error) => {
            tracing::warn!(
                conversation_id = %conversation_id.as_str(),
                error = %error,
                "Failed to load prior Codex raw usage baseline; leaving usage uncounted"
            );
            None
        }
    }
}

/// Per-context-type timeout thresholds for stream processing.
///
/// Different agent contexts have different expected run durations.
/// Task execution needs generous timeouts for long-running commands,
/// while merge/review contexts should fail-fast on stalls.
#[derive(Debug, Clone)]
pub struct StreamTimeoutConfig {
    /// Max time to wait for a single line of stdout before killing the agent.
    pub line_read_timeout: Duration,
    /// Max time to tolerate stdout traffic with no parseable stream events.
    pub parse_stall_timeout: Duration,
}

impl StreamTimeoutConfig {
    /// Returns timeout thresholds appropriate for the given context type.
    pub fn for_context(context_type: &ChatContextType) -> Self {
        let cfg = stream_timeouts();
        match context_type {
            ChatContextType::Merge => Self {
                line_read_timeout: Duration::from_secs(cfg.merge_line_read_secs),
                parse_stall_timeout: Duration::from_secs(cfg.merge_parse_stall_secs),
            },
            ChatContextType::Review => Self {
                line_read_timeout: Duration::from_secs(cfg.review_line_read_secs),
                parse_stall_timeout: Duration::from_secs(cfg.review_parse_stall_secs),
            },
            // TaskExecution, Ideation, Task, Project — generous defaults
            _ => Self {
                line_read_timeout: Duration::from_secs(cfg.default_line_read_secs),
                parse_stall_timeout: Duration::from_secs(cfg.default_parse_stall_secs),
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct StreamOutcome {
    pub response_text: String,
    pub tool_calls: Vec<ToolCall>,
    pub content_blocks: Vec<ContentBlockItem>,
    pub session_id: Option<String>,
    pub usage: AgentRunUsage,
    pub usage_provenance: Option<UsageProvenance>,
    pub stderr_text: String,
    /// Number of turns fully finalized during interactive streaming
    /// (via `TurnComplete` events). When > 0 and `response_text` is empty,
    /// the post-loop caller should skip re-finalization and duplicate
    /// `run_completed` emission (or `turn_completed` in interactive mode).
    pub turns_finalized: usize,
    /// Whether this stream won the guarded Running -> Completed transition.
    pub completion_applied: bool,
    /// Whether the execution slot is still held when the stream exits.
    /// False when TurnComplete decremented the slot and no new message arrived
    /// to re-increment it (process was idle between turns at exit time).
    /// Used by the caller to prevent double-decrement in on_exit.
    pub execution_slot_held: bool,
    /// True when the stream observed the execution completion MCP tool before
    /// the provider process exited.
    pub completion_tool_called: bool,
    /// True when the process exited while idle between interactive turns.
    /// Suppresses queue processing and run_completed emission is forced.
    pub silent_interactive_exit: bool,
    /// The stream exited because its exact runtime was retired for a backend-owned
    /// mode handoff. This is deliberately distinct from a user cancellation: the
    /// background owner must still drain the durable replacement queue.
    pub mode_handoff_exit: bool,
}

impl StreamOutcome {
    pub fn has_meaningful_output(&self) -> bool {
        has_meaningful_output(
            &self.response_text,
            self.tool_calls.len(),
            &self.stderr_text,
        )
    }
}

/// Tracks the number of active subagent tasks (Task tool calls) in flight.
///
/// When the lead agent spawns sidechain subagents via the Task tool, its stdout
/// goes silent while the subagents work (their output goes to JSONL sidechain
/// files, not the lead's stdout). Without tracking, the stream timeout kills
/// the lead agent even though work is actively happening.
///
/// Incremented on `TaskStarted`, decremented on `TaskCompleted`.
/// The timeout handler checks `has_active_tasks()` to bypass the timeout.
#[derive(Debug, Default)]
#[doc(hidden)]
pub struct ActiveTaskTracker {
    count: usize,
}

impl ActiveTaskTracker {
    #[doc(hidden)]
    pub fn task_started(&mut self) {
        self.count += 1;
    }

    #[doc(hidden)]
    pub fn task_completed(&mut self) {
        self.count = self.count.saturating_sub(1);
    }

    #[doc(hidden)]
    pub fn has_active_tasks(&self) -> bool {
        self.count > 0
    }

    #[doc(hidden)]
    pub fn count(&self) -> usize {
        self.count
    }
}

/// Tracks whether a completion MCP tool has been called for this stream run.
///
/// Completion tools intentionally close stdin and enter a quiet shutdown window
/// where Claude may emit no more stdout before exiting. This tracker lets the
/// timeout logic bypass line-read and parse-stall kills briefly so the process
/// can exit naturally.
#[derive(Debug, Default)]
#[doc(hidden)]
pub struct CompletionSignalTracker {
    completion_called_at: Option<std::time::Instant>,
}

impl CompletionSignalTracker {
    #[doc(hidden)]
    pub fn mark_completion_called(&mut self) {
        self.completion_called_at = Some(std::time::Instant::now());
    }

    #[doc(hidden)]
    pub fn mark_completion_called_at(&mut self, now: std::time::Instant) {
        self.completion_called_at = Some(now);
    }

    #[doc(hidden)]
    pub fn was_called(&self) -> bool {
        self.completion_called_at.is_some()
    }

    #[doc(hidden)]
    pub fn is_in_grace_period(&self, grace_duration: std::time::Duration) -> bool {
        self.completion_called_at
            .map(|called_at| called_at.elapsed() < grace_duration)
            .unwrap_or(false)
    }
}

pub(super) fn completion_tool_result_accepted(result: Option<&serde_json::Value>) -> bool {
    let Some(result) = result else {
        return true;
    };
    if ["is_error", "isError"].iter().any(|key| {
        result
            .get(*key)
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    }) {
        return false;
    }
    if result
        .get("success")
        .and_then(|value| value.as_bool())
        .is_some_and(|success| !success)
    {
        return false;
    }
    if let Some(status) = result.get("status").and_then(|value| value.as_str()) {
        let status = status.to_ascii_lowercase();
        if matches!(status.as_str(), "error" | "failed" | "failure") {
            return false;
        }
    }
    true
}

// ============================================================================
// Background stream processing
// ============================================================================

/// Process stream output in background, emitting events and persisting activity events
///
/// # Arguments
/// * `child` - The spawned Claude CLI process
/// * `context_type` - The chat context type
/// * `context_id` - The context ID (task_id, project_id, etc.)
/// * `conversation_id` - The conversation ID
/// * `app_handle` - Tauri app handle for events
/// * `activity_event_repo` - Repository for persisting activity events (optional)
/// * `task_repo` - Task repository for fetching current status (optional)
/// * `chat_message_repo` - Chat message repository for incremental persistence (optional)
/// * `chat_timeline_repo` - Normalized timeline repository for live visible block persistence (optional)
/// * `assistant_message_id` - Pre-created assistant message ID for incremental updates (optional)
/// * `question_state` - QuestionState for checking pending questions (optional)
/// * `streaming_state_cache` - Cache for streaming state to hydrate frontend on navigation
pub async fn process_stream_background<R: Runtime>(
    mut child: tokio::process::Child,
    harness: AgentHarnessKind,
    context_type: ChatContextType,
    context_id: &str,
    conversation_id: &ChatConversationId,
    app_handle: Option<AppHandle<R>>,
    activity_event_repo: Option<Arc<dyn ActivityEventRepository>>,
    task_repo: Option<Arc<dyn TaskRepository>>,
    chat_message_repo: Option<Arc<dyn ChatMessageRepository>>,
    chat_timeline_repo: Option<Arc<dyn ChatTimelineRepository>>,
    mut assistant_message_id: Option<String>,
    question_state: Option<Arc<QuestionState>>,
    cancellation_token: CancellationToken,
    streaming_state_cache: StreamingStateCache,
    running_agent_registry: Option<Arc<dyn RunningAgentRegistry>>,
    agent_run_repo: Option<Arc<dyn AgentRunRepository>>,
    agent_run_id: Option<String>,
    execution_state: Option<Arc<crate::commands::ExecutionState>>,
    conversation_repo: Option<Arc<dyn ChatConversationRepository>>,
    split_verification_transcript: bool,
    persist_conversation_provider_session_ref: bool,
    interactive_process_registry: Option<Arc<InteractiveProcessRegistry>>,
    interactive_process_key: Option<InteractiveProcessKey>,
    interactive_process_token: Option<InteractiveProcessToken>,
) -> Result<StreamOutcome, StreamError> {
    streaming_state_cache
        .set_run_id(&conversation_id.as_str(), agent_run_id.clone())
        .await;
    if stream_mode_for_harness(harness) == HarnessStreamMode::CodexJsonl {
        return process_codex_stream_background(
            child,
            context_type,
            context_id,
            conversation_id,
            app_handle,
            activity_event_repo,
            task_repo,
            chat_message_repo,
            chat_timeline_repo,
            assistant_message_id,
            question_state,
            cancellation_token,
            streaming_state_cache,
            running_agent_registry,
            agent_run_repo,
            agent_run_id,
            execution_state,
            conversation_repo,
            split_verification_transcript,
            persist_conversation_provider_session_ref,
        )
        .await;
    }

    let timeout_config = StreamTimeoutConfig::for_context(&context_type);
    let stream_cfg = stream_timeouts();
    tracing::debug!(
        conversation_id = conversation_id.as_str(),
        %context_type,
        context_id,
        line_read_timeout_secs = timeout_config.line_read_timeout.as_secs(),
        parse_stall_timeout_secs = timeout_config.parse_stall_timeout.as_secs(),
        "process_stream_background start"
    );
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| StreamError::ProcessSpawnFailed {
            command: "claude".to_string(),
            error: "Failed to capture stdout".to_string(),
        })?;

    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| StreamError::ProcessSpawnFailed {
            command: "claude".to_string(),
            error: "Failed to capture stderr".to_string(),
        })?;

    let event_ctx = event_context(conversation_id, &context_type, context_id);
    let conversation_id_str = event_ctx.conversation_id.clone();
    let context_type_str = event_ctx.context_type.clone();
    let context_id_str = event_ctx.context_id.clone();
    let debug_path = crate::utils::runtime_log_paths::stream_debug_log_file(&conversation_id_str);
    tracing::debug!(
        path = %debug_path.display(),
        "Debug log path (written on parse failure)"
    );

    // Parse task_id for activity persistence (for TaskExecution and Merge contexts).
    // Merge context uses the task_id as context_id, so the mapping is identical.
    let task_id_for_persistence = if matches!(
        context_type,
        ChatContextType::TaskExecution | ChatContextType::Merge
    ) {
        Some(TaskId::from_string(context_id.to_string()))
    } else {
        None
    };

    // Spawn stderr reader
    let _stderr_handle = app_handle.clone();
    let _stderr_conv_id = conversation_id_str.clone();
    let stderr_task = tokio::spawn(async move {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();
        let mut stderr_content = String::new();

        while let Ok(Some(line)) = lines.next_line().await {
            stderr_content.push_str(&line);
            stderr_content.push('\n');
        }

        stderr_content
    });

    // Process stdout
    let stdout_reader = BufReader::new(stdout);
    let mut lines = stdout_reader.lines();
    let mut processor = StreamProcessor::new();
    let mut debug_lines: Vec<String> = Vec::new();
    let mut lines_seen: usize = 0;
    let mut lines_parsed: usize = 0;
    let mut stream_seq: u64 = 0;
    let mut last_parsed_at = std::time::Instant::now();
    // Activity-aware idle cap: kill after max_wall_clock_secs of no meaningful activity
    let stream_start = std::time::Instant::now();
    let max_wall_clock = std::time::Duration::from_secs(stream_cfg.max_wall_clock_secs);
    let mut last_activity_at = std::time::Instant::now();
    let completion_grace_duration =
        std::time::Duration::from_secs(stream_cfg.completion_grace_secs);

    // Keep persistence rate-bounded while leaving streaming cache updates and UI events
    // unthrottled. Terminal and content-boundary flushes make this only a tuning knob.
    let streaming_persistence_debounce =
        std::time::Duration::from_millis(stream_cfg.streaming_persistence_debounce_ms);
    let mut streaming_persistence_dirty = false;
    let mut last_streaming_persisted_at = std::time::Instant::now()
        .checked_sub(streaming_persistence_debounce)
        .unwrap_or_else(std::time::Instant::now);
    const USAGE_FLUSH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);
    let mut last_usage_flush = std::time::Instant::now();

    // Throttled heartbeat: update last_active_at every 5s on any parsed event
    let heartbeat_key = running_agent_registry
        .as_ref()
        .map(|_| RunningAgentKey::new(context_type.to_string(), context_id));
    let mut last_heartbeat = std::time::Instant::now();
    const HEARTBEAT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

    // Track active subagent tasks (Task tool calls) to prevent timeout during sidechain work.
    // When the lead spawns in-process subagents, stdout goes silent — this tracker
    // lets the timeout handler know work is still happening.
    let mut active_task_tracker = ActiveTaskTracker::default();
    let mut completion_signal_tracker = CompletionSignalTracker::default();
    let mut last_emitted_usage = AgentRunUsage::default();

    // Count of turns fully finalized in the loop (interactive mode).
    // Used to tell the caller whether post-loop finalization should be skipped.
    let mut turns_finalized: usize = 0;
    let mut completion_applied_for_stream = false;

    // When true, the process is legitimately idle between interactive turns
    // (TurnComplete received, waiting for next stdin message). The timeout
    // handler should kill silently instead of returning an error.
    let mut between_interactive_turns: bool = false;
    // Set to true when an interactive process is killed while idle between
    // turns. Suppresses post-loop error returns so the exit is silent.
    let mut silent_interactive_exit: bool = false;
    let mut mode_handoff_exit: bool = false;
    // Track whether we've already persisted session_id to the DB (only need once)
    let mut session_id_persisted: bool = false;

    loop {
        // Race line-read (with timeout) against cancellation token
        let line = tokio::select! {
            biased;
            _ = cancellation_token.cancelled() => {
                let mode_handoff_armed = if let (Some(registry), Some(key), Some(token), Some(run_id)) = (
                    interactive_process_registry.as_ref(),
                    interactive_process_key.as_ref(),
                    interactive_process_token,
                    agent_run_id.as_deref(),
                ) {
                    is_armed_mode_handoff_disposition(
                        registry.retire_after_turn_disposition_if_owner(key, token, run_id).await,
                    )
                } else {
                    false
                };
                if mode_handoff_armed {
                    tracing::info!(
                        conversation_id = %conversation_id_str,
                        lines_seen,
                        "Stream cancellation is an exact mode-handoff retirement"
                    );
                    let _ = child.kill().await;
                    silent_interactive_exit = true;
                    mode_handoff_exit = true;
                    break;
                }
                if between_interactive_turns {
                    tracing::info!(
                        conversation_id = %conversation_id_str,
                        lines_seen,
                        "Interactive process idle between turns — silent exit on cancellation"
                    );
                    let _ = child.kill().await;
                    silent_interactive_exit = true;
                    break;
                }
                tracing::info!(
                    conversation_id = %conversation_id_str,
                    lines_seen,
                    "Stream cancelled via cancellation token, killing agent"
                );
                let _ = child.kill().await;
                flush_streaming_persistence_if_dirty(
                    &mut streaming_persistence_dirty,
                    &mut last_streaming_persisted_at,
                    &chat_message_repo,
                    &chat_timeline_repo,
                    &conversation_id_str,
                    &assistant_message_id,
                    &processor.response_text,
                    &processor.tool_calls,
                    &processor.content_blocks,
                    ChatTimelineItemStatus::Error,
                )
                .await;
                flush_content_before_error(
                    &chat_message_repo, &assistant_message_id,
                    &processor.response_text, &processor.tool_calls, &processor.content_blocks,
                ).await;
                persist_timeline_snapshot(
                    &chat_timeline_repo,
                    &conversation_id_str,
                    &assistant_message_id,
                    &processor.content_blocks,
                    ChatTimelineItemStatus::Error,
                ).await;
                return Err(StreamError::Cancelled {
                    turns_finalized,
                    completion_tool_called: completion_signal_tracker.was_called(),
                });
            }
            read_result = timeout(timeout_config.line_read_timeout, lines.next_line()) => {
                match read_result {
                    Ok(Ok(Some(line))) => line,
                    Ok(Ok(None)) => {
                        tracing::info!(
                            conversation_id = %conversation_id_str,
                            context_id,
                            lines_seen,
                            lines_parsed,
                            between_interactive_turns,
                            "[STREAM_EOF] stdout closed — process exited"
                        );
                        // Process exited between interactive turns — treat as
                        // normal completion, same as cancellation-token path.
                        if between_interactive_turns {
                            silent_interactive_exit = true;
                        }
                        break;
                    }
                    Ok(Err(e)) => {
                        tracing::error!(
                            conversation_id = %conversation_id_str,
                            error = %e,
                            "Stream read error"
                        );
                        if between_interactive_turns {
                            silent_interactive_exit = true;
                        }
                        break;
                    }
                    Err(_) => {
                        // Timeout — no output for configured timeout seconds

                        // Gather state for kill decision (async state first)
                        let has_pending_question = if let Some(ref qs) = question_state {
                            qs.has_pending_for_session(context_id).await
                        } else {
                            false
                        };
                        let (pid_alive, child_exited) = if let Some(pid) = child.id() {
                            let exited = child.try_wait().ok().flatten().is_some();
                            let alive = crate::domain::services::is_process_alive(pid);
                            (alive, exited)
                        } else {
                            (false, true)
                        };
                        let is_completion_grace_period = completion_signal_tracker
                            .is_in_grace_period(completion_grace_duration);

                        if should_kill_on_timeout(
                            last_activity_at.elapsed(),
                            max_wall_clock,
                            has_pending_question,
                            between_interactive_turns,
                            pid_alive,
                            child_exited,
                            active_task_tracker.has_active_tasks(),
                            is_completion_grace_period,
                        ) {
                            if last_activity_at.elapsed() > max_wall_clock {
                                tracing::warn!(
                                    conversation_id = %conversation_id_str,
                                    idle_secs = last_activity_at.elapsed().as_secs(),
                                    total_secs = stream_start.elapsed().as_secs(),
                                    "Idle cap reached — killing agent"
                                );
                            }
                            tracing::warn!(
                                conversation_id = %conversation_id_str,
                                lines_seen,
                                lines_parsed,
                                "Stream timeout: no output for {} seconds, killing agent",
                                timeout_config.line_read_timeout.as_secs()
                            );
                            if completion_signal_tracker.was_called() {
                                tracing::warn!(
                                    conversation_id = %conversation_id_str,
                                    context_id,
                                    grace_secs = completion_grace_duration.as_secs(),
                                    "Completion grace period expired after completion tool call, proceeding with kill"
                                );
                            }
                            let _ = child.kill().await;
                            flush_content_before_error(
                                &chat_message_repo, &assistant_message_id,
                                &processor.response_text, &processor.tool_calls, &processor.content_blocks,
                            ).await;
                            persist_timeline_snapshot(
                                &chat_timeline_repo,
                                &conversation_id_str,
                                &assistant_message_id,
                                &processor.content_blocks,
                                ChatTimelineItemStatus::Error,
                            ).await;
                            return Err(StreamError::Timeout {
                                context_type,
                                elapsed_secs: timeout_config.line_read_timeout.as_secs(),
                            });
                        } else if has_pending_question {
                            tracing::info!(
                                conversation_id = %conversation_id_str,
                                context_id,
                                lines_seen,
                                "Stream no output but pending question exists, resetting timeout"
                            );
                            continue;
                        } else if between_interactive_turns {
                            // Interactive mode: process is idle between turns. Kill
                            // silently and exit as a normal completion — not an error.
                            tracing::info!(
                                conversation_id = %conversation_id_str,
                                context_id,
                                lines_seen,
                                timeout_secs = timeout_config.line_read_timeout.as_secs(),
                                "Interactive process idle between turns — silent exit on timeout"
                            );
                            let _ = child.kill().await;
                            silent_interactive_exit = true;
                            break;
                        } else if pid_alive && !child_exited {
                            // PID-alive bypass: subprocess is running but stdout is buffered
                            // (e.g., cargo test | tail). Only bypass when wall-clock not exceeded.
                            if let Some(pid) = child.id() {
                                tracing::info!(
                                    conversation_id = %conversation_id_str,
                                    context_id,
                                    pid,
                                    lines_seen,
                                    "Stream timeout but child process alive — resetting"
                                );
                                emit_heartbeat(
                                    &app_handle,
                                    &conversation_id_str,
                                    context_id,
                                    "pid_alive_bypass",
                                    Some(serde_json::json!({ "pid": pid })),
                                );
                            }
                            continue;
                        } else if active_task_tracker.has_active_tasks() {
                            // Active tasks bypass: subagent tasks active (sidechain work in progress).
                            // Lead stdout goes silent while Task tool subagents work — their
                            // output goes to JSONL sidechain files, not the lead's stdout.
                            let active_count = active_task_tracker.count();
                            tracing::info!(
                                conversation_id = %conversation_id_str,
                                context_id,
                                lines_seen,
                                active_tasks = active_count,
                                "Stream no output but {} active subagent task(s), resetting timeout",
                                active_count
                            );
                            emit_heartbeat(
                                &app_handle,
                                &conversation_id_str,
                                context_id,
                                "active_tasks_bypass",
                                Some(serde_json::json!({ "active_tasks": active_count })),
                            );
                            continue;
                        } else {
                            tracing::info!(
                                conversation_id = %conversation_id_str,
                                context_id,
                                lines_seen,
                                grace_secs = completion_grace_duration.as_secs(),
                                "Stream no output after completion tool call, staying in shutdown grace period"
                            );
                            continue;
                        }
                    }
                }
            }
        };

        // New output arrived — we're no longer idle between turns.
        between_interactive_turns = false;

        lines_seen += 1;
        if debug_lines.len() < 50 {
            debug_lines.push(line.clone());
        }

        // [STREAM_RAW] Log every raw stdout line for team message debugging
        tracing::debug!(
            conversation_id = %conversation_id_str,
            lines_seen,
            line_len = line.len(),
            line_preview = %truncate_str(&line, 200),
            "[STREAM_RAW] Lead stdout line"
        );

        if let Some(parsed) = StreamProcessor::parse_line(&line) {
            lines_parsed += 1;
            last_parsed_at = std::time::Instant::now();
            last_activity_at = std::time::Instant::now();

            // [STREAM_MSG] Log parsed message variant
            tracing::debug!(
                conversation_id = %conversation_id_str,
                lines_parsed,
                msg_type = %format!("{:?}", &parsed.message).chars().take(80).collect::<String>(),
                has_parent = parsed.parent_tool_use_id.is_some(),
                is_synthetic = parsed.is_synthetic,
                has_tool_use_result = parsed.tool_use_result.is_some(),
                "[STREAM_MSG] Parsed stream message"
            );

            let stream_events = processor.process_parsed_line(parsed);

            for event in stream_events {
                // [STREAM_EVT] Log every stream event for team message debugging
                match &event {
                    StreamEvent::TextChunk(text) => {
                        tracing::debug!(
                            conversation_id = %conversation_id_str,
                            text_len = text.len(),
                            text_preview = %text.chars().take(100).collect::<String>(),
                            "[STREAM_EVT] TextChunk"
                        );
                    }
                    StreamEvent::ToolCallStarted { name, id, .. } => {
                        tracing::debug!(
                            conversation_id = %conversation_id_str,
                            tool_name = %name,
                            tool_id = ?id,
                            "[STREAM_EVT] ToolCallStarted"
                        );
                    }
                    StreamEvent::ToolCallCompleted { tool_call, .. } => {
                        tracing::debug!(
                            conversation_id = %conversation_id_str,
                            tool_name = %tool_call.name,
                            tool_id = ?tool_call.id,
                            "[STREAM_EVT] ToolCallCompleted"
                        );
                    }
                    StreamEvent::TurnComplete { session_id } => {
                        tracing::info!(
                            conversation_id = %conversation_id_str,
                            ?session_id,
                            response_text_len = processor.response_text.len(),
                            tool_calls_count = processor.tool_calls.len(),
                            content_blocks_count = processor.content_blocks.len(),
                            "[STREAM_EVT] TurnComplete — accumulated content summary"
                        );
                    }
                    StreamEvent::TaskStarted {
                        tool_use_id,
                        description,
                        ..
                    } => {
                        tracing::debug!(
                            conversation_id = %conversation_id_str,
                            tool_use_id = %tool_use_id,
                            description = ?description,
                            "[STREAM_EVT] TaskStarted"
                        );
                    }
                    StreamEvent::TaskCompleted {
                        tool_use_id,
                        agent_id,
                        ..
                    } => {
                        tracing::debug!(
                            conversation_id = %conversation_id_str,
                            tool_use_id = %tool_use_id,
                            agent_id = ?agent_id,
                            "[STREAM_EVT] TaskCompleted"
                        );
                    }
                    _ => {
                        tracing::debug!(
                            conversation_id = %conversation_id_str,
                            event_type = %format!("{:?}", &event).chars().take(60).collect::<String>(),
                            "[STREAM_EVT] Other event"
                        );
                    }
                }

                // Lazily create assistant message on first content-producing event
                if assistant_message_id.is_none()
                    && matches!(
                        event,
                        StreamEvent::TextChunk(_)
                            | StreamEvent::Thinking(_)
                            | StreamEvent::ToolCallStarted { .. }
                    )
                {
                    if let Some(ref repo) = chat_message_repo {
                        let msg = super::chat_service_context::create_assistant_message(
                            context_type,
                            context_id,
                            "",
                            conversation_id.clone(),
                            &[],
                            &[],
                        );
                        let new_id = msg.id.as_str().to_string();
                        let _ = repo.create(msg).await;
                        tracing::debug!(
                            conversation_id = %conversation_id_str,
                            assistant_message_id = %new_id,
                            "[STREAM_EVT] Created new assistant message"
                        );
                        assistant_message_id = Some(new_id);
                    }
                }

                match event {
                    StreamEvent::TextChunk(text) => {
                        let block_position = current_text_block_position(&processor.content_blocks);
                        // Update streaming state cache
                        streaming_state_cache
                            .append_text(&conversation_id_str, block_position as usize, &text)
                            .await;

                        streaming_persistence_dirty = true;
                        if last_streaming_persisted_at.elapsed() >= streaming_persistence_debounce {
                            flush_streaming_persistence_if_dirty(
                                &mut streaming_persistence_dirty,
                                &mut last_streaming_persisted_at,
                                &chat_message_repo,
                                &chat_timeline_repo,
                                &conversation_id_str,
                                &assistant_message_id,
                                &processor.response_text,
                                &processor.tool_calls,
                                &processor.content_blocks,
                                ChatTimelineItemStatus::Streaming,
                            )
                            .await;
                        }

                        if let Some(ref handle) = app_handle {
                            // Unified event
                            let _ = handle.emit(
                                events::AGENT_CHUNK,
                                AgentChunkPayload {
                                    text: text.clone(),
                                    run_id: agent_run_id.clone(),
                                    block_index: Some(block_position),
                                    conversation_id: conversation_id_str.clone(),
                                    context_type: context_type_str.clone(),
                                    context_id: context_id_str.clone(),
                                    seq: stream_seq,
                                    append_to_previous: true,
                                },
                            );
                            stream_seq += 1;

                            // Activity stream event for task execution and merge
                            if matches!(
                                context_type,
                                ChatContextType::TaskExecution | ChatContextType::Merge
                            ) {
                                let _ = handle.emit(
                                    events::AGENT_MESSAGE,
                                    serde_json::json!({
                                        "taskId": context_id_str,
                                        "type": "text",
                                        "content": text,
                                        "timestamp": chrono::Utc::now().timestamp_millis(),
                                    }),
                                );

                                // Persist activity event to database
                                if let (Some(ref repo), Some(ref task_id)) =
                                    (&activity_event_repo, &task_id_for_persistence)
                                {
                                    let event = ActivityEvent::new_task_event(
                                        task_id.clone(),
                                        ActivityEventType::Text,
                                        text.clone(),
                                    );
                                    // Fetch current task status and add to event
                                    let event = if let Some(ref t_repo) = task_repo {
                                        if let Ok(Some(task)) = t_repo.get_by_id(task_id).await {
                                            event.with_status(task.internal_status)
                                        } else {
                                            event
                                        }
                                    } else {
                                        event
                                    };
                                    let _ = repo.save(event).await;
                                }
                            }
                        }
                    }
                    StreamEvent::Thinking(text) => {
                        let block_position = current_text_block_position(&processor.content_blocks);
                        streaming_state_cache
                            .append_thinking(&conversation_id_str, block_position as usize, &text)
                            .await;
                        if let Some(ref handle) = app_handle {
                            let _ = handle.emit(
                                events::AGENT_THINKING,
                                AgentThinkingPayload {
                                    text: text.clone(),
                                    run_id: agent_run_id.clone(),
                                    block_index: Some(block_position),
                                    conversation_id: conversation_id_str.clone(),
                                    context_type: context_type_str.clone(),
                                    context_id: context_id_str.clone(),
                                    seq: stream_seq,
                                    append_to_previous: true,
                                },
                            );
                            stream_seq += 1;
                        }
                        flush_streaming_persistence_if_dirty(
                            &mut streaming_persistence_dirty,
                            &mut last_streaming_persisted_at,
                            &chat_message_repo,
                            &chat_timeline_repo,
                            &conversation_id_str,
                            &assistant_message_id,
                            &processor.response_text,
                            &processor.tool_calls,
                            &processor.content_blocks,
                            ChatTimelineItemStatus::Streaming,
                        )
                        .await;
                        // Activity stream event for task execution and merge
                        if matches!(
                            context_type,
                            ChatContextType::TaskExecution | ChatContextType::Merge
                        ) {
                            if let Some(ref handle) = app_handle {
                                let _ = handle.emit(
                                    events::AGENT_MESSAGE,
                                    serde_json::json!({
                                        "taskId": context_id_str,
                                        "type": "thinking",
                                        "content": text,
                                        "timestamp": chrono::Utc::now().timestamp_millis(),
                                    }),
                                );
                            }

                            // Persist activity event to database
                            if let (Some(ref repo), Some(ref task_id)) =
                                (&activity_event_repo, &task_id_for_persistence)
                            {
                                let event = ActivityEvent::new_task_event(
                                    task_id.clone(),
                                    ActivityEventType::Thinking,
                                    text.clone(),
                                );
                                // Fetch current task status and add to event
                                let event = if let Some(ref t_repo) = task_repo {
                                    if let Ok(Some(task)) = t_repo.get_by_id(task_id).await {
                                        event.with_status(task.internal_status)
                                    } else {
                                        event
                                    }
                                } else {
                                    event
                                };
                                let _ = repo.save(event).await;
                            }
                        }
                    }
                    StreamEvent::ThinkingProgress {
                        estimated_tokens,
                        estimated_tokens_delta,
                    } => {
                        if let Some(ref handle) = app_handle {
                            let _ = handle.emit(
                                events::AGENT_THINKING_PROGRESS,
                                AgentThinkingProgressPayload {
                                    estimated_tokens,
                                    estimated_tokens_delta,
                                    run_id: agent_run_id.clone(),
                                    conversation_id: conversation_id_str.clone(),
                                    context_type: context_type_str.clone(),
                                    context_id: context_id_str.clone(),
                                },
                            );
                        }
                    }
                    StreamEvent::ToolCallStarted {
                        name,
                        id,
                        parent_tool_use_id,
                    } => {
                        flush_streaming_persistence_if_dirty(
                            &mut streaming_persistence_dirty,
                            &mut last_streaming_persisted_at,
                            &chat_message_repo,
                            &chat_timeline_repo,
                            &conversation_id_str,
                            &assistant_message_id,
                            &processor.response_text,
                            &processor.tool_calls,
                            &processor.content_blocks,
                            ChatTimelineItemStatus::Streaming,
                        )
                        .await;
                        // Update streaming state cache with started tool call
                        let cached_tool = CachedToolCall {
                            id: id.clone().unwrap_or_default(),
                            name: name.clone(),
                            arguments: serde_json::Value::Null,
                            result: None,
                            diff_context: None,
                            parent_tool_use_id: parent_tool_use_id.clone(),
                        };
                        streaming_state_cache
                            .upsert_tool_call(&conversation_id_str, cached_tool)
                            .await;

                        if let Some(ref handle) = app_handle {
                            let _ = handle.emit(
                                events::AGENT_TOOL_CALL,
                                AgentToolCallPayload {
                                    tool_name: name.clone(),
                                    tool_id: id.clone(),
                                    arguments: serde_json::Value::Null,
                                    result: None,
                                    run_id: agent_run_id.clone(),
                                    preview: AgentToolCallPreviewFields::default(),
                                    conversation_id: conversation_id_str.clone(),
                                    context_type: context_type_str.clone(),
                                    context_id: context_id_str.clone(),
                                    diff_context: None,
                                    parent_tool_use_id,
                                    seq: stream_seq,
                                },
                            );
                            stream_seq += 1;
                        }
                    }
                    StreamEvent::ToolCallCompleted {
                        mut tool_call,
                        parent_tool_use_id,
                    } => {
                        // Capture old file content for Edit/Write tool calls
                        let name_lower = tool_call.name.to_lowercase();
                        if name_lower == "edit" || name_lower == "write" {
                            if let Some(file_path) = tool_call
                                .arguments
                                .get("file_path")
                                .and_then(|v| v.as_str())
                            {
                                let (old_content, old_file_exists) =
                                    capture_file_diff_baseline(file_path);
                                let diff_ctx = DiffContext {
                                    old_content,
                                    old_file_exists,
                                    file_path: file_path.to_string(),
                                };
                                tool_call.diff_context = Some(diff_ctx.clone());

                                // Update processor's stored tool_call and content_block
                                // (they were pushed before this event was emitted)
                                if let Some(last_tc) = processor.tool_calls.last_mut() {
                                    last_tc.diff_context = Some(diff_ctx.clone());
                                }
                                if let Some(ContentBlockItem::ToolUse { diff_context, .. }) =
                                    processor.content_blocks.last_mut()
                                {
                                    *diff_context = serde_json::to_value(&diff_ctx).ok();
                                }
                            }
                        }

                        let diff_context_value = tool_call
                            .diff_context
                            .as_ref()
                            .and_then(|dc| serde_json::to_value(dc).ok());
                        let argument_preview =
                            assistant_message_id.as_deref().and_then(|message_id| {
                                let detail_ref = tool_detail_ref(
                                    &conversation_id_str,
                                    message_id,
                                    tool_call.id.as_deref(),
                                    None,
                                );
                                build_live_tool_argument_preview(
                                    &tool_call,
                                    diff_context_value.as_ref(),
                                    Some(detail_ref),
                                )
                            });
                        if argument_preview.is_some() {
                            persist_assistant_message_snapshot(
                                &chat_message_repo,
                                &assistant_message_id,
                                &processor.response_text,
                                &processor.tool_calls,
                                &processor.content_blocks,
                            )
                            .await;
                            persist_timeline_snapshot(
                                &chat_timeline_repo,
                                &conversation_id_str,
                                &assistant_message_id,
                                &processor.content_blocks,
                                ChatTimelineItemStatus::Streaming,
                            )
                            .await;
                        }

                        // Update streaming state cache with completed tool call
                        let cached_tool = CachedToolCall {
                            id: tool_call.id.clone().unwrap_or_default(),
                            name: tool_call.name.clone(),
                            arguments: tool_call.arguments.clone(),
                            result: None,
                            diff_context: diff_context_value.clone(),
                            parent_tool_use_id: parent_tool_use_id.clone(),
                        };
                        streaming_state_cache
                            .upsert_tool_call(&conversation_id_str, cached_tool)
                            .await;

                        if let Some(ref handle) = app_handle {
                            let _ = handle.emit(
                                events::AGENT_TOOL_CALL,
                                AgentToolCallPayload::from_completed_tool_call(
                                    &tool_call,
                                    None,
                                    argument_preview.as_ref(),
                                    &conversation_id_str,
                                    &context_type_str,
                                    &context_id_str,
                                    agent_run_id.as_deref(),
                                    diff_context_value,
                                    parent_tool_use_id.clone(),
                                    stream_seq,
                                ),
                            );
                            stream_seq += 1;

                            // Activity stream event for task execution and merge
                            if matches!(
                                context_type,
                                ChatContextType::TaskExecution | ChatContextType::Merge
                            ) {
                                let tool_content = format!(
                                    "{} ({})",
                                    tool_call.name,
                                    serde_json::to_string(&tool_call.arguments).unwrap_or_default()
                                );
                                let tool_metadata = serde_json::json!({
                                    "tool_name": tool_call.name,
                                    "arguments": tool_call.arguments,
                                });

                                let _ = handle.emit(
                                    events::AGENT_MESSAGE,
                                    serde_json::json!({
                                        "taskId": context_id_str,
                                        "type": "tool_call",
                                        "content": tool_content,
                                        "timestamp": chrono::Utc::now().timestamp_millis(),
                                        "metadata": tool_metadata,
                                    }),
                                );

                                // Persist activity event to database
                                if let (Some(ref repo), Some(ref task_id)) =
                                    (&activity_event_repo, &task_id_for_persistence)
                                {
                                    let event = ActivityEvent::new_task_event(
                                        task_id.clone(),
                                        ActivityEventType::ToolCall,
                                        tool_content,
                                    )
                                    .with_metadata(tool_metadata.to_string());
                                    // Fetch current task status and add to event
                                    let event = if let Some(ref t_repo) = task_repo {
                                        if let Ok(Some(task)) = t_repo.get_by_id(task_id).await {
                                            event.with_status(task.internal_status)
                                        } else {
                                            event
                                        }
                                    } else {
                                        event
                                    };
                                    let _ = repo.save(event).await;
                                }
                            }
                        }
                    }
                    StreamEvent::SessionId(_) => {
                        // Captured in processor.finish()
                    }
                    StreamEvent::TurnComplete { session_id } => {
                        flush_streaming_persistence_if_dirty(
                            &mut streaming_persistence_dirty,
                            &mut last_streaming_persisted_at,
                            &chat_message_repo,
                            &chat_timeline_repo,
                            &conversation_id_str,
                            &assistant_message_id,
                            &processor.response_text,
                            &processor.tool_calls,
                            &processor.content_blocks,
                            ChatTimelineItemStatus::Streaming,
                        )
                        .await;
                        tracing::info!(
                            conversation_id = %conversation_id_str,
                            ?session_id,
                            "TurnComplete: finalizing assistant message for interactive turn"
                        );

                        let accepted_completion_diagnostic = if processor.result_is_error
                            && completion_signal_tracker.was_called()
                        {
                            let error_msg = if !processor.result_errors.is_empty() {
                                processor.result_errors.join("; ")
                            } else if !processor.response_text.trim().is_empty() {
                                processor.response_text.trim().to_string()
                            } else {
                                "Agent failed during execution".to_string()
                            };
                            super::chat_service_errors::classify_provider_error(&error_msg)
                                .is_none()
                        } else {
                            false
                        };

                        if accepted_completion_diagnostic {
                            tracing::warn!(
                                conversation_id = %conversation_id_str,
                                ?session_id,
                                "TurnComplete carried a non-provider error after accepted completion; treating it as post-completion diagnostic noise"
                            );
                        } else if processor.result_is_error {
                            tracing::warn!(
                                conversation_id = %conversation_id_str,
                                ?session_id,
                                "TurnComplete carried a result error; terminating interactive turn immediately"
                            );

                            flush_content_before_error(
                                &chat_message_repo,
                                &assistant_message_id,
                                &processor.response_text,
                                &processor.tool_calls,
                                &processor.content_blocks,
                            )
                            .await;
                            persist_timeline_snapshot(
                                &chat_timeline_repo,
                                &conversation_id_str,
                                &assistant_message_id,
                                &processor.content_blocks,
                                ChatTimelineItemStatus::Error,
                            )
                            .await;
                            if let Some(capture) = processor.current_turn_capture() {
                                let persisted = persist_usage_capture_run_first(
                                    &agent_run_repo,
                                    &agent_run_id,
                                    &chat_message_repo,
                                    &assistant_message_id,
                                    &capture,
                                )
                                .await;
                                if persisted {
                                    emit_usage_updated_event(
                                        &app_handle,
                                        &conversation_id_str,
                                        &context_type_str,
                                        &context_id_str,
                                    );
                                }
                            }

                            if !session_id_persisted && persist_conversation_provider_session_ref {
                                if let (Some(ref sess_id), Some(ref repo)) =
                                    (&session_id, &conversation_repo)
                                {
                                    let session_ref =
                                        provider_session_ref_for_harness(harness, sess_id.clone());
                                    if let Err(e) = repo
                                        .update_provider_session_ref(conversation_id, &session_ref)
                                        .await
                                    {
                                        tracing::error!(
                                            error = %e,
                                            conversation_id = %conversation_id_str,
                                            session_id = %sess_id,
                                            "TurnComplete error: failed to persist provider_session_ref"
                                        );
                                    }
                                }
                            }

                            let error_msg = if !processor.result_errors.is_empty() {
                                processor.result_errors.join("; ")
                            } else if !processor.response_text.trim().is_empty() {
                                processor.response_text.trim().to_string()
                            } else {
                                "Agent failed during execution".to_string()
                            };
                            let provider_error =
                                super::chat_service_errors::classify_provider_error(&error_msg);
                            let _ = child.start_kill();
                            stderr_task.abort();
                            if let Some(provider_err) = provider_error {
                                return Err(provider_err);
                            }
                            return Err(StreamError::AgentExit {
                                exit_code: None,
                                stderr: error_msg,
                            });
                        }

                        // Finalize the current assistant message with accumulated content
                        let assistant_message_persisted = if let (
                            Some(ref repo),
                            Some(ref msg_id),
                        ) =
                            (&chat_message_repo, &assistant_message_id)
                        {
                            let role =
                                super::chat_service_helpers::get_assistant_role(&context_type)
                                    .to_string();
                            let persisted = super::chat_service_send_background::finalize_structured_assistant_message(
                                repo,
                                &chat_timeline_repo,
                                app_handle.as_ref(),
                                context_type,
                                context_id,
                                conversation_id,
                                msg_id,
                                &role,
                                &processor.response_text,
                                &processor.tool_calls,
                                &processor.content_blocks,
                                split_verification_transcript,
                            )
                            .await;
                            let turn_capture = processor.current_turn_capture();
                            if let Some(capture) = turn_capture {
                                let turn_usage = capture.normalized.clone();
                                let usage_persisted = persist_usage_capture_run_first(
                                    &agent_run_repo,
                                    &agent_run_id,
                                    &chat_message_repo,
                                    &assistant_message_id,
                                    &capture,
                                )
                                .await;
                                if usage_persisted && turn_usage != last_emitted_usage {
                                    emit_usage_updated_event(
                                        &app_handle,
                                        &conversation_id_str,
                                        &context_type_str,
                                        &context_id_str,
                                    );
                                    last_emitted_usage = turn_usage;
                                }
                            }
                            persisted
                        } else {
                            false
                        };
                        if chat_message_repo.is_some() && !assistant_message_persisted {
                            return Err(StreamError::LocalToolFailed {
                                message: "Failed to persist the final assistant message"
                                    .to_string(),
                            });
                        }

                        // Persist session_id to DB on first TurnComplete
                        if !session_id_persisted && persist_conversation_provider_session_ref {
                            if let (Some(ref sess_id), Some(ref repo)) =
                                (&session_id, &conversation_repo)
                            {
                                let session_ref =
                                    provider_session_ref_for_harness(harness, sess_id.clone());
                                if let Err(e) = repo
                                    .update_provider_session_ref(conversation_id, &session_ref)
                                    .await
                                {
                                    tracing::error!(
                                        error = %e,
                                        conversation_id = %conversation_id_str,
                                        session_id = %sess_id,
                                        "TurnComplete: failed to persist provider_session_ref"
                                    );
                                } else {
                                    tracing::info!(
                                        conversation_id = %conversation_id_str,
                                        session_id = %sess_id,
                                        "TurnComplete: persisted provider_session_ref to DB"
                                    );
                                }
                                session_id_persisted = true;
                            }
                        }

                        let completion_applied = if let (Some(ref repo), Some(ref run_id)) =
                            (&agent_run_repo, &agent_run_id)
                        {
                            super::chat_service_run_finalization::finalize_run_completed(
                                repo,
                                &AgentRunId::from_string(run_id),
                            )
                            .await
                        } else {
                            false
                        };
                        completion_applied_for_stream |= completion_applied;

                        // Clear streaming state cache (same as normal run_completed path)
                        streaming_state_cache.clear(&conversation_id_str).await;

                        // Reset processor for the next turn (preserves session_id)
                        processor.reset_for_next_turn();

                        // Clear assistant message ID — a new one will be lazily
                        // created when the next content-producing event arrives.
                        assistant_message_id = None;

                        turns_finalized += 1;

                        // Free the execution slot while process is idle between turns.
                        // Only for contexts that use execution slots.
                        if super::uses_execution_slot(context_type) {
                            if let Some(ref exec_state) = execution_state {
                                // Atomically decrement + mark idle to prevent race where
                                // a concurrent claim_interactive_slot between two separate
                                // calls would skip increment and leak a count.
                                let slot_key = format!("{}/{}", context_type, context_id_str);
                                let new_count = exec_state.decrement_and_mark_idle(&slot_key);
                                tracing::debug!(
                                    %context_type,
                                    context_id = context_id_str.as_str(),
                                    new_count,
                                    "TurnComplete: decremented running count (idle between turns)"
                                );
                                if let Some(ref handle) = app_handle {
                                    exec_state.emit_status_changed(handle, "interactive_turn_idle");
                                }
                            }
                        }

                        let interactive_idle_applied =
                            if let (Some(registry), Some(key), Some(token)) = (
                                interactive_process_registry.as_ref(),
                                interactive_process_key.as_ref(),
                                interactive_process_token,
                            ) {
                                registry.mark_idle_if_token(key, token).await
                            } else {
                                false
                            };

                        let mut verification_pending = false;
                        if completion_applied && interactive_idle_applied {
                            if let (Some(handle), Some(run_id)) =
                                (app_handle.as_ref(), agent_run_id.as_ref())
                            {
                                if let Some(state) = handle.try_state::<AppState>() {
                                    let chat_service = state.build_chat_service_for_runtime(
                                        execution_state.clone(),
                                        Some(handle.clone()),
                                    );
                                    match crate::application::plan_verification_service::admit_automatic_plan_verification(
                                        state.inner(),
                                        &chat_service,
                                        conversation_id,
                                        &AgentRunId::from_string(run_id),
                                        true,
                                    )
                                    .await
                                    {
                                        Ok(disposition) => {
                                            verification_pending =
                                                disposition.verification_pending();
                                        }
                                        Err(error) => {
                                            tracing::error!(
                                                error = %error,
                                                conversation_id = %conversation_id_str,
                                                run_id,
                                                "TurnComplete: automatic plan verification admission failed"
                                            );
                                        }
                                    }
                                }
                            }
                        }

                        if completion_applied {
                            if let (Some(handle), Some(run_id)) =
                                (app_handle.as_ref(), agent_run_id.as_ref())
                            {
                                if let Some(state) = handle.try_state::<AppState>() {
                                    if !verification_pending {
                                        if let Err(error) = crate::application::plan_approval_notification_service::release_deferred_plan_approval_for_conversation(
                                            state.inner(),
                                            conversation_id,
                                        )
                                        .await
                                        {
                                            tracing::warn!(error = %error, conversation_id = %conversation_id_str, "Failed to release deferred plan approval after automatic admission settled");
                                        }
                                    }
                                    if let Err(error) = crate::application::plan_approval_notification_service::release_deferred_plan_approval_for_run(
                                        state.inner(),
                                        &AgentRunId::from_string(run_id),
                                    )
                                    .await
                                    {
                                        tracing::warn!(error = %error, run_id, "Failed to release deferred plan approval for terminal verification run");
                                    }
                                }
                            }
                        }

                        // Emit turn_completed (NOT run_completed) for interactive turns.
                        // Only the guarded winning completion may publish success events.
                        if completion_applied {
                            if let Some(ref handle) = app_handle {
                                let provider_session_id = session_id.clone();
                                let _ = handle.emit(
                                    super::chat_service_types::events::AGENT_TURN_COMPLETED,
                                    super::chat_service_types::AgentRunCompletedPayload::with_provider_session_and_run_id(
                                        agent_run_id.clone(),
                                        conversation_id_str.clone(),
                                        context_type_str.clone(),
                                        context_id_str.clone(),
                                        Some(harness),
                                        provider_session_id,
                                        None,
                                    ),
                                );
                                if !verification_pending {
                                    record_agent_waiting_if_user_attended(
                                        handle,
                                        context_type,
                                        context_id,
                                        conversation_id,
                                        agent_run_id.as_deref(),
                                    )
                                    .await;
                                }
                            }
                        }

                        let retire_after_turn =
                            if let (Some(registry), Some(key), Some(token), Some(run_id)) = (
                                interactive_process_registry.as_ref(),
                                interactive_process_key.as_ref(),
                                interactive_process_token,
                                agent_run_id.as_deref(),
                            ) {
                                matches!(
                                    registry.complete_turn_if_owner(key, token, run_id).await,
                                    InteractiveProcessTurnCompleteDisposition::RetireAfterTurn
                                )
                            } else {
                                false
                            };

                        if retire_after_turn {
                            tracing::info!(
                                conversation_id = %conversation_id_str,
                                "TurnComplete retired exact runtime for mode handoff"
                            );
                            // The normal guarded completion and turn_completed event above
                            // intentionally remain observable before the handoff exits.
                            between_interactive_turns = true;
                            silent_interactive_exit = true;
                            mode_handoff_exit = true;
                            let _ = child.start_kill();
                            break;
                        }

                        // Mark that we're now between interactive turns —
                        // the timeout handler should not kill the process.
                        between_interactive_turns = true;
                    }
                    StreamEvent::TaskStarted {
                        tool_use_id,
                        tool_name,
                        description,
                        subagent_type,
                        model,
                    } => {
                        // Track active subagent tasks for timeout bypass
                        active_task_tracker.task_started();

                        // Update streaming state cache with new task
                        let cached_task = CachedStreamingTask {
                            tool_use_id: tool_use_id.clone(),
                            description: description.clone(),
                            subagent_type: subagent_type.clone(),
                            model: model.clone(),
                            status: "running".to_string(),
                            agent_id: None,
                            delegated_job_id: None,
                            delegated_session_id: None,
                            delegated_conversation_id: None,
                            delegated_agent_run_id: None,
                            provider_harness: None,
                            provider_session_id: None,
                            upstream_provider: None,
                            provider_profile: None,
                            logical_model: None,
                            effective_model_id: None,
                            logical_effort: None,
                            effective_effort: None,
                            approval_policy: None,
                            sandbox_mode: None,
                            total_tokens: None,
                            total_tool_uses: None,
                            duration_ms: None,
                            input_tokens: None,
                            output_tokens: None,
                            cache_creation_tokens: None,
                            cache_read_tokens: None,
                            estimated_usd: None,
                            text_output: None,
                            started_at: None,
                            completed_at: None,
                            timestamp_provenance: None,
                            seq: Some(stream_seq),
                        };
                        streaming_state_cache
                            .add_task(&conversation_id_str, cached_task)
                            .await;

                        if let Some(ref handle) = app_handle {
                            let _ = handle.emit(
                                events::AGENT_TASK_STARTED,
                                AgentTaskStartedPayload {
                                    tool_use_id,
                                    run_id: agent_run_id.clone(),
                                    tool_name,
                                    description,
                                    subagent_type,
                                    model,
                                    teammate_name: None,
                                    delegated_job_id: None,
                                    delegated_session_id: None,
                                    delegated_conversation_id: None,
                                    delegated_agent_run_id: None,
                                    provider_harness: None,
                                    provider_session_id: None,
                                    upstream_provider: None,
                                    provider_profile: None,
                                    logical_model: None,
                                    effective_model_id: None,
                                    logical_effort: None,
                                    effective_effort: None,
                                    approval_policy: None,
                                    sandbox_mode: None,
                                    started_at: None,
                                    completed_at: None,
                                    timestamp_provenance: None,
                                    conversation_id: conversation_id_str.clone(),
                                    context_type: context_type_str.clone(),
                                    context_id: context_id_str.clone(),
                                    seq: stream_seq,
                                },
                            );
                            stream_seq += 1;
                        }
                    }
                    StreamEvent::TaskCompleted {
                        tool_use_id,
                        agent_id,
                        total_duration_ms,
                        total_tokens,
                        total_tool_use_count,
                    } => {
                        // Track active subagent tasks for timeout bypass
                        active_task_tracker.task_completed();

                        // Update streaming state cache - mark task as completed
                        streaming_state_cache
                            .complete_task(
                                &conversation_id_str,
                                &tool_use_id,
                                Some(ToolCallStats {
                                    model: None,
                                    total_tokens,
                                    total_tool_uses: total_tool_use_count,
                                    duration_ms: total_duration_ms,
                                }),
                            )
                            .await;

                        if let Some(ref handle) = app_handle {
                            let _ = handle.emit(
                                events::AGENT_TASK_COMPLETED,
                                AgentTaskCompletedPayload {
                                    teammate_name: None,
                                    tool_use_id,
                                    run_id: agent_run_id.clone(),
                                    agent_id,
                                    status: Some("completed".to_string()),
                                    total_duration_ms,
                                    total_tokens,
                                    total_tool_use_count,
                                    delegated_job_id: None,
                                    delegated_session_id: None,
                                    delegated_conversation_id: None,
                                    delegated_agent_run_id: None,
                                    provider_harness: None,
                                    provider_session_id: None,
                                    upstream_provider: None,
                                    provider_profile: None,
                                    logical_model: None,
                                    effective_model_id: None,
                                    logical_effort: None,
                                    effective_effort: None,
                                    approval_policy: None,
                                    sandbox_mode: None,
                                    started_at: None,
                                    completed_at: None,
                                    timestamp_provenance: None,
                                    input_tokens: None,
                                    output_tokens: None,
                                    cache_creation_tokens: None,
                                    cache_read_tokens: None,
                                    estimated_usd: None,
                                    text_output: None,
                                    error: None,
                                    conversation_id: conversation_id_str.clone(),
                                    context_type: context_type_str.clone(),
                                    context_id: context_id_str.clone(),
                                    seq: stream_seq,
                                },
                            );
                            stream_seq += 1;
                        }
                    }
                    StreamEvent::HookStarted {
                        hook_id,
                        hook_name,
                        hook_event,
                    } => {
                        if let Some(ref handle) = app_handle {
                            let _ = handle.emit(
                                events::AGENT_HOOK,
                                AgentHookPayload {
                                    hook_type: "started".to_string(),
                                    hook_name: Some(hook_name),
                                    hook_event: Some(hook_event),
                                    hook_id: Some(hook_id),
                                    output: None,
                                    outcome: None,
                                    exit_code: None,
                                    reason: None,
                                    conversation_id: conversation_id_str.clone(),
                                    context_type: context_type_str.clone(),
                                    context_id: context_id_str.clone(),
                                    timestamp: chrono::Utc::now().timestamp_millis(),
                                },
                            );
                        }
                    }
                    StreamEvent::HookCompleted {
                        hook_id,
                        hook_name,
                        hook_event,
                        output,
                        exit_code,
                        outcome,
                    } => {
                        if let Some(ref handle) = app_handle {
                            let _ = handle.emit(
                                events::AGENT_HOOK,
                                AgentHookPayload {
                                    hook_type: "completed".to_string(),
                                    hook_name: Some(hook_name),
                                    hook_event: Some(hook_event),
                                    hook_id: Some(hook_id),
                                    output,
                                    outcome,
                                    exit_code,
                                    reason: None,
                                    conversation_id: conversation_id_str.clone(),
                                    context_type: context_type_str.clone(),
                                    context_id: context_id_str.clone(),
                                    timestamp: chrono::Utc::now().timestamp_millis(),
                                },
                            );
                        }
                    }
                    StreamEvent::HookBlock { reason } => {
                        if let Some(ref handle) = app_handle {
                            let _ = handle.emit(
                                events::AGENT_HOOK,
                                AgentHookPayload {
                                    hook_type: "block".to_string(),
                                    hook_name: None,
                                    hook_event: None,
                                    hook_id: None,
                                    output: None,
                                    outcome: None,
                                    exit_code: None,
                                    reason: Some(reason),
                                    conversation_id: conversation_id_str.clone(),
                                    context_type: context_type_str.clone(),
                                    context_id: context_id_str.clone(),
                                    timestamp: chrono::Utc::now().timestamp_millis(),
                                },
                            );
                        }
                    }

                    StreamEvent::ToolResultReceived {
                        tool_use_id,
                        result,
                        is_error,
                        parent_tool_use_id,
                    } => {
                        if let Some(tool_call) = processor
                            .tool_calls
                            .iter()
                            .find(|tool_call| tool_call.id.as_deref() == Some(&tool_use_id))
                        {
                            if is_completion_tool_name(&tool_call.name) {
                                if !is_error && completion_tool_result_accepted(Some(&result)) {
                                    completion_signal_tracker.mark_completion_called();
                                    tracing::info!(
                                        conversation_id = %conversation_id_str,
                                        context_id,
                                        tool_name = %tool_call.name,
                                        grace_secs = completion_grace_duration.as_secs(),
                                        "Completion tool result accepted, entering shutdown grace period"
                                    );
                                } else {
                                    tracing::warn!(
                                        conversation_id = %conversation_id_str,
                                        context_id,
                                        tool_name = %tool_call.name,
                                        result = ?result,
                                        "Completion tool result rejected; not entering shutdown grace period"
                                    );
                                }
                            }
                        }

                        let result_preview = build_live_tool_result_preview_for_tool_id(
                            &processor.tool_calls,
                            Some(&conversation_id_str),
                            assistant_message_id.as_deref(),
                            &tool_use_id,
                            &result,
                        );
                        if result_preview.is_previewed() {
                            persist_assistant_message_snapshot(
                                &chat_message_repo,
                                &assistant_message_id,
                                &processor.response_text,
                                &processor.tool_calls,
                                &processor.content_blocks,
                            )
                            .await;
                            persist_timeline_snapshot(
                                &chat_timeline_repo,
                                &conversation_id_str,
                                &assistant_message_id,
                                &processor.content_blocks,
                                ChatTimelineItemStatus::Streaming,
                            )
                            .await;
                        }

                        if let Some(ref handle) = app_handle {
                            let _ = handle.emit(
                                events::AGENT_TOOL_CALL,
                                AgentToolCallPayload::from_live_tool_result(
                                    &tool_use_id,
                                    &result_preview,
                                    &conversation_id_str,
                                    &context_type_str,
                                    &context_id_str,
                                    agent_run_id.as_deref(),
                                    parent_tool_use_id,
                                    stream_seq,
                                ),
                            );
                            stream_seq += 1;

                            // Activity stream event for task execution and merge
                            if matches!(
                                context_type,
                                ChatContextType::TaskExecution | ChatContextType::Merge
                            ) {
                                let result_content =
                                    live_tool_result_activity_content(&result_preview);
                                let result_metadata = live_tool_result_activity_metadata(
                                    &tool_use_id,
                                    &result_preview,
                                );

                                let _ = handle.emit(
                                    events::AGENT_MESSAGE,
                                    serde_json::json!({
                                        "taskId": context_id_str,
                                        "type": "tool_result",
                                        "content": result_content,
                                        "timestamp": chrono::Utc::now().timestamp_millis(),
                                        "metadata": result_metadata,
                                    }),
                                );

                                // Persist activity event to database
                                if let (Some(ref repo), Some(ref task_id)) =
                                    (&activity_event_repo, &task_id_for_persistence)
                                {
                                    let event = ActivityEvent::new_task_event(
                                        task_id.clone(),
                                        ActivityEventType::ToolResult,
                                        result_content,
                                    )
                                    .with_metadata(result_metadata.to_string());
                                    // Fetch current task status and add to event
                                    let event = if let Some(ref t_repo) = task_repo {
                                        if let Ok(Some(task)) = t_repo.get_by_id(task_id).await {
                                            event.with_status(task.internal_status)
                                        } else {
                                            event
                                        }
                                    } else {
                                        event
                                    };
                                    let _ = repo.save(event).await;
                                }
                            }
                        }
                    }
                }
            }
        } else if lines_seen > 0 && last_parsed_at.elapsed() >= timeout_config.parse_stall_timeout {
            // Gather state for kill decision (async state first)
            let has_pending_question = if let Some(ref qs) = question_state {
                qs.has_pending_for_session(context_id).await
            } else {
                false
            };
            let (pid_alive, child_exited) = if let Some(pid) = child.id() {
                let exited = child.try_wait().ok().flatten().is_some();
                let alive = crate::domain::services::is_process_alive(pid);
                (alive, exited)
            } else {
                (false, true)
            };
            let is_completion_grace_period =
                completion_signal_tracker.is_in_grace_period(completion_grace_duration);

            if should_kill_on_timeout(
                last_activity_at.elapsed(),
                max_wall_clock,
                has_pending_question,
                false, // parse stall path has no interactive_turns bypass
                pid_alive,
                child_exited,
                active_task_tracker.has_active_tasks(),
                is_completion_grace_period,
            ) {
                if last_activity_at.elapsed() > max_wall_clock {
                    tracing::warn!(
                        conversation_id = %conversation_id_str,
                        idle_secs = last_activity_at.elapsed().as_secs(),
                        total_secs = stream_start.elapsed().as_secs(),
                        "Idle cap reached in parse stall path — killing agent"
                    );
                } else {
                    tracing::warn!(
                        conversation_id = %conversation_id_str,
                        lines_seen,
                        lines_parsed,
                        stall_secs = timeout_config.parse_stall_timeout.as_secs(),
                        "Stream parse stall: received stdout but no parseable events, killing agent"
                    );
                }
                if completion_signal_tracker.was_called() {
                    tracing::warn!(
                        conversation_id = %conversation_id_str,
                        context_id,
                        grace_secs = completion_grace_duration.as_secs(),
                        "Completion grace period expired after completion tool call, proceeding with kill"
                    );
                }
                let _ = child.kill().await;
                flush_content_before_error(
                    &chat_message_repo,
                    &assistant_message_id,
                    &processor.response_text,
                    &processor.tool_calls,
                    &processor.content_blocks,
                )
                .await;
                persist_timeline_snapshot(
                    &chat_timeline_repo,
                    &conversation_id_str,
                    &assistant_message_id,
                    &processor.content_blocks,
                    ChatTimelineItemStatus::Error,
                )
                .await;
                return Err(StreamError::ParseStall {
                    context_type,
                    elapsed_secs: timeout_config.parse_stall_timeout.as_secs(),
                    lines_seen,
                    lines_parsed,
                });
            } else {
                // Bypass: reset stall timer and log reason
                if has_pending_question {
                    tracing::info!(
                        conversation_id = %conversation_id_str,
                        context_id,
                        lines_seen,
                        "Stream parse stall but pending question exists, resetting stall timer"
                    );
                } else if pid_alive && !child_exited {
                    if let Some(pid) = child.id() {
                        tracing::info!(
                            conversation_id = %conversation_id_str,
                            pid,
                            "Parse stall but child process alive — resetting"
                        );
                        emit_heartbeat(
                            &app_handle,
                            &conversation_id_str,
                            context_id,
                            "pid_alive_bypass_parse_stall",
                            Some(serde_json::json!({ "pid": pid })),
                        );
                    }
                } else if active_task_tracker.has_active_tasks() {
                    let active_count = active_task_tracker.count();
                    tracing::info!(
                        conversation_id = %conversation_id_str,
                        context_id,
                        lines_seen,
                        active_tasks = active_count,
                        "Stream parse stall but {} active subagent task(s), resetting stall timer",
                        active_count
                    );
                    emit_heartbeat(
                        &app_handle,
                        &conversation_id_str,
                        context_id,
                        "active_tasks_bypass",
                        Some(serde_json::json!({ "active_tasks": active_count })),
                    );
                } else if is_completion_grace_period {
                    tracing::info!(
                        conversation_id = %conversation_id_str,
                        context_id,
                        lines_seen,
                        grace_secs = completion_grace_duration.as_secs(),
                        "Stream parse stall after completion tool call, staying in shutdown grace period"
                    );
                } else {
                    debug_assert!(
                        false,
                        "parse stall bypass branch should be exhaustively handled"
                    );
                }
                // CRITICAL: reset last_parsed_at to prevent hot spin loop
                last_parsed_at = std::time::Instant::now();
                // Fall through to debounced flush (do NOT use continue)
            }
        }

        if streaming_persistence_dirty
            && last_streaming_persisted_at.elapsed() >= streaming_persistence_debounce
        {
            flush_streaming_persistence_if_dirty(
                &mut streaming_persistence_dirty,
                &mut last_streaming_persisted_at,
                &chat_message_repo,
                &chat_timeline_repo,
                &conversation_id_str,
                &assistant_message_id,
                &processor.response_text,
                &processor.tool_calls,
                &processor.content_blocks,
                ChatTimelineItemStatus::Streaming,
            )
            .await;
        }

        // Usage runs on its own cadence. Tying it to the text debounce would
        // stall usage updates for turns that stream no text, such as a long
        // run of tool calls.
        if last_usage_flush.elapsed() >= USAGE_FLUSH_INTERVAL {
            last_usage_flush = std::time::Instant::now();
            if let Some(capture) = processor.current_turn_capture() {
                let current_turn_usage = capture.normalized.clone();
                let usage_persisted = persist_usage_capture_run_first(
                    &agent_run_repo,
                    &agent_run_id,
                    &chat_message_repo,
                    &assistant_message_id,
                    &capture,
                )
                .await;
                if usage_persisted && current_turn_usage != last_emitted_usage {
                    emit_usage_updated_event(
                        &app_handle,
                        &conversation_id_str,
                        &context_type_str,
                        &context_id_str,
                    );
                    last_emitted_usage = current_turn_usage;
                }
            }
        }

        // Throttled heartbeat: write last_active_at every 5s on any parsed event
        if lines_parsed > 0 && last_heartbeat.elapsed() >= HEARTBEAT_INTERVAL {
            if let (Some(ref registry), Some(ref key), Some(ref run_id)) =
                (&running_agent_registry, &heartbeat_key, &agent_run_id)
            {
                let _ = registry
                    .update_heartbeat(key, run_id, chrono::Utc::now())
                    .await;
            }
            last_heartbeat = std::time::Instant::now();
        }

        #[allow(unknown_lints, clippy::manual_is_multiple_of)]
        if lines_seen > 0 && lines_seen % 50 == 0 {
            tracing::debug!(
                conversation_id = %conversation_id_str,
                lines_seen,
                lines_parsed,
                response_len = processor.response_text.len(),
                tool_calls = processor.tool_calls.len(),
                "Stream progress"
            );
        }
    }

    let result = processor.finish();

    // Wait for stderr task
    let stderr_content = {
        let raw = stderr_task.await.unwrap_or_default();
        crate::utils::secret_redactor::redact(&raw)
    };

    // Wait for process
    let status = child.wait().await.map_err(|e| StreamError::AgentExit {
        exit_code: None,
        stderr: e.to_string(),
    })?;
    let exit_details = process_exit_details(&status);
    let stderr_preview = truncate_str(stderr_content.trim(), 2000);
    let response_len = result.response_text.len();
    let tool_calls_count = result.tool_calls.len();
    let content_blocks_count = result.content_blocks.len();

    // Log stderr and exit metadata when agent produced no output (critical diagnostic)
    if lines_seen == 0 {
        tracing::warn!(
            conversation_id = %conversation_id_str,
            exit_code = exit_details.exit_code,
            exit_signal = exit_details.exit_signal,
            stderr_len = stderr_content.len(),
            stderr_preview = %stderr_preview,
            "Stream ended with ZERO lines from stdout. stderr: {}",
            stderr_preview
        );
    }

    if !exit_details.success && !silent_interactive_exit {
        if completion_signal_tracker.was_called() {
            tracing::warn!(
                conversation_id = %conversation_id_str,
                context_id,
                lines_seen,
                lines_parsed,
                turns_finalized,
                response_len,
                tool_calls = tool_calls_count,
                content_blocks = content_blocks_count,
                exit_code = exit_details.exit_code,
                exit_signal = exit_details.exit_signal,
                stderr_len = stderr_content.len(),
                stderr_preview = %stderr_preview,
                "Agent exited non-zero after completion tool call; treating as successful completion"
            );
        } else {
            tracing::error!(
                conversation_id = %conversation_id_str,
                context_id,
                lines_seen,
                lines_parsed,
                turns_finalized,
                response_len,
                tool_calls = tool_calls_count,
                content_blocks = content_blocks_count,
                exit_code = exit_details.exit_code,
                exit_signal = exit_details.exit_signal,
                stderr_len = stderr_content.len(),
                stderr_preview = %stderr_preview,
                "Agent process exited unsuccessfully during stream"
            );

            flush_content_before_error(
                &chat_message_repo,
                &assistant_message_id,
                &result.response_text,
                &result.tool_calls,
                &result.content_blocks,
            )
            .await;
            persist_timeline_snapshot(
                &chat_timeline_repo,
                &conversation_id_str,
                &assistant_message_id,
                &result.content_blocks,
                ChatTimelineItemStatus::Error,
            )
            .await;

            return Err(StreamError::AgentExit {
                exit_code: exit_details.exit_code,
                stderr: format_agent_exit_stderr(exit_details, &stderr_content),
            });
        }
    }

    if context_type == ChatContextType::Ideation && turns_finalized == 0 && !silent_interactive_exit
    {
        tracing::warn!(
            conversation_id = %conversation_id_str,
            context_id,
            lines_seen,
            lines_parsed,
            response_len,
            tool_calls = tool_calls_count,
            content_blocks = content_blocks_count,
            exit_code = exit_details.exit_code,
            exit_signal = exit_details.exit_signal,
            stderr_len = stderr_content.len(),
            stderr_preview = %stderr_preview,
            "Ideation stream ended without TurnComplete"
        );
    }

    // The execution slot is held unless we're idle between interactive turns
    // (TurnComplete decremented and no new message re-incremented).
    let execution_slot_held =
        !between_interactive_turns || !super::uses_execution_slot(context_type);

    let outcome = StreamOutcome {
        response_text: result.response_text,
        tool_calls: result.tool_calls,
        content_blocks: result.content_blocks,
        session_id: result.session_id,
        usage: result.usage,
        usage_provenance: result.usage_provenance,
        stderr_text: stderr_content,
        turns_finalized,
        completion_applied: completion_applied_for_stream,
        execution_slot_held,
        completion_tool_called: completion_signal_tracker.was_called(),
        silent_interactive_exit,
        mode_handoff_exit,
    };

    // Final flush of accumulated content so post-loop error returns don't lose data
    flush_content_before_error(
        &chat_message_repo,
        &assistant_message_id,
        &outcome.response_text,
        &outcome.tool_calls,
        &outcome.content_blocks,
    )
    .await;
    persist_timeline_snapshot(
        &chat_timeline_repo,
        &conversation_id_str,
        &assistant_message_id,
        &outcome.content_blocks,
        ChatTimelineItemStatus::Finalized,
    )
    .await;
    if !outcome.usage.is_empty() {
        let capture = UsageCapture::normalized(
            outcome.usage.clone(),
            outcome
                .usage_provenance
                .unwrap_or(UsageProvenance::ProviderSnapshotFallback),
        );
        let no_message_mirror = None;
        let capture_message_id = if turns_finalized == 0 {
            &assistant_message_id
        } else {
            &no_message_mirror
        };
        if persist_usage_capture_run_first(
            &agent_run_repo,
            &agent_run_id,
            &chat_message_repo,
            capture_message_id,
            &capture,
        )
        .await
            && (turns_finalized == 0 || outcome.usage != last_emitted_usage)
        {
            emit_usage_updated_event(
                &app_handle,
                &conversation_id_str,
                &context_type_str,
                &context_id_str,
            );
        }
    }

    // Check if cancellation was requested during/after stream processing.
    // Fixes race where EOF from killed process wins the tokio::select! over
    // the cancellation token, causing the loop to break instead of returning
    // Err(Cancelled). If the token is cancelled, always return Cancelled —
    // unless this was a silent interactive exit (already handled above).
    if cancellation_token.is_cancelled() && !silent_interactive_exit {
        return Err(StreamError::Cancelled {
            turns_finalized,
            completion_tool_called: completion_signal_tracker.was_called(),
        });
    }

    tracing::debug!(
        conversation_id = %conversation_id_str,
        success = exit_details.success,
        exit_code = exit_details.exit_code,
        exit_signal = exit_details.exit_signal,
        response_len = outcome.response_text.len(),
        tool_calls = outcome.tool_calls.len(),
        "Stream finished"
    );

    let has_output = outcome.has_meaningful_output();

    if outcome.tool_calls.is_empty() {
        if let Some(provider_err) =
            super::chat_service_errors::classify_provider_error_from_assistant_content(
                &outcome.response_text,
            )
        {
            return Err(provider_err);
        }
    }

    if !has_output {
        let payload = if debug_lines.is_empty() {
            format!(
                "no stdout lines captured\n\nexit_code: {:?}\nexit_signal: {:?}\n\nstderr:\n{}",
                exit_details.exit_code,
                exit_details.exit_signal,
                outcome.stderr_text.trim(),
            )
        } else {
            format!(
                "stdout sample:\n{}\n\nexit_code: {:?}\nexit_signal: {:?}\n\nstderr:\n{}",
                debug_lines.join("\n"),
                exit_details.exit_code,
                exit_details.exit_signal,
                outcome.stderr_text.trim()
            )
        };
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            if let Some(parent) = debug_path.parent() {
                // codeql[rust/path-injection]
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = crate::utils::path_safety::checked_remove_file(&debug_path, "stream debug log");
            match std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                // codeql[rust/path-injection]
                .open(&debug_path)
            {
                Ok(mut f) => {
                    let _ = f.write_all(payload.as_bytes());
                    info!(
                        path = %debug_path.display(),
                        conversation_id = %conversation_id_str,
                        "Wrote stream debug log"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        path = %debug_path.display(),
                        error = %e,
                        "Failed to write stream debug log"
                    );
                }
            }
        }
    }

    if result.is_error {
        let error_msg = if !result.errors.is_empty() {
            result.errors.join("; ")
        } else {
            "Agent failed during execution".to_string()
        };
        // Check for recoverable provider errors before returning generic AgentExit
        if let Some(provider_err) = super::chat_service_errors::classify_provider_error(&error_msg)
        {
            return Err(provider_err);
        }
        // Also check stderr for provider error patterns
        if let Some(provider_err) =
            super::chat_service_errors::classify_provider_error(&outcome.stderr_text)
        {
            return Err(provider_err);
        }
        if completion_signal_tracker.was_called() {
            tracing::warn!(
                conversation_id = %conversation_id_str,
                context_id,
                error = %error_msg,
                "Agent result reported a non-provider error after accepted completion; treating it as post-completion diagnostic noise"
            );
        } else {
            return Err(StreamError::AgentExit {
                exit_code: status.code(),
                stderr: error_msg,
            });
        }
    }

    if !status.success()
        && !has_output
        && turns_finalized == 0
        && !silent_interactive_exit
        && !completion_signal_tracker.was_called()
    {
        let stderr_trimmed = outcome.stderr_text.trim().to_string();
        // Check for recoverable provider errors in stderr
        if let Some(provider_err) =
            super::chat_service_errors::classify_provider_error(&stderr_trimmed)
        {
            return Err(provider_err);
        }
        return Err(StreamError::AgentExit {
            exit_code: status.code(),
            stderr: stderr_trimmed,
        });
    }

    Ok(outcome)
}

pub(super) fn is_armed_mode_handoff_disposition(
    disposition: InteractiveProcessRetireAfterTurnDisposition,
) -> bool {
    matches!(
        disposition,
        InteractiveProcessRetireAfterTurnDisposition::Active { is_armed: true }
            | InteractiveProcessRetireAfterTurnDisposition::Idle { is_armed: true }
    )
}

#[allow(clippy::too_many_arguments)]
async fn process_codex_stream_background<R: Runtime>(
    mut child: tokio::process::Child,
    context_type: ChatContextType,
    context_id: &str,
    conversation_id: &ChatConversationId,
    app_handle: Option<AppHandle<R>>,
    activity_event_repo: Option<Arc<dyn ActivityEventRepository>>,
    task_repo: Option<Arc<dyn TaskRepository>>,
    chat_message_repo: Option<Arc<dyn ChatMessageRepository>>,
    chat_timeline_repo: Option<Arc<dyn ChatTimelineRepository>>,
    assistant_message_id: Option<String>,
    question_state: Option<Arc<QuestionState>>,
    cancellation_token: CancellationToken,
    streaming_state_cache: StreamingStateCache,
    running_agent_registry: Option<Arc<dyn RunningAgentRegistry>>,
    agent_run_repo: Option<Arc<dyn AgentRunRepository>>,
    agent_run_id: Option<String>,
    _execution_state: Option<Arc<crate::commands::ExecutionState>>,
    conversation_repo: Option<Arc<dyn ChatConversationRepository>>,
    _split_verification_transcript: bool,
    persist_conversation_provider_session_ref: bool,
) -> Result<StreamOutcome, StreamError> {
    let timeout_config = StreamTimeoutConfig::for_context(&context_type);
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| StreamError::ProcessSpawnFailed {
            command: "codex".to_string(),
            error: "Failed to capture stdout".to_string(),
        })?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| StreamError::ProcessSpawnFailed {
            command: "codex".to_string(),
            error: "Failed to capture stderr".to_string(),
        })?;

    let event_ctx = event_context(conversation_id, &context_type, context_id);
    let conversation_id_str = event_ctx.conversation_id.clone();
    let context_type_str = event_ctx.context_type.clone();
    let context_id_str = event_ctx.context_id.clone();
    let task_id_for_persistence = if matches!(
        context_type,
        ChatContextType::TaskExecution | ChatContextType::Merge
    ) {
        Some(TaskId::from_string(context_id.to_string()))
    } else {
        None
    };

    let stderr_task = tokio::spawn(async move {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();
        let mut stderr_content = String::new();

        while let Ok(Some(line)) = lines.next_line().await {
            stderr_content.push_str(&line);
            stderr_content.push('\n');
        }

        stderr_content
    });

    let stdout_reader = BufReader::new(stdout);
    let mut lines = stdout_reader.lines();
    let mut response_text = String::new();
    let mut tool_calls = Vec::<ToolCall>::new();
    let mut content_blocks = Vec::<ContentBlockItem>::new();
    let mut runtime_errors = Vec::<String>::new();
    let mut local_tool_errors = Vec::<String>::new();
    let mut session_id: Option<String> = None;
    let mut run_session_attribution_ready = agent_run_repo.is_none() || agent_run_id.is_none();
    let mut usage = AgentRunUsage::default();
    let mut lines_seen = 0usize;
    let mut lines_parsed = 0usize;
    let mut stream_seq = 0u64;
    let mut last_parsed_at = std::time::Instant::now();
    let max_wall_clock = std::time::Duration::from_secs(stream_timeouts().max_wall_clock_secs);
    let mut last_activity_at = std::time::Instant::now();
    let mut last_flush = std::time::Instant::now();
    const FLUSH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);
    let mut completion_signal_tracker = CompletionSignalTracker::default();
    let mut last_emitted_capture: Option<UsageCapture> = None;
    let mut pending_codex_file_changes: HashMap<String, PendingCodexFileChange> = HashMap::new();
    let mut codex_turn_completed = false;
    let heartbeat_key = running_agent_registry
        .as_ref()
        .map(|_| RunningAgentKey::new(context_type.to_string(), context_id));
    let mut last_heartbeat = std::time::Instant::now();
    const HEARTBEAT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

    loop {
        let line = tokio::select! {
            _ = cancellation_token.cancelled() => {
                let _ = child.kill().await;
                flush_content_before_error(
                    &chat_message_repo,
                    &assistant_message_id,
                    &response_text,
                    &tool_calls,
                    &content_blocks,
                ).await;
                return Err(StreamError::Cancelled {
                    turns_finalized: 0,
                    completion_tool_called: false,
                });
            }
            read_result = timeout(timeout_config.line_read_timeout, lines.next_line()) => {
                match read_result {
                    Ok(Ok(Some(line))) => line,
                    Ok(Ok(None)) => break,
                    Ok(Err(error)) => {
                        return Err(StreamError::AgentExit {
                            exit_code: None,
                            stderr: error.to_string(),
                        });
                    }
                    Err(_) => {
                        let has_pending_question = if let Some(ref qs) = question_state {
                            qs.has_pending_for_session(context_id).await
                        } else {
                            false
                        };
                        let (pid_alive, child_exited) = if let Some(pid) = child.id() {
                            let exited = child.try_wait().ok().flatten().is_some();
                            let alive = crate::domain::services::is_process_alive(pid);
                            (alive, exited)
                        } else {
                            (false, true)
                        };

                        if should_kill_on_timeout(
                            last_activity_at.elapsed(),
                            max_wall_clock,
                            has_pending_question,
                            false,
                            pid_alive,
                            child_exited,
                            false,
                            false,
                        ) {
                            let _ = child.kill().await;
                            flush_content_before_error(
                                &chat_message_repo,
                                &assistant_message_id,
                                &response_text,
                                &tool_calls,
                                &content_blocks,
                            ).await;
                            return Err(StreamError::Timeout {
                                context_type,
                                elapsed_secs: timeout_config.line_read_timeout.as_secs(),
                            });
                        }
                        continue;
                    }
                }
            }
        };

        lines_seen += 1;

        if let Some(event) = parse_codex_event_line(&line) {
            lines_parsed += 1;
            last_parsed_at = std::time::Instant::now();
            last_activity_at = std::time::Instant::now();

            if let Some(thread_id) = extract_codex_thread_id(&event) {
                session_id = Some(thread_id.clone());
                let session_ref =
                    provider_session_ref_for_harness(AgentHarnessKind::Codex, thread_id.clone());
                if let (Some(repo), Some(run_id)) = (agent_run_repo.as_ref(), agent_run_id.as_ref())
                {
                    run_session_attribution_ready = match repo
                        .update_attribution(
                            &AgentRunId::from_string(run_id.clone()),
                            &crate::domain::entities::AgentRunAttribution {
                                harness: Some(AgentHarnessKind::Codex),
                                provider_session_id: Some(thread_id.clone()),
                                ..Default::default()
                            },
                        )
                        .await
                    {
                        Ok(()) => true,
                        Err(error) => {
                            tracing::warn!(
                                run_id,
                                error = %error,
                                "Failed to persist Codex session attribution; cumulative usage capture will be suppressed"
                            );
                            false
                        }
                    };
                }
                if let (Some(repo), Some(message_id)) =
                    (chat_message_repo.as_ref(), assistant_message_id.as_ref())
                {
                    let _ = repo
                        .update_provider_session_ref(
                            &ChatMessageId::from_string(message_id.clone()),
                            &session_ref,
                        )
                        .await;
                }
                if persist_conversation_provider_session_ref {
                    if let Some(ref repo) = conversation_repo {
                        let _ = repo
                            .update_provider_session_ref(conversation_id, &session_ref)
                            .await;
                    }
                }
            }

            if let Some(text) = extract_codex_agent_message(&event) {
                let block_position = current_text_block_position(&content_blocks);
                if !response_text.is_empty() {
                    response_text.push_str("\n\n");
                }
                response_text.push_str(&text);
                content_blocks.push(ContentBlockItem::Text { text: text.clone() });

                persist_assistant_message_snapshot(
                    &chat_message_repo,
                    &assistant_message_id,
                    &response_text,
                    &tool_calls,
                    &content_blocks,
                )
                .await;
                persist_timeline_snapshot(
                    &chat_timeline_repo,
                    &conversation_id_str,
                    &assistant_message_id,
                    &content_blocks,
                    ChatTimelineItemStatus::Streaming,
                )
                .await;

                if let Some(ref handle) = app_handle {
                    let _ = handle.emit(
                        events::AGENT_CHUNK,
                        AgentChunkPayload {
                            text,
                            run_id: agent_run_id.clone(),
                            block_index: Some(block_position),
                            conversation_id: conversation_id_str.clone(),
                            context_type: context_type_str.clone(),
                            context_id: context_id_str.clone(),
                            seq: stream_seq,
                            append_to_previous: false,
                        },
                    );
                    stream_seq += 1;
                }
            }

            if let Some(text) = extract_codex_reasoning(&event) {
                let block_position = current_text_block_position(&content_blocks);
                content_blocks.push(ContentBlockItem::Thinking {
                    text: text.clone(),
                    duration_ms: None,
                });
                streaming_state_cache
                    .append_thinking(&conversation_id_str, block_position as usize, &text)
                    .await;

                persist_assistant_message_snapshot(
                    &chat_message_repo,
                    &assistant_message_id,
                    &response_text,
                    &tool_calls,
                    &content_blocks,
                )
                .await;
                persist_timeline_snapshot(
                    &chat_timeline_repo,
                    &conversation_id_str,
                    &assistant_message_id,
                    &content_blocks,
                    ChatTimelineItemStatus::Streaming,
                )
                .await;

                if let Some(ref handle) = app_handle {
                    let _ = handle.emit(
                        events::AGENT_THINKING,
                        AgentThinkingPayload {
                            text,
                            run_id: agent_run_id.clone(),
                            block_index: Some(block_position),
                            conversation_id: conversation_id_str.clone(),
                            context_type: context_type_str.clone(),
                            context_id: context_id_str.clone(),
                            seq: stream_seq,
                            append_to_previous: false,
                        },
                    );
                    stream_seq += 1;
                }
            }

            let mut codex_tool_snapshots = Vec::new();
            if let Some(file_change_snapshot) = extract_codex_file_change_snapshot(&event) {
                codex_tool_snapshots.extend(resolve_codex_file_change_tool_call_snapshots(
                    file_change_snapshot,
                    &mut pending_codex_file_changes,
                ));
            }
            if let Some(snapshot) = extract_codex_tool_call_snapshot(&event) {
                codex_tool_snapshots.push(snapshot);
            }

            for snapshot in codex_tool_snapshots {
                let tool_call = snapshot.tool_call;
                if is_completion_tool_name(&tool_call.name) {
                    tracing::info!(
                        conversation_id = %conversation_id_str,
                        context_id,
                        tool_name = %tool_call.name,
                        "Detected completion tool call in Codex stream"
                    );
                }
                upsert_codex_tool_call_snapshot(
                    &mut tool_calls,
                    &mut content_blocks,
                    tool_call.clone(),
                );
                if snapshot.phase == CodexToolCallPhase::Completed
                    && is_completion_tool_name(&tool_call.name)
                {
                    tracing::info!(
                        conversation_id = %conversation_id_str,
                        context_id,
                        tool_name = %tool_call.name,
                        "Completion tool call observed during Codex streaming; enabling shutdown grace period"
                    );
                }
                let diff_context_value = tool_call
                    .diff_context
                    .as_ref()
                    .and_then(|value| serde_json::to_value(value).ok());
                streaming_state_cache
                    .upsert_tool_call(
                        &conversation_id_str,
                        CachedToolCall {
                            id: tool_call
                                .id
                                .clone()
                                .unwrap_or_else(|| format!("codex-tool-{}", stream_seq)),
                            name: tool_call.name.clone(),
                            arguments: tool_call.arguments.clone(),
                            result: tool_call.result.clone(),
                            diff_context: diff_context_value.clone(),
                            parent_tool_use_id: None,
                        },
                    )
                    .await;

                persist_assistant_message_snapshot(
                    &chat_message_repo,
                    &assistant_message_id,
                    &response_text,
                    &tool_calls,
                    &content_blocks,
                )
                .await;
                persist_timeline_snapshot(
                    &chat_timeline_repo,
                    &conversation_id_str,
                    &assistant_message_id,
                    &content_blocks,
                    ChatTimelineItemStatus::Streaming,
                )
                .await;

                let result_preview = build_live_tool_result_preview_for_tool_call(
                    &conversation_id_str,
                    assistant_message_id.as_deref(),
                    &tool_call,
                );
                let argument_preview = assistant_message_id.as_deref().and_then(|message_id| {
                    let detail_ref = tool_detail_ref(
                        &conversation_id_str,
                        message_id,
                        tool_call.id.as_deref(),
                        None,
                    );
                    build_live_tool_argument_preview(
                        &tool_call,
                        diff_context_value.as_ref(),
                        Some(detail_ref),
                    )
                });

                if let Some(ref handle) = app_handle {
                    let _ = handle.emit(
                        events::AGENT_TOOL_CALL,
                        AgentToolCallPayload::from_completed_tool_call(
                            &tool_call,
                            result_preview.as_ref(),
                            argument_preview.as_ref(),
                            &conversation_id_str,
                            &context_type_str,
                            &context_id_str,
                            agent_run_id.as_deref(),
                            diff_context_value,
                            None,
                            stream_seq,
                        ),
                    );
                    stream_seq += 1;
                }

                if matches!(
                    context_type,
                    ChatContextType::TaskExecution | ChatContextType::Merge
                ) {
                    if let (Some(ref repo), Some(ref task_id)) =
                        (&activity_event_repo, &task_id_for_persistence)
                    {
                        let event = ActivityEvent::new_task_event(
                            task_id.clone(),
                            ActivityEventType::ToolCall,
                            tool_call.name.clone(),
                        );
                        let event = if let Some(ref t_repo) = task_repo {
                            if let Ok(Some(task)) = t_repo.get_by_id(task_id).await {
                                event.with_status(task.internal_status)
                            } else {
                                event
                            }
                        } else {
                            event
                        };
                        let _ = repo.save(event).await;
                    }
                }

                if snapshot.phase == CodexToolCallPhase::Completed
                    && is_completion_tool_name(&tool_call.name)
                {
                    if extract_codex_error(&event).is_none()
                        && completion_tool_result_accepted(tool_call.result.as_ref())
                    {
                        completion_signal_tracker.mark_completion_called();
                    } else {
                        tracing::warn!(
                            conversation_id = %conversation_id_str,
                            context_id,
                            tool_name = %tool_call.name,
                            result = ?tool_call.result,
                            "Codex completion tool returned an error; not entering shutdown grace period"
                        );
                    }
                }
            }

            if let Some(command_execution) = extract_codex_command_execution(&event) {
                if let Some(exit_code) = command_execution.exit_code {
                    if exit_code != 0 {
                        local_tool_errors.push(
                            command_execution
                                .aggregated_output
                                .clone()
                                .unwrap_or_else(|| {
                                    format!(
                                        "Codex command_execution failed with exit code {exit_code}"
                                    )
                                }),
                        );
                    }
                }
            }

            if let Some(error) = extract_codex_error(&event) {
                if crate::infrastructure::agents::codex::stream_processor::is_non_fatal_mcp_resource_probe_error(
                    &event,
                    &error.message,
                ) {
                    continue;
                }
                match error.source {
                    CodexErrorSource::Runtime => runtime_errors.push(error.message),
                    CodexErrorSource::McpTool => local_tool_errors.push(error.message),
                }
            }

            if let Some(event_usage) = extract_codex_usage(&event) {
                let capture = if event_usage.source == CodexUsageSource::CumulativeTotal
                    && !run_session_attribution_ready
                {
                    None
                } else {
                    normalize_codex_stream_usage_for_persistence(
                        agent_run_usage_from_codex_usage(event_usage.usage),
                        event_usage.source,
                        &agent_run_repo,
                        conversation_id,
                        agent_run_id.as_deref(),
                        session_id.as_deref(),
                    )
                    .await
                };
                if let Some(capture) = capture {
                    usage = capture.normalized.clone();
                    let persisted = persist_usage_capture_run_first(
                        &agent_run_repo,
                        &agent_run_id,
                        &chat_message_repo,
                        &assistant_message_id,
                        &capture,
                    )
                    .await;
                    if persisted && last_emitted_capture.as_ref() != Some(&capture) {
                        emit_usage_updated_event(
                            &app_handle,
                            &conversation_id_str,
                            &context_type_str,
                            &context_id_str,
                        );
                        last_emitted_capture = Some(capture);
                    }
                }
            }

            if event.event_type == "turn.completed" {
                codex_turn_completed = true;
                let _ = child.start_kill();
                break;
            }
        } else if lines_seen > 0 && last_parsed_at.elapsed() >= timeout_config.parse_stall_timeout {
            let _ = child.kill().await;
            flush_content_before_error(
                &chat_message_repo,
                &assistant_message_id,
                &response_text,
                &tool_calls,
                &content_blocks,
            )
            .await;
            return Err(StreamError::ParseStall {
                context_type,
                elapsed_secs: timeout_config.parse_stall_timeout.as_secs(),
                lines_seen,
                lines_parsed,
            });
        }

        if last_flush.elapsed() >= FLUSH_INTERVAL {
            persist_assistant_message_snapshot(
                &chat_message_repo,
                &assistant_message_id,
                &response_text,
                &tool_calls,
                &content_blocks,
            )
            .await;
            persist_timeline_snapshot(
                &chat_timeline_repo,
                &conversation_id_str,
                &assistant_message_id,
                &content_blocks,
                ChatTimelineItemStatus::Streaming,
            )
            .await;
            last_flush = std::time::Instant::now();
        }

        if lines_parsed > 0 && last_heartbeat.elapsed() >= HEARTBEAT_INTERVAL {
            if let (Some(ref registry), Some(ref key), Some(ref run_id)) =
                (&running_agent_registry, &heartbeat_key, &agent_run_id)
            {
                let _ = registry
                    .update_heartbeat(key, run_id, chrono::Utc::now())
                    .await;
            }
            last_heartbeat = std::time::Instant::now();
        }
    }

    let (stderr_content, status) = if codex_turn_completed {
        detach_codex_completed_process_cleanup(child, stderr_task);
        (String::new(), None)
    } else {
        let raw = stderr_task.await.unwrap_or_default();
        let stderr_content = crate::utils::secret_redactor::redact(&raw);
        let status = child.wait().await.map_err(|error| StreamError::AgentExit {
            exit_code: None,
            stderr: error.to_string(),
        })?;
        (stderr_content, Some(status))
    };
    let status_success = status
        .as_ref()
        .map(|status| status.success())
        .unwrap_or(true);
    let status_code = status.as_ref().and_then(|status| status.code());
    let status_signal = status
        .as_ref()
        .and_then(|status| process_exit_details(status).exit_signal);
    let stderr_preview = truncate_str(stderr_content.trim(), 2000);

    if !status_success || (!codex_turn_completed && response_text.trim().is_empty()) {
        tracing::warn!(
            conversation_id = %conversation_id_str,
            agent_run_id = agent_run_id.as_deref().unwrap_or("unknown"),
            exit_code = ?status_code,
            exit_signal = ?status_signal,
            stdout_lines_seen = lines_seen,
            stdout_lines_parsed = lines_parsed,
            runtime_error_count = runtime_errors.len(),
            local_tool_error_count = local_tool_errors.len(),
            stderr_len = stderr_content.len(),
            stderr_preview = %stderr_preview,
            "Codex process reached terminal stream state"
        );
    }

    let outcome = StreamOutcome {
        response_text,
        tool_calls,
        content_blocks,
        session_id: session_id.clone(),
        usage,
        usage_provenance: last_emitted_capture
            .as_ref()
            .map(|capture| capture.provenance),
        stderr_text: stderr_content.clone(),
        turns_finalized: 0,
        completion_applied: false,
        execution_slot_held: true,
        completion_tool_called: completion_signal_tracker.was_called(),
        silent_interactive_exit: false,
        mode_handoff_exit: false,
    };

    flush_content_before_error(
        &chat_message_repo,
        &assistant_message_id,
        &outcome.response_text,
        &outcome.tool_calls,
        &outcome.content_blocks,
    )
    .await;
    persist_timeline_snapshot(
        &chat_timeline_repo,
        &conversation_id_str,
        &assistant_message_id,
        &outcome.content_blocks,
        if status_success || codex_turn_completed || outcome.has_meaningful_output() {
            ChatTimelineItemStatus::Finalized
        } else {
            ChatTimelineItemStatus::Error
        },
    )
    .await;
    if cancellation_token.is_cancelled() {
        return Err(StreamError::Cancelled {
            turns_finalized: 0,
            completion_tool_called: false,
        });
    }
    if let Some(stream_error) = super::chat_service_errors::classify_codex_stream_failure(
        &runtime_errors,
        &local_tool_errors,
        status_code,
        codex_turn_completed || completion_signal_tracker.was_called(),
    ) {
        return Err(stream_error);
    }

    if !status_success
        && !codex_turn_completed
        && !outcome.has_meaningful_output()
        && !completion_signal_tracker.was_called()
    {
        let stderr_trimmed = outcome.stderr_text.trim().to_string();
        if let Some(provider_error) =
            super::chat_service_errors::classify_provider_error(&stderr_trimmed)
        {
            return Err(provider_error);
        }
        if !stderr_trimmed.is_empty() {
            let meaningful_stderr =
                super::chat_service_errors::meaningful_agent_exit_stderr(&stderr_trimmed);
            if !meaningful_stderr.is_empty() {
                return Err(StreamError::AgentExit {
                    exit_code: status_code,
                    stderr: stderr_trimmed,
                });
            }
        }
    }

    if !outcome.has_meaningful_output() && !completion_signal_tracker.was_called() {
        if persist_conversation_provider_session_ref {
            if let Some(repository) = conversation_repo.as_ref() {
                if let Err(error) = repository.clear_provider_session_ref(conversation_id).await {
                    tracing::warn!(
                        conversation_id = %conversation_id,
                        error = %error,
                        "Failed to clear provider session after empty Codex completion"
                    );
                }
            }
        }
        tracing::warn!(
            context_type = %context_type,
            context_id,
            conversation_id = %conversation_id,
            exit_code = ?status_code,
            status_success,
            codex_turn_completed,
            runtime_error_count = runtime_errors.len(),
            local_tool_error_count = local_tool_errors.len(),
            "Codex terminal stream produced no meaningful completion"
        );
        return Err(StreamError::NoOutput {
            context_type,
            exit_code: status_code,
            exit_signal: status_signal,
            stderr: stderr_preview.to_string(),
        });
    }

    if outcome.tool_calls.is_empty() {
        if let Some(provider_error) =
            super::chat_service_errors::classify_provider_error_from_assistant_content(
                &outcome.response_text,
            )
        {
            return Err(provider_error);
        }
    }

    Ok(outcome)
}

fn detach_codex_completed_process_cleanup(
    mut child: tokio::process::Child,
    stderr_task: tokio::task::JoinHandle<String>,
) {
    std::mem::drop(tokio::spawn(async move {
        let _ = child.start_kill();
        if tokio::time::timeout(std::time::Duration::from_secs(5), child.wait())
            .await
            .is_err()
        {
            let _ = child.kill().await;
            let _ = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await;
        }
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), stderr_task).await;
    }));
}

/// Determines whether the stream should be killed on timeout.
///
/// Returns `true` = kill (terminate with error), `false` = reset timeout and continue.
/// Ordering mirrors the actual `Err(_)` branch in `process_stream_background`:
/// idle-cap → question_state → interactive_turns → PID-alive → active_tasks
/// → completion_grace → kill
///
/// `idle_elapsed` is the duration since the last meaningful activity (parsed
/// stream event, tool call, interactive turn, or stdin message). Using
/// idle-based timing instead of absolute process age ensures long-running
/// interactive/IPR agents are not killed while they are actively working.
///
/// This pure function is extracted for unit testability. Side effects (tracing,
/// heartbeat emission) remain in the calling code.
#[doc(hidden)]
pub fn should_kill_on_timeout(
    idle_elapsed: std::time::Duration,
    max_idle: std::time::Duration,
    has_pending_question: bool,
    is_interactive_turn: bool,
    pid_alive: bool,
    child_exited: bool,
    has_active_tasks: bool,
    is_completion_grace_period: bool,
) -> bool {
    // 1. Idle cap: kill only when the agent has been idle longer than max_idle
    if idle_elapsed > max_idle {
        return true;
    }
    // 2. Pending question bypass (existing)
    if has_pending_question {
        return false;
    }
    // 3. Interactive turn bypass (existing)
    if is_interactive_turn {
        return false;
    }
    // 4. PID-alive bypass (only if child hasn't exited — PID recycling guard)
    if pid_alive && !child_exited {
        return false;
    }
    // 5. Active task bypass (existing)
    if has_active_tasks {
        return false;
    }
    // 6. Completion grace bypass (post-completion quiet shutdown window)
    if is_completion_grace_period {
        return false;
    }
    // 7. Default: kill
    true
}

/// Emit an `agent:heartbeat` event to the frontend.
///
/// Used by all timeout-bypass sites (PID-alive and active_tasks) to prevent
/// the frontend watchdog from false-positive stall detection.
fn emit_heartbeat<R: Runtime>(
    app_handle: &Option<AppHandle<R>>,
    conversation_id: &str,
    context_id: &str,
    reason: &str,
    extra: Option<serde_json::Value>,
) {
    if let Some(ref handle) = app_handle {
        let mut payload = serde_json::json!({
            "conversation_id": conversation_id,
            "context_id": context_id,
            "reason": reason,
        });
        if let Some(extra_fields) = extra {
            if let (Some(obj), Some(extra_obj)) =
                (payload.as_object_mut(), extra_fields.as_object())
            {
                for (k, v) in extra_obj {
                    obj.insert(k.clone(), v.clone());
                }
            }
        }
        let _ = handle.emit("agent:heartbeat", payload);
    }
}

#[cfg(test)]
#[path = "chat_service_streaming_tests.rs"]
mod tests;
