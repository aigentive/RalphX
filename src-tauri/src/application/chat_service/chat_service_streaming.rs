// Chat Service Streaming Logic
//
// Extracted from chat_service.rs to improve modularity and reduce file size.
// Handles background stream processing and event emission.

use std::collections::HashMap;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Runtime};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::time::{timeout, Duration};
use tracing::info;

use crate::application::question_state::QuestionState;
use crate::application::team_events;
use crate::application::team_state_tracker::TeammateStatus;
use crate::domain::agents::{
    standard_harness_behavior, AgentHarnessKind, HarnessStreamMode, ProviderSessionRef,
};
use crate::domain::entities::{
    ActivityEvent, ActivityEventType, AgentRunId, AgentRunUsage, ChatContextType,
    ChatConversationId, ChatMessageId, TaskId,
};
use crate::domain::repositories::{
    ActivityEventRepository, AgentRunRepository, ChatConversationRepository, ChatMessageRepository,
    TaskRepository,
};
use crate::domain::services::{RunningAgentKey, RunningAgentRegistry};
use crate::infrastructure::agents::claude::stream_timeouts;
use crate::infrastructure::agents::claude::{
    ContentBlockItem, DiffContext, StreamEvent, StreamProcessor, ToolCall, ToolCallStats,
};
use crate::infrastructure::agents::{
    extract_codex_agent_message, extract_codex_command_execution, extract_codex_error,
    extract_codex_file_change_snapshot, extract_codex_thread_id, extract_codex_tool_call_snapshot,
    extract_codex_usage, parse_codex_event_line, CodexErrorSource, CodexFileChange,
    CodexFileChangeSnapshot, CodexToolCallPhase, CodexToolCallSnapshot,
};
use tokio_util::sync::CancellationToken;

use super::chat_service_errors::StreamError;
use super::chat_service_types::AgentUsageUpdatedPayload;
use super::streaming_state_cache::{CachedStreamingTask, CachedToolCall, StreamingStateCache};
use super::tool_result_preview::{
    build_live_tool_result_preview_for_tool_call, build_live_tool_result_preview_for_tool_id,
    live_tool_result_activity_content, live_tool_result_activity_metadata,
};
use super::{
    event_context, events, has_meaningful_output, AgentChunkPayload, AgentHookPayload,
    AgentTaskCompletedPayload, AgentTaskStartedPayload, AgentToolCallPayload,
    AgentToolCallPreviewFields,
};
use crate::utils::truncate_str;

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

async fn persist_agent_run_usage(
    agent_run_repo: &Option<Arc<dyn AgentRunRepository>>,
    agent_run_id: &Option<String>,
    usage: &AgentRunUsage,
) {
    if usage.is_empty() {
        return;
    }

    if let (Some(repo), Some(run_id)) = (agent_run_repo.as_ref(), agent_run_id.as_ref()) {
        let _ = repo
            .update_usage(&AgentRunId::from_string(run_id.clone()), usage)
            .await;
    }
}

