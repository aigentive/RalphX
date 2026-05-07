// Teammate stdout stream processor
//
// Reads stream-json output from a spawned teammate's stdout line-by-line,
// parses events via StreamProcessor, and emits Tauri events with teammate_name
// so the frontend can route them to the correct teammate in teamStore.
//
// The function spawns a tokio task that runs until stdout closes (teammate exits),
// then updates the teammate's status to Idle or Shutdown.

use std::sync::Arc;
use tauri::{AppHandle, Emitter, Runtime};

use crate::commands::ExecutionState;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::ChildStdout;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tracing::Instrument;

use crate::application::chat_service::tool_result_preview::{
    build_live_tool_result_preview, tool_detail_ref,
};
use crate::application::interactive_process_registry::{
    InteractiveProcessKey, InteractiveProcessRegistry,
};
use crate::application::team_events;
use crate::application::team_service::TeamService;
use crate::application::team_state_tracker::{
    TeamMessageType, TeamStateTracker, TeammateCost, TeammateStatus,
};
use crate::domain::entities::{
    ChatContextType, ChatConversation, ChatConversationId, ChatMessage, ChatMessageId, MessageRole,
};
use crate::domain::repositories::{ChatConversationRepository, ChatMessageRepository};
use crate::infrastructure::agents::claude::{
    format_stream_json_input, ContentBlockItem, StreamEvent, StreamProcessor, ToolCall,
};
use crate::utils::truncate_str;