async fn persist_assistant_message_usage(
    chat_message_repo: &Option<Arc<dyn ChatMessageRepository>>,
    assistant_message_id: &Option<String>,
    usage: &AgentRunUsage,
) {
    if usage.is_empty() {
        return;
    }

    if let (Some(repo), Some(message_id)) =
        (chat_message_repo.as_ref(), assistant_message_id.as_ref())
    {
        let _ = repo
            .update_usage(&ChatMessageId::from_string(message_id.clone()), usage)
            .await;
    }
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
                    pending_changes.insert(
                        tool_id.clone(),
                        PendingCodexFileChange {
                            path: change.path.clone(),
                            kind: change.kind.clone(),
                            old_content: std::fs::read_to_string(&change.path).ok(),
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

fn add_usage_u64(total: &mut Option<u64>, value: Option<u64>) {
    if let Some(value) = value {
        *total = Some(total.unwrap_or(0) + value);
    }
}

fn accumulate_codex_usage(
    total: &mut AgentRunUsage,
    usage: crate::infrastructure::agents::CodexUsage,
) {
    add_usage_u64(&mut total.input_tokens, usage.input_tokens);
    add_usage_u64(&mut total.cache_read_tokens, usage.cached_input_tokens);
    add_usage_u64(&mut total.output_tokens, usage.output_tokens);
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
    /// Teammate name (set when streaming a team member's output).
    #[allow(dead_code)]
    pub teammate_name: Option<String>,
    /// Teammate display color (set when streaming a team member's output).
    #[allow(dead_code)]
    pub teammate_color: Option<String>,
}

impl StreamTimeoutConfig {
    /// Returns timeout thresholds appropriate for the given context type.
    pub fn for_context(context_type: &ChatContextType) -> Self {
        let cfg = stream_timeouts();
        match context_type {
            ChatContextType::Merge => Self {
                line_read_timeout: Duration::from_secs(cfg.merge_line_read_secs),
                parse_stall_timeout: Duration::from_secs(cfg.merge_parse_stall_secs),
                teammate_name: None,
                teammate_color: None,
            },
            ChatContextType::Review => Self {
                line_read_timeout: Duration::from_secs(cfg.review_line_read_secs),
                parse_stall_timeout: Duration::from_secs(cfg.review_parse_stall_secs),
                teammate_name: None,
                teammate_color: None,
            },
            // TaskExecution, Ideation, Task, Project — generous defaults
            _ => Self {
                line_read_timeout: Duration::from_secs(cfg.default_line_read_secs),
                parse_stall_timeout: Duration::from_secs(cfg.default_parse_stall_secs),
                teammate_name: None,
                teammate_color: None,
            },
        }
    }

    /// Attach team member identity to this config (builder pattern).
    #[allow(dead_code)]
    pub fn with_teammate(mut self, name: String, color: String) -> Self {
        self.teammate_name = Some(name);
        self.teammate_color = Some(color);
        self
    }
}

#[derive(Debug, Clone)]
pub struct StreamOutcome {
    pub response_text: String,
    pub tool_calls: Vec<ToolCall>,
    pub content_blocks: Vec<ContentBlockItem>,
    pub session_id: Option<String>,
    pub usage: AgentRunUsage,
    pub stderr_text: String,
    /// Number of turns fully finalized during interactive streaming
    /// (via `TurnComplete` events). When > 0 and `response_text` is empty,
    /// the post-loop caller should skip re-finalization and duplicate
    /// `run_completed` emission (or `turn_completed` in interactive mode).
    pub turns_finalized: usize,
    /// Whether the execution slot is still held when the stream exits.
    /// False when TurnComplete decremented the slot and no new message arrived
    /// to re-increment it (process was idle between turns at exit time).
    /// Used by the caller to prevent double-decrement in on_exit.
    pub execution_slot_held: bool,
    /// True when the process exited while idle between interactive turns.
    /// Suppresses queue processing and run_completed emission is forced.
    pub silent_interactive_exit: bool,
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
    mut assistant_message_id: Option<String>,
    question_state: Option<Arc<QuestionState>>,
    cancellation_token: CancellationToken,
    team_service: Option<std::sync::Arc<crate::application::TeamService>>,
    team_mode: bool,
    streaming_state_cache: StreamingStateCache,
    running_agent_registry: Option<Arc<dyn RunningAgentRegistry>>,
    agent_run_repo: Option<Arc<dyn AgentRunRepository>>,
    agent_run_id: Option<String>,
    execution_state: Option<Arc<crate::commands::ExecutionState>>,
    conversation_repo: Option<Arc<dyn ChatConversationRepository>>,
    split_verification_transcript: bool,
    persist_conversation_provider_session_ref: bool,
) -> Result<StreamOutcome, StreamError> {
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

    let mut timeout_config = StreamTimeoutConfig::for_context(&context_type);
    // Team leads wait long periods while teammates work — use team-specific timeout
    let stream_cfg = stream_timeouts();
    if team_mode {
        timeout_config.line_read_timeout = Duration::from_secs(stream_cfg.team_line_read_secs);
        timeout_config.parse_stall_timeout = Duration::from_secs(stream_cfg.team_parse_stall_secs);
    }
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
    // Wall-clock cap: hard kill after max_wall_clock_secs regardless of PID state
    let stream_start = std::time::Instant::now();
    let max_wall_clock = std::time::Duration::from_secs(stream_cfg.max_wall_clock_secs);
    let completion_grace_duration =
        std::time::Duration::from_secs(stream_cfg.completion_grace_secs);

    // Debounced flush for incremental persistence (every 2 seconds)
    let mut last_flush = std::time::Instant::now();
    const FLUSH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

    // Throttled heartbeat: update last_active_at every 5s on any parsed event
    let heartbeat_key = running_agent_registry
        .as_ref()
        .map(|_| RunningAgentKey::new(context_type.to_string(), context_id));
    let mut last_heartbeat = std::time::Instant::now();
    const HEARTBEAT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

    // Track Task tool_use_id → (team_name, teammate_name) for teammate lifecycle
    let mut teammate_task_map: HashMap<String, (String, String)> = HashMap::new();

    // Track active subagent tasks (Task tool calls) to prevent timeout during sidechain work.
    // When the lead spawns in-process subagents, stdout goes silent — this tracker
    // lets the timeout handler know work is still happening.
    let mut active_task_tracker = ActiveTaskTracker::default();
    let mut completion_signal_tracker = CompletionSignalTracker::default();
    let mut last_emitted_usage = AgentRunUsage::default();

    // Count of turns fully finalized in the loop (interactive mode).
    // Used to tell the caller whether post-loop finalization should be skipped.
    let mut turns_finalized: usize = 0;

    // When true, the process is legitimately idle between interactive turns
    // (TurnComplete received, waiting for next stdin message). The timeout
    // handler should kill silently instead of returning an error.
    let mut between_interactive_turns: bool = false;
    // Set to true when an interactive process is killed while idle between
    // turns. Suppresses post-loop error returns so the exit is silent.
    let mut silent_interactive_exit: bool = false;
    // Track whether we've already persisted session_id to the DB (only need once)
    let mut session_id_persisted: bool = false;

    loop {
        // Race line-read (with timeout) against cancellation token
        let line = tokio::select! {
            biased;
            _ = cancellation_token.cancelled() => {
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
                flush_content_before_error(
                    &chat_message_repo, &assistant_message_id,
                    &processor.response_text, &processor.tool_calls, &processor.content_blocks,
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
                            stream_start.elapsed(),
                            max_wall_clock,
                            has_pending_question,
                            between_interactive_turns,
                            pid_alive,
                            child_exited,
                            active_task_tracker.has_active_tasks(),
                            is_completion_grace_period,
                        ) {
                            if stream_start.elapsed() > max_wall_clock {
                                tracing::warn!(
                                    conversation_id = %conversation_id_str,
                                    elapsed_secs = stream_start.elapsed().as_secs(),
                                    "Wall-clock cap reached — killing agent"
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
                    StreamEvent::TeamMessageSent {
                        sender,
                        recipient,
                        content,
                        message_type,
                    } => {
                        tracing::info!(
                            conversation_id = %conversation_id_str,
                            sender = %sender,
                            recipient = ?recipient,
                            content_len = content.len(),
                            message_type = %message_type,
                            "[STREAM_EVT] TeamMessageSent — lead captured team message"
                        );
                    }
                    StreamEvent::TeamCreated { team_name, .. } => {
                        tracing::info!(
                            conversation_id = %conversation_id_str,
                            team_name = %team_name,
                            "[STREAM_EVT] TeamCreated"
                        );
                    }
                    StreamEvent::TeammateSpawned {
                        teammate_name,
                        team_name,
                        ..
                    } => {
                        tracing::info!(
                            conversation_id = %conversation_id_str,
                            teammate_name = %teammate_name,
                            team_name = %team_name,
                            "[STREAM_EVT] TeammateSpawned"
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
                        teammate_name,
                        team_name,
                        ..
                    } => {
                        tracing::debug!(
                            conversation_id = %conversation_id_str,
                            tool_use_id = %tool_use_id,
                            description = ?description,
                            teammate_name = ?teammate_name,
                            team_name = ?team_name,
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
                        // Update streaming state cache
                        streaming_state_cache
                            .append_text(&conversation_id_str, &text)
                            .await;

                        if let Some(ref handle) = app_handle {
                            // Unified event
                            let _ = handle.emit(
                                events::AGENT_CHUNK,
                                AgentChunkPayload {
                                    text: text.clone(),
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
                    StreamEvent::ToolCallStarted {
                        name,
                        id,
                        parent_tool_use_id,
                    } => {
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
                        if is_completion_tool_name(&tool_call.name) {
                            completion_signal_tracker.mark_completion_called();
                            tracing::info!(
                                conversation_id = %conversation_id_str,
                                context_id,
                                tool_name = %tool_call.name,
                                grace_secs = completion_grace_duration.as_secs(),
                                "Completion tool called, entering shutdown grace period"
                            );
                        }

                        // Capture old file content for Edit/Write tool calls
                        let name_lower = tool_call.name.to_lowercase();
                        if name_lower == "edit" || name_lower == "write" {
                            if let Some(file_path) = tool_call
                                .arguments
                                .get("file_path")
                                .and_then(|v| v.as_str())
                            {
                                let old_content = std::fs::read_to_string(file_path).ok();
                                let diff_ctx = DiffContext {
                                    old_content,
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
                                AgentToolCallPayload {
                                    tool_name: tool_call.name.clone(),
                                    tool_id: tool_call.id.clone(),
                                    arguments: tool_call.arguments.clone(),
                                    result: None,
                                    preview: AgentToolCallPreviewFields::default(),
                                    conversation_id: conversation_id_str.clone(),
                                    context_type: context_type_str.clone(),
                                    context_id: context_id_str.clone(),
                                    diff_context: diff_context_value,
                                    parent_tool_use_id: parent_tool_use_id.clone(),
                                    seq: stream_seq,
                                },
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
                        tracing::info!(
                            conversation_id = %conversation_id_str,
                            ?session_id,
                            "TurnComplete: finalizing assistant message for interactive turn"
                        );

                        // Finalize the current assistant message with accumulated content
                        if let (Some(ref repo), Some(ref msg_id)) =
                            (&chat_message_repo, &assistant_message_id)
                        {
                            let role =
                                super::chat_service_helpers::get_assistant_role(&context_type)
                                    .to_string();
                            super::chat_service_send_background::finalize_structured_assistant_message(
                                repo,
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
                            let turn_usage = processor.current_turn_usage();
                            persist_assistant_message_usage(
                                &chat_message_repo,
                                &assistant_message_id,
                                &turn_usage,
                            )
                            .await;
                            if turn_usage != last_emitted_usage {
                                emit_usage_updated_event(
                                    &app_handle,
                                    &conversation_id_str,
                                    &context_type_str,
                                    &context_id_str,
                                );
                                last_emitted_usage = turn_usage;
                            }
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

                        // Complete the agent_run DB record so the recovery poll
                        // (`useChatRecovery`) no longer sees status=running.
                        if let (Some(ref repo), Some(ref run_id)) = (&agent_run_repo, &agent_run_id)
                        {
                            let _ = repo.complete(&AgentRunId::from_string(run_id)).await;
                        }

                        // Emit turn_completed (NOT run_completed) for interactive turns.
                        // The process is still alive and waiting for stdin — emitting
                        // run_completed would cause the frontend to set isAgentRunning=false,
                        // making the next user message go through sendAgentMessage (which
                        // creates a new conversation for TaskExecution contexts) instead
                        // of queueAgentMessage (which delivers via existing stdin).
                        if let Some(ref handle) = app_handle {
                            let provider_session_id = session_id.clone();
                            let _ = handle.emit(
                                super::chat_service_types::events::AGENT_TURN_COMPLETED,
                                super::chat_service_types::AgentRunCompletedPayload::with_provider_session(
                                    conversation_id_str.clone(),
                                    context_type_str.clone(),
                                    context_id_str.clone(),
                                    Some(harness),
                                    provider_session_id,
                                    None,
                                ),
                            );
                        }

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
                        teammate_name: tm_name,
                        team_name: tm_team,
                    } => {
                        // Track active subagent tasks for timeout bypass
                        active_task_tracker.task_started();

                        // Track teammate Task calls for lifecycle management
                        if let (Some(ref tn), Some(ref tt)) = (&tm_name, &tm_team) {
                            teammate_task_map.insert(tool_use_id.clone(), (tt.clone(), tn.clone()));

                            // Update status to Running via TeamService (persistence + events)
                            if let Some(ref service) = team_service {
                                let _ = service
                                    .update_teammate_status(tt, tn, TeammateStatus::Running)
                                    .await;
                            }

                            // Emit agent:run_started with teammate_name for frontend
                            if let Some(ref handle) = app_handle {
                                let _ = handle.emit(
                                    events::AGENT_RUN_STARTED,
                                    serde_json::json!({
                                        "teammate_name": tn,
                                        "team_name": tt,
                                        "context_type": context_type_str,
                                        "context_id": context_id_str,
                                    }),
                                );
                            }
                        }

                        // Update streaming state cache with new task
                        let cached_task = CachedStreamingTask {
                            tool_use_id: tool_use_id.clone(),
                            description: description.clone(),
                            subagent_type: subagent_type.clone(),
                            model: model.clone(),
                            status: "running".to_string(),
                            agent_id: None,
                            teammate_name: tm_name.clone(),
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
                        };
                        streaming_state_cache
                            .add_task(&conversation_id_str, cached_task)
                            .await;

                        if let Some(ref handle) = app_handle {
                            let _ = handle.emit(
                                events::AGENT_TASK_STARTED,
                                AgentTaskStartedPayload {
                                    tool_use_id,
                                    tool_name,
                                    description,
                                    subagent_type,
                                    model,
                                    teammate_name: tm_name,
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

                        // Check if this completes a teammate Task
                        let tm_name_for_payload =
                            if let Some((tt, tn)) = teammate_task_map.remove(&tool_use_id) {
                                // Update status to Idle via TeamService (persistence + events)
                                if let Some(ref service) = team_service {
                                    let _ = service
                                        .update_teammate_status(&tt, &tn, TeammateStatus::Idle)
                                        .await;
                                }

                                // Emit agent:run_completed with teammate_name for frontend
                                if let Some(ref handle) = app_handle {
                                    let _ = handle.emit(
                                        events::AGENT_RUN_COMPLETED,
                                        serde_json::json!({
                                            "teammate_name": tn,
                                            "team_name": tt,
                                            "context_type": context_type_str,
                                            "context_id": context_id_str,
                                        }),
                                    );
                                }

                                Some(tn)
                            } else {
                                None
                            };

                        if let Some(ref handle) = app_handle {
                            let _ = handle.emit(
                                events::AGENT_TASK_COMPLETED,
                                AgentTaskCompletedPayload {
                                    tool_use_id,
                                    agent_id,
                                    status: Some("completed".to_string()),
                                    total_duration_ms,
                                    total_tokens,
                                    total_tool_use_count,
                                    teammate_name: tm_name_for_payload,
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

                    StreamEvent::TeamCreated {
                        team_name,
                        config_path: _,
                    } => {
                        // Create team via TeamService (persistence + events)
                        if let Some(ref service) = team_service {
                            if !service.team_exists(&team_name).await {
                                let _ = service
                                    .create_team(&team_name, &context_id_str, &context_type_str)
                                    .await;
                            }
                        } else if let Some(ref handle) = app_handle {
                            // Fallback: emit event directly if no service available
                            team_events::emit_team_created(
                                handle,
                                &team_name,
                                &context_id_str,
                                &context_type_str,
                            );
                        }

                        // Dynamic team_mode upgrade: when the lead creates a team mid-session,
                        // upgrade the line-read timeout from default (600s) to team (3600s).
                        // The lead was spawned before the team existed, so team_mode was false
                        // at spawn time. Without this, the lead gets killed after 10 min idle.
                        if !team_mode {
                            let cfg = stream_timeouts();
                            let old_secs = timeout_config.line_read_timeout.as_secs();
                            timeout_config.line_read_timeout =
                                Duration::from_secs(cfg.team_line_read_secs);
                            timeout_config.parse_stall_timeout =
                                Duration::from_secs(cfg.team_parse_stall_secs);
                            tracing::info!(
                                conversation_id = %conversation_id_str,
                                team_name = %team_name,
                                old_timeout_secs = old_secs,
                                new_timeout_secs = cfg.team_line_read_secs,
                                "[TEAM_TIMEOUT] Upgraded line-read timeout on TeamCreated"
                            );
                        }
                    }
                    StreamEvent::TeammateSpawned {
                        teammate_name,
                        team_name,
                        agent_id: _,
                        model,
                        color,
                        prompt: _,
                        agent_type: _,
                    } => {
                        // Register teammate via TeamService (persistence + events).
                        // May already exist from approve_team_plan — add_teammate is idempotent.
                        // NOTE: CLI worker processes are spawned in approve_team_plan (teams.rs),
                        // NOT here, to avoid double-spawn when both paths fire.
                        if let Some(ref service) = team_service {
                            let _ = service
                                .add_teammate(
                                    &team_name,
                                    &teammate_name,
                                    &color,
                                    &model,
                                    "team-member",
                                )
                                .await;
                        }
                        // Always re-emit team:teammate_spawned so the frontend creates the
                        // filter tab immediately. The teammate may already be registered from
                        // approve_team_plan (add_teammate returns TeammateAlreadyExists), but
                        // we re-emit here so the frontend recovers if it missed the initial event.
                        // Try to include conversation_id if the stream processor already created one.
                        let conv_id = if let Some(ref service) = team_service {
                            service
                                .tracker()
                                .get_team_status(&team_name)
                                .await
                                .ok()
                                .and_then(|s| {
                                    s.teammates
                                        .iter()
                                        .find(|t| t.name == teammate_name)
                                        .and_then(|t| t.conversation_id.clone())
                                })
                        } else {
                            None
                        };
                        if let Some(ref handle) = app_handle {
                            team_events::emit_teammate_spawned(
                                handle,
                                &team_name,
                                &teammate_name,
                                &color,
                                &model,
                                "team-member",
                                &context_type_str,
                                &context_id_str,
                                conv_id.as_deref(),
                            );
                        }
                    }
                    StreamEvent::TeamMessageSent {
                        sender,
                        recipient,
                        content,
                        message_type,
                    } => {
                        // Persist message and emit full-payload event via TeamService
                        use crate::application::team_state_tracker::TeamMessageType;

                        let msg_type = match message_type.as_str() {
                            "broadcast" => TeamMessageType::Broadcast,
                            _ => TeamMessageType::TeammateMessage,
                        };

                        if let Some(ref service) = team_service {
                            let _ = service
                                .add_teammate_message(
                                    // Derive team_name from active teams
                                    &{
                                        let teams = service.list_teams().await;
                                        teams.into_iter().next().unwrap_or_default()
                                    },
                                    &sender,
                                    recipient.as_deref(),
                                    &content,
                                    msg_type,
                                )
                                .await;
                        } else if let Some(ref handle) = app_handle {
                            // Fallback: emit event directly without persistence
                            let _ = handle.emit(
                                events::TEAM_MESSAGE,
                                serde_json::json!({
                                    "sender": sender,
                                    "recipient": recipient,
                                    "content": content,
                                    "message_type": message_type,
                                    "context_type": context_type_str,
                                    "context_id": context_id_str,
                                }),
                            );
                        }
                    }
                    StreamEvent::TeamDeleted { team_name } => {
                        // Disband team via TeamService (persistence + events)
                        if let Some(ref service) = team_service {
                            let _ = service.disband_team(&team_name).await;
                        } else if let Some(ref handle) = app_handle {
                            // Fallback: emit event directly if no service available
                            team_events::emit_team_disbanded(
                                handle,
                                &team_name,
                                &context_type_str,
                                &context_id_str,
                            );
                        }
                    }

                    StreamEvent::ToolResultReceived {
                        tool_use_id,
                        result,
                        parent_tool_use_id,
                    } => {
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
                stream_start.elapsed(),
                max_wall_clock,
                has_pending_question,
                false, // parse stall path has no interactive_turns bypass
                pid_alive,
                child_exited,
                active_task_tracker.has_active_tasks(),
                is_completion_grace_period,
            ) {
                if stream_start.elapsed() > max_wall_clock {
                    tracing::warn!(
                        conversation_id = %conversation_id_str,
                        elapsed_secs = stream_start.elapsed().as_secs(),
                        "Wall-clock cap reached in parse stall path — killing agent"
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

        // Debounced flush: persist accumulated content every 2s for crash recovery
        if last_flush.elapsed() >= FLUSH_INTERVAL {
            persist_assistant_message_snapshot(
                &chat_message_repo,
                &assistant_message_id,
                &processor.response_text,
                &processor.tool_calls,
                &processor.content_blocks,
            )
            .await;
            let current_turn_usage = processor.current_turn_usage();
            persist_assistant_message_usage(
                &chat_message_repo,
                &assistant_message_id,
                &current_turn_usage,
            )
            .await;
            persist_agent_run_usage(&agent_run_repo, &agent_run_id, &current_turn_usage).await;
            if current_turn_usage != last_emitted_usage {
                emit_usage_updated_event(
                    &app_handle,
                    &conversation_id_str,
                    &context_type_str,
                    &context_id_str,
                );
                last_emitted_usage = current_turn_usage;
            }
            last_flush = std::time::Instant::now();
        }

        // Throttled heartbeat: write last_active_at every 5s on any parsed event
        if lines_parsed > 0 && last_heartbeat.elapsed() >= HEARTBEAT_INTERVAL {
            if let (Some(ref registry), Some(ref key)) = (&running_agent_registry, &heartbeat_key) {
                registry.update_heartbeat(key, chrono::Utc::now()).await;
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
        stderr_text: stderr_content,
        turns_finalized,
        execution_slot_held,
        silent_interactive_exit,
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
    persist_assistant_message_usage(&chat_message_repo, &assistant_message_id, &outcome.usage)
        .await;
    persist_agent_run_usage(&agent_run_repo, &agent_run_id, &outcome.usage).await;

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
            super::chat_service_errors::classify_provider_error(&outcome.response_text)
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
        return Err(StreamError::AgentExit {
            exit_code: status.code(),
            stderr: error_msg,
        });
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
    let mut usage = AgentRunUsage::default();
    let mut lines_seen = 0usize;
    let mut lines_parsed = 0usize;
    let mut stream_seq = 0u64;
    let mut last_parsed_at = std::time::Instant::now();
    let stream_start = std::time::Instant::now();
    let max_wall_clock = std::time::Duration::from_secs(stream_timeouts().max_wall_clock_secs);
    let mut last_flush = std::time::Instant::now();
    const FLUSH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);
    let mut completion_signal_tracker = CompletionSignalTracker::default();
    let mut last_emitted_usage = AgentRunUsage::default();
    let mut pending_codex_file_changes: HashMap<String, PendingCodexFileChange> = HashMap::new();
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
                            stream_start.elapsed(),
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

            if let Some(thread_id) = extract_codex_thread_id(&event) {
                session_id = Some(thread_id.clone());
                if persist_conversation_provider_session_ref {
                    if let Some(ref repo) = conversation_repo {
                        let _ = repo
                            .update_provider_session_ref(
                                conversation_id,
                                &provider_session_ref_for_harness(
                                    AgentHarnessKind::Codex,
                                    thread_id,
                                ),
                            )
                            .await;
                    }
                }
            }

            if let Some(text) = extract_codex_agent_message(&event) {
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

                if let Some(ref handle) = app_handle {
                    let _ = handle.emit(
                        events::AGENT_CHUNK,
                        AgentChunkPayload {
                            text,
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

                let result_preview = build_live_tool_result_preview_for_tool_call(
                    &conversation_id_str,
                    assistant_message_id.as_deref(),
                    &tool_call,
                );

                if let Some(ref handle) = app_handle {
                    let _ = handle.emit(
                        events::AGENT_TOOL_CALL,
                        AgentToolCallPayload::from_completed_tool_call(
                            &tool_call,
                            result_preview.as_ref(),
                            &conversation_id_str,
                            &context_type_str,
                            &context_id_str,
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

                if is_completion_tool_name(&tool_call.name) {
                    completion_signal_tracker.mark_completion_called();
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
                accumulate_codex_usage(&mut usage, event_usage);
                persist_assistant_message_usage(&chat_message_repo, &assistant_message_id, &usage)
                    .await;
                persist_agent_run_usage(&agent_run_repo, &agent_run_id, &usage).await;
                if usage != last_emitted_usage {
                    emit_usage_updated_event(
                        &app_handle,
                        &conversation_id_str,
                        &context_type_str,
                        &context_id_str,
                    );
                    last_emitted_usage = usage.clone();
                }
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
            last_flush = std::time::Instant::now();
        }

        if lines_parsed > 0 && last_heartbeat.elapsed() >= HEARTBEAT_INTERVAL {
            if let (Some(ref registry), Some(ref key)) = (&running_agent_registry, &heartbeat_key) {
                registry.update_heartbeat(key, chrono::Utc::now()).await;
            }
            last_heartbeat = std::time::Instant::now();
        }
    }

    let stderr_content = {
        let raw = stderr_task.await.unwrap_or_default();
        crate::utils::secret_redactor::redact(&raw)
    };
    let status = child.wait().await.map_err(|error| StreamError::AgentExit {
        exit_code: None,
        stderr: error.to_string(),
    })?;

    let outcome = StreamOutcome {
        response_text,
        tool_calls,
        content_blocks,
        session_id: session_id.clone(),
        usage,
        stderr_text: stderr_content.clone(),
        turns_finalized: 0,
        execution_slot_held: true,
        silent_interactive_exit: false,
    };

    flush_content_before_error(
        &chat_message_repo,
        &assistant_message_id,
        &outcome.response_text,
        &outcome.tool_calls,
        &outcome.content_blocks,
    )
    .await;
    persist_assistant_message_usage(&chat_message_repo, &assistant_message_id, &outcome.usage)
        .await;
    persist_agent_run_usage(&agent_run_repo, &agent_run_id, &outcome.usage).await;

    if let Some(stream_error) = super::chat_service_errors::classify_codex_stream_failure(
        &runtime_errors,
        &local_tool_errors,
        status.code(),
    ) {
        return Err(stream_error);
    }

    if !status.success()
        && !outcome.has_meaningful_output()
        && !completion_signal_tracker.was_called()
    {
        let stderr_trimmed = outcome.stderr_text.trim().to_string();
        if let Some(provider_error) =
            super::chat_service_errors::classify_provider_error(&stderr_trimmed)
        {
            return Err(provider_error);
        }
        return Err(StreamError::AgentExit {
            exit_code: status.code(),
            stderr: stderr_trimmed,
        });
    }

    if outcome.tool_calls.is_empty() {
        if let Some(provider_error) =
            super::chat_service_errors::classify_provider_error(&outcome.response_text)
        {
            return Err(provider_error);
        }
    }

    if cancellation_token.is_cancelled() {
        return Err(StreamError::Cancelled {
            turns_finalized: 0,
            completion_tool_called: false,
        });
    }

    Ok(outcome)
}

/// Determines whether the stream should be killed on timeout.
///
/// Returns `true` = kill (terminate with error), `false` = reset timeout and continue.
/// Ordering mirrors the actual `Err(_)` branch in `process_stream_background`:
/// wall-clock → question_state → interactive_turns → PID-alive → active_tasks
/// → completion_grace → kill
///
/// This pure function is extracted for unit testability. Side effects (tracing,
/// heartbeat emission) remain in the calling code.
#[doc(hidden)]
pub fn should_kill_on_timeout(
    wall_clock_elapsed: std::time::Duration,
    max_wall_clock: std::time::Duration,
    has_pending_question: bool,
    is_interactive_turn: bool,
    pid_alive: bool,
    child_exited: bool,
    has_active_tasks: bool,
    is_completion_grace_period: bool,
) -> bool {
    // 1. Wall-clock cap overrides everything
    if wall_clock_elapsed > max_wall_clock {
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