/// Start a background task that reads a teammate's stdout and emits Tauri events.
///
/// Returns a `JoinHandle` that should be stored in `TeammateHandle.stream_task`
/// so it can be aborted when the teammate is stopped.
///
/// # Arguments
/// * `stdout` - The teammate process's piped stdout
/// * `exit_signal` - Fires when the Claude process exits (from the process monitor task).
///   Breaks the read loop even if a grandchild (e.g., Node.js MCP server) holds the
///   pipe open — prevents the stream processor from blocking until the 3600s timeout.
/// * `team_name` - Name of the team this teammate belongs to
/// * `teammate_name` - Unique name of the teammate (used in event payloads)
/// * `context_type` - Chat context type (e.g. "ideation")
/// * `context_id` - Chat context ID (e.g. session ID)
/// * `app_handle` - Tauri AppHandle for emitting events to the frontend
/// * `team_tracker` - TeamStateTracker for updating teammate cost/status
/// * `team_service` - Optional TeamService for message persistence and proper event emission
/// * `chat_conversation_repo` - For creating per-teammate conversations
/// * `chat_message_repo` - For persisting assistant messages with content_blocks
/// * `interactive_process_registry` - Optional registry for nudging the lead's stdin when
///   a teammate sends a message targeting the lead
/// * `execution_state` - Optional execution state for updating running count when the lead
///   is nudged (mirrors the claim_interactive_slot + increment_running pattern in chat_service)
pub fn start_teammate_stream<R: Runtime>(
    stdout: ChildStdout,
    exit_signal: oneshot::Receiver<()>,
    team_name: String,
    teammate_name: String,
    context_type: String,
    context_id: String,
    app_handle: AppHandle<R>,
    team_tracker: Arc<TeamStateTracker>,
    team_service: Option<Arc<TeamService>>,
    chat_conversation_repo: Option<Arc<dyn ChatConversationRepository>>,
    chat_message_repo: Option<Arc<dyn ChatMessageRepository>>,
    interactive_process_registry: Option<Arc<InteractiveProcessRegistry>>,
    execution_state: Option<Arc<ExecutionState>>,
) -> JoinHandle<()> {
    let span = tracing::info_span!(
        "teammate_stream",
        teammate = %teammate_name,
        team = %team_name,
    );

    tokio::spawn(async move {
        tracing::info!(
            teammate = %teammate_name,
            team = %team_name,
            "Starting teammate stdout stream processor"
        );

        // --- Create per-teammate conversation and pre-create assistant message ---
        // Uses the same pattern as the lead's streaming pipeline.
        let teammate_conversation_id: Option<ChatConversationId> =
            if let Some(ref conv_repo) = chat_conversation_repo {
                // Build a unique context_id for this teammate's conversation
                let teammate_ctx_id = format!("teammate:{}:{}", team_name, teammate_name);
                let conv = ChatConversation {
                    id: ChatConversationId::new(),
                    context_type: context_type.parse::<ChatContextType>().unwrap_or(ChatContextType::Ideation),
                    context_id: teammate_ctx_id,
                    claude_session_id: None,
                    provider_session_id: None,
                    provider_harness: None,
                    upstream_provider: None,
                    provider_profile: None,
                    agent_mode: None,
                    title: Some(format!("Teammate: {}", teammate_name)),
                    message_count: 0,
                    last_message_at: None,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                    archived_at: None,
                    parent_conversation_id: None,
                    attribution_backfill_status: None,
                    attribution_backfill_source: None,
                    attribution_backfill_source_path: None,
                    attribution_backfill_last_attempted_at: None,
                    attribution_backfill_completed_at: None,
                    attribution_backfill_error_summary: None,
                };
                let conv_id = conv.id;
                match conv_repo.create(conv).await {
                    Ok(_created) => {
                        tracing::info!(
                            teammate = %teammate_name,
                            conversation_id = %conv_id,
                            "Created per-teammate conversation"
                        );
                        // Store conversation_id in teammate state
                        let _ = team_tracker
                            .set_teammate_conversation_id(
                                &team_name,
                                &teammate_name,
                                conv_id.as_str(),
                            )
                            .await;
                        // Re-emit team:teammate_spawned with conversation_id now known
                        if let Ok(team_status) = team_tracker.get_team_status(&team_name).await {
                            if let Some(tm) = team_status.teammates.iter().find(|t| t.name == teammate_name) {
                                let conv_id_string = conv_id.as_str();
                                team_events::emit_teammate_spawned(
                                    &app_handle,
                                    &team_name,
                                    &teammate_name,
                                    &tm.color,
                                    &tm.model,
                                    &tm.role,
                                    &context_type,
                                    &context_id,
                                    Some(&conv_id_string),
                                );
                                tracing::info!(
                                    teammate = %teammate_name,
                                    conversation_id = %conv_id_string,
                                    "Re-emitted teammate_spawned with conversation_id"
                                );
                            }
                        }
                        Some(conv_id)
                    }
                    Err(e) => {
                        tracing::error!(
                            teammate = %teammate_name,
                            error = %e,
                            "Failed to create teammate conversation"
                        );
                        None
                    }
                }
            } else {
                None
            };

        let conversation_id_str = teammate_conversation_id
            .as_ref()
            .map(|id| id.as_str());

        // Pre-create empty assistant message (crash recovery pattern)
        let mut assistant_message_id: Option<String> =
            if let (Some(ref msg_repo), Some(ref conv_id)) =
                (&chat_message_repo, &teammate_conversation_id)
            {
                let msg = ChatMessage {
                    id: ChatMessageId::new(),
                    session_id: None,
                    project_id: None,
                    task_id: None,
                    conversation_id: Some(*conv_id),
                    role: MessageRole::Orchestrator,
                    content: String::new(),
                    metadata: None,
                    parent_message_id: None,
                    tool_calls: None,
                    content_blocks: None,
                    attribution_source: None,
                    provider_harness: None,
                    provider_session_id: None,
                    upstream_provider: None,
                    provider_profile: None,
                    logical_model: None,
                    effective_model_id: None,
                    logical_effort: None,
                    effective_effort: None,
                    input_tokens: None,
                    output_tokens: None,
                    cache_creation_tokens: None,
                    cache_read_tokens: None,
                    estimated_usd: None,
                    created_at: chrono::Utc::now(),
                };
                let msg_id = msg.id.as_str().to_string();
                match msg_repo.create(msg).await {
                    Ok(_) => {
                        tracing::info!(
                            teammate = %teammate_name,
                            assistant_message_id = %msg_id,
                            "Pre-created assistant message for teammate"
                        );
                        Some(msg_id)
                    }
                    Err(e) => {
                        tracing::error!(
                            teammate = %teammate_name,
                            error = %e,
                            "Failed to pre-create assistant message"
                        );
                        None
                    }
                }
            } else {
                None
            };

        // Debounced flush for incremental persistence (every 2 seconds)
        let mut last_flush = std::time::Instant::now();
        const FLUSH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

        // Emit agent:run_started so the frontend knows this teammate is running
        let _ = app_handle.emit(
            "agent:run_started",
            serde_json::json!({
                "teammate_name": teammate_name,
                "context_type": context_type,
                "context_id": context_id,
                "conversation_id": conversation_id_str,
            }),
        );

        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        let mut processor = StreamProcessor::new();
        let mut lines_seen: usize = 0;
        let mut lines_parsed: usize = 0;

        let mut has_emitted_running = false;
        // Fix B: tracks TurnComplete → Idle state between turns
        let mut is_idle = false;

        // Track cumulative cost from result events
        let mut total_cost_usd: f64 = 0.0;
        let mut total_input_tokens: u64 = 0;
        let mut total_output_tokens: u64 = 0;

        // Pin exit_signal so it can be used repeatedly in select!
        tokio::pin!(exit_signal);

        loop {
            // Use select! so we break when Claude exits even if a grandchild process
            // (e.g., Node.js MCP server) holds the stdout pipe open — which would
            // otherwise block next_line() indefinitely.
            let line_result = tokio::select! {
                biased;
                _ = &mut exit_signal => {
                    tracing::info!(
                        teammate = %teammate_name,
                        team = %team_name,
                        "Claude process exited — stopping stream processor (pipe inheritance guard)"
                    );
                    break;
                }
                result = lines.next_line() => result,
            };
            match line_result {
                Ok(Some(line)) => {
                    lines_seen += 1;

                    // DIAGNOSTIC: Log every raw stdout line at INFO level
                    // to verify teammate output is reaching the stream processor
                    let line_preview: &str = truncate_str(&line, 200);
                    tracing::info!(
                        teammate = %teammate_name,
                        team = %team_name,
                        lines_seen,
                        line_len = line.len(),
                        line_preview = %line_preview,
                        "[TEAMMATE_STREAM] raw stdout line"
                    );

                    if let Some(parsed) = StreamProcessor::parse_line(&line) {
                        lines_parsed += 1;
                        let stream_events = processor.process_parsed_line(parsed);

                        // DIAGNOSTIC: Log parsed event count per line
                        if !stream_events.is_empty() {
                            let event_names: Vec<&str> = stream_events.iter().map(|e| match e {
                                StreamEvent::TextChunk(_) => "TextChunk",
                                StreamEvent::Thinking(_) => "Thinking",
                                StreamEvent::ToolCallStarted { .. } => "ToolCallStarted",
                                StreamEvent::ToolCallCompleted { .. } => "ToolCallCompleted",
                                StreamEvent::ToolResultReceived { .. } => "ToolResultReceived",
                                StreamEvent::SessionId(_) => "SessionId",
                                StreamEvent::TaskStarted { .. } => "TaskStarted",
                                StreamEvent::TaskCompleted { .. } => "TaskCompleted",
                                StreamEvent::TeamMessageSent { .. } => "TeamMessageSent",
                                StreamEvent::TeamCreated { .. } => "TeamCreated",
                                StreamEvent::TeammateSpawned { .. } => "TeammateSpawned",
                                StreamEvent::TeamDeleted { .. } => "TeamDeleted",
                                StreamEvent::TurnComplete { .. } => "TurnComplete",
                                StreamEvent::HookStarted { .. } => "HookStarted",
                                StreamEvent::HookCompleted { .. } => "HookCompleted",
                                StreamEvent::HookBlock { .. } => "HookBlock",
                            }).collect();
                            tracing::info!(
                                teammate = %teammate_name,
                                team = %team_name,
                                lines_seen,
                                event_count = stream_events.len(),
                                events = ?event_names,
                                "[TEAMMATE_STREAM] parsed events"
                            );
                        }

                        for event in stream_events {
                            // Fix B: Transition back to Running when activity resumes after TurnComplete idle
                            if is_idle
                                && matches!(
                                    event,
                                    StreamEvent::TextChunk(_)
                                        | StreamEvent::Thinking(_)
                                        | StreamEvent::ToolCallStarted { .. }
                                        | StreamEvent::ToolCallCompleted { .. }
                                        | StreamEvent::ToolResultReceived { .. }
                                )
                            {
                                is_idle = false;

                                // Pre-create new assistant message for this turn
                                if assistant_message_id.is_none() {
                                    if let (Some(ref msg_repo), Some(ref conv_id)) =
                                        (&chat_message_repo, &teammate_conversation_id)
                                    {
                                        let msg = ChatMessage {
                                            id: ChatMessageId::new(),
                                            session_id: None,
                                            project_id: None,
                                            task_id: None,
                                            conversation_id: Some(*conv_id),
                                            role: MessageRole::Orchestrator,
                                            content: String::new(),
                                            metadata: None,
                                            parent_message_id: None,
                                            tool_calls: None,
                                            content_blocks: None,
                                            attribution_source: None,
                                            provider_harness: None,
                                            provider_session_id: None,
                                            upstream_provider: None,
                                            provider_profile: None,
                                            logical_model: None,
                                            effective_model_id: None,
                                            logical_effort: None,
                                            effective_effort: None,
                                            input_tokens: None,
                                            output_tokens: None,
                                            cache_creation_tokens: None,
                                            cache_read_tokens: None,
                                            estimated_usd: None,
                                            created_at: chrono::Utc::now(),
                                        };
                                        let new_id = msg.id.as_str().to_string();
                                        if msg_repo.create(msg).await.is_ok() {
                                            assistant_message_id = Some(new_id);
                                        }
                                    }
                                }

                                let _ = team_tracker
                                    .update_teammate_status(
                                        &team_name,
                                        &teammate_name,
                                        TeammateStatus::Running,
                                    )
                                    .await;
                                let _ = app_handle.emit(
                                    "agent:run_started",
                                    serde_json::json!({
                                        "teammate_name": teammate_name,
                                        "context_type": context_type,
                                        "context_id": context_id,
                                        "conversation_id": conversation_id_str,
                                    }),
                                );
                            }

                            match event {
                                StreamEvent::TextChunk(text) => {
                                    // Emit "running" status on first text output
                                    if !has_emitted_running {
                                        has_emitted_running = true;
                                        tracing::info!(
                                            teammate = %teammate_name,
                                            team = %team_name,
                                            context_type = %context_type,
                                            context_id = %context_id,
                                            "[TEAMMATE_STREAM] first text chunk — emitting Running status"
                                        );
                                        let _ = team_tracker
                                            .update_teammate_status(
                                                &team_name,
                                                &teammate_name,
                                                TeammateStatus::Running,
                                            )
                                            .await;
                                        team_events::emit_teammate_status_change(
                                            &app_handle,
                                            &team_name,
                                            &teammate_name,
                                            TeammateStatus::Running,
                                            &context_type,
                                            &context_id,
                                        );
                                    }

                                    let text_preview: &str = truncate_str(&text, 100);
                                    tracing::info!(
                                        teammate = %teammate_name,
                                        team = %team_name,
                                        text_len = text.len(),
                                        text_preview = %text_preview,
                                        context_type = %context_type,
                                        context_id = %context_id,
                                        "[TEAMMATE_STREAM] emitting agent:chunk"
                                    );

                                    let emit_result = app_handle.emit(
                                        "agent:chunk",
                                        serde_json::json!({
                                            "teammate_name": teammate_name,
                                            "text": text,
                                            "context_type": context_type,
                                            "context_id": context_id,
                                            "conversation_id": conversation_id_str,
                                            "append_to_previous": true,
                                        }),
                                    );
                                    if let Err(ref e) = emit_result {
                                        tracing::error!(
                                            teammate = %teammate_name,
                                            error = %e,
                                            "[TEAMMATE_STREAM] agent:chunk emit FAILED"
                                        );
                                    }

                                    // Debounced flush: persist content_blocks to DB every 2s
                                    if assistant_message_id.is_some()
                                        && last_flush.elapsed() >= FLUSH_INTERVAL
                                    {
                                        flush_teammate_message(
                                            &chat_message_repo,
                                            &assistant_message_id,
                                            &processor.response_text,
                                            &processor.tool_calls,
                                            &processor.content_blocks,
                                        )
                                        .await;
                                        last_flush = std::time::Instant::now();
                                    }
                                }
                                StreamEvent::Thinking(text) => {
                                    // Emit thinking as a chunk with a marker so frontend
                                    // can distinguish if needed
                                    let _ = app_handle.emit(
                                        "agent:chunk",
                                        serde_json::json!({
                                            "teammate_name": teammate_name,
                                            "text": text,
                                            "context_type": context_type,
                                            "context_id": context_id,
                                            "conversation_id": conversation_id_str,
                                            "append_to_previous": true,
                                        }),
                                    );
                                }
                                StreamEvent::ToolCallStarted {
                                    name,
                                    id,
                                    parent_tool_use_id,
                                } => {
                                    let _ = app_handle.emit(
                                        "agent:tool_call",
                                        serde_json::json!({
                                            "teammate_name": teammate_name,
                                            "tool_name": name,
                                            "tool_id": id,
                                            "arguments": serde_json::Value::Null,
                                            "context_type": context_type,
                                            "context_id": context_id,
                                            "parent_tool_use_id": parent_tool_use_id,
                                            "conversation_id": conversation_id_str,
                                        }),
                                    );
                                }
                                StreamEvent::ToolCallCompleted {
                                    tool_call,
                                    parent_tool_use_id,
                                } => {
                                    let _ = app_handle.emit(
                                        "agent:tool_call",
                                        serde_json::json!({
                                            "teammate_name": teammate_name,
                                            "tool_name": tool_call.name,
                                            "tool_id": tool_call.id,
                                            "arguments": tool_call.arguments,
                                            "context_type": context_type,
                                            "context_id": context_id,
                                            "parent_tool_use_id": parent_tool_use_id,
                                            "conversation_id": conversation_id_str,
                                        }),
                                    );

                                    // Debounced flush after tool call completion
                                    if assistant_message_id.is_some()
                                        && last_flush.elapsed() >= FLUSH_INTERVAL
                                    {
                                        flush_teammate_message(
                                            &chat_message_repo,
                                            &assistant_message_id,
                                            &processor.response_text,
                                            &processor.tool_calls,
                                            &processor.content_blocks,
                                        )
                                        .await;
                                        last_flush = std::time::Instant::now();
                                    }
                                }
                                StreamEvent::ToolResultReceived {
                                    tool_use_id,
                                    result,
                                    parent_tool_use_id,
                                } => {
                                    let original_tool_name = processor
                                        .tool_calls
                                        .iter()
                                        .find(|tool_call| {
                                            tool_call.id.as_deref() == Some(tool_use_id.as_str())
                                        })
                                        .map(|tool_call| tool_call.name.as_str());
                                    let detail_ref =
                                        assistant_message_id.as_ref().and_then(|message_id| {
                                            conversation_id_str.as_deref().map(|conversation_id| {
                                                tool_detail_ref(
                                                    conversation_id,
                                                    message_id,
                                                    Some(tool_use_id.as_str()),
                                                    None,
                                                )
                                            })
                                        });
                                    let result_preview = build_live_tool_result_preview(
                                        original_tool_name,
                                        &result,
                                        detail_ref,
                                    );
                                    if result_preview.is_previewed() {
                                        flush_teammate_message(
                                            &chat_message_repo,
                                            &assistant_message_id,
                                            &processor.response_text,
                                            &processor.tool_calls,
                                            &processor.content_blocks,
                                        )
                                        .await;
                                    }

                                    let _ = app_handle.emit(
                                        "agent:tool_call",
                                        serde_json::json!({
                                            "teammate_name": teammate_name,
                                            "tool_name": format!("result:{}", tool_use_id),
                                            "tool_id": tool_use_id,
                                            "arguments": serde_json::Value::Null,
                                            "result": result_preview.result,
                                            "result_preview_truncated": result_preview.preview.as_ref().map(|_| true),
                                            "result_preview_original_bytes": result_preview.preview.as_ref().map(|preview| preview.original_bytes),
                                            "result_preview_line_count": result_preview.preview.as_ref().map(|preview| preview.line_count),
                                            "result_preview_omitted_lines": result_preview.preview.as_ref().map(|preview| preview.omitted_lines),
                                            "detail_ref": result_preview.preview.as_ref().and_then(|preview| preview.detail_ref.clone()),
                                            "context_type": context_type,
                                            "context_id": context_id,
                                            "parent_tool_use_id": parent_tool_use_id,
                                            "conversation_id": conversation_id_str,
                                        }),
                                    );
                                }
                                StreamEvent::SessionId(_) => {
                                    // Session ID captured in processor — not needed for
                                    // teammate streaming (teammates don't use --resume)
                                }
                                StreamEvent::TaskStarted {
                                    tool_use_id,
                                    tool_name,
                                    description,
                                    subagent_type,
                                    model,
                                    teammate_name: _,
                                    team_name: _,
                                } => {
                                    let _ = app_handle.emit(
                                        "agent:task_started",
                                        serde_json::json!({
                                            "teammate_name": teammate_name,
                                            "tool_use_id": tool_use_id,
                                            "tool_name": tool_name,
                                            "description": description,
                                            "subagent_type": subagent_type,
                                            "model": model,
                                            "context_type": context_type,
                                            "context_id": context_id,
                                        }),
                                    );
                                }
                                StreamEvent::TaskCompleted {
                                    tool_use_id,
                                    agent_id,
                                    total_duration_ms,
                                    total_tokens,
                                    total_tool_use_count,
                                } => {
                                    let _ = app_handle.emit(
                                        "agent:task_completed",
                                        serde_json::json!({
                                            "teammate_name": teammate_name,
                                            "tool_use_id": tool_use_id,
                                            "agent_id": agent_id,
                                            "total_duration_ms": total_duration_ms,
                                            "total_tokens": total_tokens,
                                            "total_tool_use_count": total_tool_use_count,
                                            "context_type": context_type,
                                            "context_id": context_id,
                                        }),
                                    );
                                }
                                StreamEvent::TeamMessageSent {
                                    sender,
                                    recipient,
                                    content,
                                    message_type,
                                } => {
                                    // Persist message and emit proper team:message event
                                    let msg_type = match message_type.as_str() {
                                        "broadcast" => TeamMessageType::Broadcast,
                                        _ => TeamMessageType::TeammateMessage,
                                    };

                                    if let Some(ref service) = team_service {
                                        // Use TeamService for full persistence + event emission
                                        match service
                                            .add_teammate_message(
                                                &team_name,
                                                &sender,
                                                recipient.as_deref(),
                                                &content,
                                                msg_type,
                                            )
                                            .await
                                        {
                                            Ok(_) => {
                                                tracing::info!(
                                                    teammate = %teammate_name,
                                                    sender = %sender,
                                                    recipient = ?recipient,
                                                    "Teammate message persisted and emitted"
                                                );
                                            }
                                            Err(e) => {
                                                tracing::error!(
                                                    error = %e,
                                                    sender = %sender,
                                                    "Failed to persist teammate message"
                                                );
                                            }
                                        }
                                    } else {
                                        // Fallback: emit event directly without persistence
                                        let _ = app_handle.emit(
                                            "team:message",
                                            serde_json::json!({
                                                "team_name": team_name,
                                                "sender": sender,
                                                "recipient": recipient,
                                                "content": content,
                                                "message_type": message_type,
                                                "context_type": context_type,
                                                "context_id": context_id,
                                            }),
                                        );
                                    }

                                    // Auto-nudge lead's stdin when a teammate sends a message
                                    // targeting the lead (or broadcasts). This wakes up the
                                    // lead's Claude CLI so it sees the teammate's message.
                                    //
                                    // IMPORTANT: The lead's stdin is registered under the LEAD's
                                    // context (from chat_service), not the teammate's. We must
                                    // look up the lead's context from the team tracker.
                                    if let Some(ref registry) = interactive_process_registry {
                                        match team_tracker.get_team_status(&team_name).await {
                                            Ok(team_status) => {
                                                let key = InteractiveProcessKey::new(
                                                    &team_status.context_type,
                                                    &team_status.context_id,
                                                );
                                                tracing::info!(
                                                    teammate = %teammate_name,
                                                    sender = %sender,
                                                    lead_context_type = %team_status.context_type,
                                                    lead_context_id = %team_status.context_id,
                                                    "[TEAM_NUDGE] Attempting lead stdin nudge"
                                                );
                                                let nudge_text = format!(
                                                    "[Team message from {}]: {}",
                                                    sender,
                                                    truncate_str(&content, 500),
                                                );
                                                let nudge = format_stream_json_input(&nudge_text);
                                                if let Err(e) = registry.write_message(&key, &nudge).await
                                                {
                                                    tracing::warn!(
                                                        error = %e,
                                                        sender = %sender,
                                                        lead_context_type = %team_status.context_type,
                                                        lead_context_id = %team_status.context_id,
                                                        "[TEAM_NUDGE] Failed to write to lead stdin"
                                                    );
                                                } else {
                                                    tracing::info!(
                                                        sender = %sender,
                                                        "[TEAM_NUDGE] Successfully nudged lead stdin"
                                                    );
                                                    // Update execution state: the lead is now
                                                    // processing a message. Mirror the pattern
                                                    // from chat_service/mod.rs send_message
                                                    // fast-path to prevent running_count=0 while
                                                    // the lead responds to a team nudge.
                                                    if let Some(ref exec) = execution_state {
                                                        let slot_key = format!(
                                                            "{}/{}",
                                                            team_status.context_type,
                                                            team_status.context_id
                                                        );
                                                        if exec.claim_interactive_slot(&slot_key) {
                                                            exec.increment_running();
                                                            exec.emit_status_changed(
                                                                &app_handle,
                                                                "team_nudge_resumed",
                                                            );
                                                            tracing::info!(
                                                                sender = %sender,
                                                                slot_key = %slot_key,
                                                                running_count = exec.running_count(),
                                                                "[TEAM_NUDGE] Claimed idle slot, incremented running_count"
                                                            );
                                                        } else {
                                                            tracing::debug!(
                                                                sender = %sender,
                                                                slot_key = %slot_key,
                                                                "[TEAM_NUDGE] Lead already active (slot not idle), skipping increment"
                                                            );
                                                        }
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                tracing::warn!(
                                                    error = %e,
                                                    team = %team_name,
                                                    sender = %sender,
                                                    "[TEAM_NUDGE] Could not resolve lead context from team tracker"
                                                );
                                            }
                                        }
                                    } else {
                                        tracing::warn!(
                                            sender = %sender,
                                            "[TEAM_NUDGE] No InteractiveProcessRegistry available"
                                        );
                                    }
                                }
                                StreamEvent::TurnComplete { .. } => {
                                    // Finalize assistant message with accumulated content
                                    if let (Some(ref msg_repo), Some(ref msg_id)) =
                                        (&chat_message_repo, &assistant_message_id)
                                    {
                                        let tool_calls_json =
                                            serde_json::to_string(&processor.tool_calls).ok();
                                        let content_blocks_json =
                                            serde_json::to_string(&processor.content_blocks).ok();
                                        let _ = msg_repo
                                            .update_content(
                                                &ChatMessageId::from_string(msg_id.clone()),
                                                &processor.response_text,
                                                tool_calls_json.as_deref(),
                                                content_blocks_json.as_deref(),
                                            )
                                            .await;

                                        // Emit agent:message_created so frontend can load from DB
                                        if let Some(ref conv_id) = conversation_id_str {
                                            let _ = app_handle.emit(
                                                "agent:message_created",
                                                serde_json::json!({
                                                    "message_id": msg_id,
                                                    "conversation_id": conv_id,
                                                    "context_type": context_type,
                                                    "context_id": context_id,
                                                    "role": "orchestrator",
                                                    "content": processor.response_text,
                                                    "teammate_name": teammate_name,
                                                }),
                                            );
                                        }
                                    }

                                    // Reset processor for next turn (preserves session_id)
                                    processor.reset_for_next_turn();

                                    // Clear assistant message — new one will be pre-created
                                    // when next turn's content arrives
                                    assistant_message_id = None;

                                    // Reset running flag for next turn
                                    has_emitted_running = false;

                                    // Fix A: Teammate went idle between turns — update status and emit events
                                    is_idle = true;
                                    let _ = team_tracker
                                        .update_teammate_status(
                                            &team_name,
                                            &teammate_name,
                                            TeammateStatus::Idle,
                                        )
                                        .await;
                                    let _ = app_handle.emit(
                                        "agent:run_completed",
                                        serde_json::json!({
                                            "teammate_name": teammate_name,
                                            "context_type": context_type,
                                            "context_id": context_id,
                                            "conversation_id": conversation_id_str,
                                        }),
                                    );
                                    team_events::emit_teammate_idle(
                                        &app_handle,
                                        &team_name,
                                        &teammate_name,
                                        &context_type,
                                        &context_id,
                                    );
                                }
                                StreamEvent::HookStarted { .. }
                                | StreamEvent::HookCompleted { .. }
                                | StreamEvent::HookBlock { .. }
                                | StreamEvent::TeamCreated { .. }
                                | StreamEvent::TeammateSpawned { .. }
                                | StreamEvent::TeamDeleted { .. } => {
                                    // Hook and team events from teammates are not forwarded
                                    // (hooks run on the lead, team events only relevant from lead's stream)
                                }
                            }
                        }
                    } else {
                        // DIAGNOSTIC: Line was NOT parseable as a StreamMessage
                        tracing::info!(
                            teammate = %teammate_name,
                            team = %team_name,
                            lines_seen,
                            line_len = line.len(),
                            line_preview = %truncate_str(&line, 200),
                            "[TEAMMATE_STREAM] line NOT parsed (not a stream-json message)"
                        );
                    }

                    // Check for events with cost/usage info and persist text buffer.
                    //
                    // Token sources:
                    // - "type": "result" — final turn cost + usage (authoritative)
                    // - "type": "assistant" — per-message usage for real-time updates
                    //   (emitted mid-turn when model produces each response)
                    if let Ok(raw) = serde_json::from_str::<serde_json::Value>(&line) {
                        let event_type = raw.get("type").and_then(|t| t.as_str());

                        if event_type == Some("result") {
                            // Extract cost_usd from result event
                            if let Some(cost) = raw.get("cost_usd").and_then(|c| c.as_f64()) {
                                total_cost_usd += cost;
                            }

                            // Extract usage tokens if present
                            if let Some(usage) = raw.get("usage") {
                                if let Some(input) =
                                    usage.get("input_tokens").and_then(|t| t.as_u64())
                                {
                                    total_input_tokens += input;
                                }
                                if let Some(output) =
                                    usage.get("output_tokens").and_then(|t| t.as_u64())
                                {
                                    total_output_tokens += output;
                                }
                            }

                            // Update teammate cost via TeamService (which emits team:cost_update)
                            let cost = TeammateCost {
                                input_tokens: total_input_tokens,
                                output_tokens: total_output_tokens,
                                cache_creation_tokens: 0,
                                cache_read_tokens: 0,
                                estimated_usd: total_cost_usd,
                            };
                            let _ = team_tracker
                                .update_teammate_cost(&team_name, &teammate_name, cost)
                                .await;

                            // Emit cost update event
                            team_events::emit_team_cost_update(
                                &app_handle,
                                &team_name,
                                &teammate_name,
                                total_input_tokens,
                                total_output_tokens,
                                total_cost_usd,
                                &context_type,
                                &context_id,
                            );
                        } else if event_type == Some("assistant") {
                            // Real-time token updates from assistant message events.
                            // These carry a top-level `message.usage` with cumulative
                            // input/output tokens for the current turn, letting the UI
                            // show progress before the final "result" event arrives.
                            let updated = extract_assistant_usage(
                                &raw,
                                &mut total_input_tokens,
                                &mut total_output_tokens,
                            );
                            if updated {
                                let cost = TeammateCost {
                                    input_tokens: total_input_tokens,
                                    output_tokens: total_output_tokens,
                                    cache_creation_tokens: 0,
                                    cache_read_tokens: 0,
                                    estimated_usd: total_cost_usd,
                                };
                                let _ = team_tracker
                                    .update_teammate_cost(
                                        &team_name,
                                        &teammate_name,
                                        cost,
                                    )
                                    .await;

                                team_events::emit_team_cost_update(
                                    &app_handle,
                                    &team_name,
                                    &teammate_name,
                                    total_input_tokens,
                                    total_output_tokens,
                                    total_cost_usd,
                                    &context_type,
                                    &context_id,
                                );
                            }
                        }
                    }

                    // Periodic progress logging (every 50 lines at INFO level for diagnostics)
                    #[allow(unknown_lints, clippy::manual_is_multiple_of)]
                    if lines_seen > 0 && lines_seen % 50 == 0 {
                        tracing::info!(
                            teammate = %teammate_name,
                            team = %team_name,
                            lines_seen,
                            lines_parsed,
                            total_cost_usd,
                            "[TEAMMATE_STREAM] progress"
                        );
                    }
                }
                Ok(None) => {
                    // Finalize any remaining assistant message content
                    flush_teammate_message(
                        &chat_message_repo,
                        &assistant_message_id,
                        &processor.response_text,
                        &processor.tool_calls,
                        &processor.content_blocks,
                    )
                    .await;

                    // EOF — stdout closed, teammate process exited
                    tracing::info!(
                        teammate = %teammate_name,
                        team = %team_name,
                        lines_seen,
                        lines_parsed,
                        total_cost_usd,
                        total_input_tokens,
                        total_output_tokens,
                        "[TEAMMATE_STREAM] stdout closed (EOF) — final stats"
                    );
                    break;
                }
                Err(e) => {
                    tracing::warn!(
                        teammate = %teammate_name,
                        team = %team_name,
                        error = %e,
                        "Teammate stdout read error"
                    );
                    break;
                }
            }
        }

        // Emit agent:run_completed so the frontend knows this teammate finished
        let _ = app_handle.emit(
            "agent:run_completed",
            serde_json::json!({
                "teammate_name": teammate_name,
                "context_type": context_type,
                "context_id": context_id,
                "conversation_id": conversation_id_str,
            }),
        );

        // Update teammate status to Idle (graceful exit) or Shutdown
        let new_status = TeammateStatus::Idle;
        let _ = team_tracker
            .update_teammate_status(&team_name, &teammate_name, new_status)
            .await;

        // Emit the idle event
        team_events::emit_teammate_idle(
            &app_handle,
            &team_name,
            &teammate_name,
            &context_type,
            &context_id,
        );

        tracing::info!(
            teammate = %teammate_name,
            team = %team_name,
            total_cost_usd,
            total_input_tokens,
            total_output_tokens,
            "Teammate stream processor finished"
        );
    }.instrument(span))
}


/// Extract usage tokens from a `"type": "assistant"` event's `message.usage` field.
///
/// Claude Code emits assistant events with cumulative usage per-message:
/// ```json
/// {"type":"assistant","message":{"usage":{"input_tokens":1234,"output_tokens":567},...},...}
/// ```
///
/// Updates the running totals only if the new values exceed the current totals
/// (usage is cumulative within a turn, so later messages have higher counts).
///
/// Returns `true` if the totals were updated (i.e., the UI should be notified).
fn extract_assistant_usage(
    raw: &serde_json::Value,
    total_input: &mut u64,
    total_output: &mut u64,
) -> bool {
    let usage = raw
        .get("message")
        .and_then(|m| m.get("usage"));
    let Some(usage) = usage else {
        return false;
    };

    let input = usage.get("input_tokens").and_then(|t| t.as_u64()).unwrap_or(0);
    let output = usage.get("output_tokens").and_then(|t| t.as_u64()).unwrap_or(0);

    // Only update if the new cumulative values exceed current totals.
    // Assistant usage is cumulative within a turn — later messages have higher counts.
    if input > *total_input || output > *total_output {
        if input > *total_input {
            *total_input = input;
        }
        if output > *total_output {
            *total_output = output;
        }
        true
    } else {
        false
    }
}

/// Flush accumulated content to the assistant message in the DB.
///
/// Mirrors the lead's `flush_content_before_error` pattern from `chat_service_streaming.rs`.
async fn flush_teammate_message(
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

#[cfg(test)]
#[path = "team_stream_processor_tests.rs"]
mod tests;
