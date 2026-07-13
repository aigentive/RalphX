use super::{
    agent_run_usage_from_codex_usage, capture_file_diff_baseline, codex_tool_call_content_block,
    completion_tool_result_accepted, flush_content_before_error, format_agent_exit_stderr,
    is_user_attended_turn_completion, normalize_codex_cumulative_usage_for_persistence,
    normalize_codex_stream_usage_for_persistence, persist_assistant_message_snapshot,
    persist_message_text_timeline_item, persist_timeline_snapshot, process_codex_stream_background,
    process_exit_details, process_stream_background, provider_session_ref_for_harness,
    resolve_codex_file_change_tool_call_snapshots, stream_mode_for_harness,
    upsert_codex_tool_call_snapshot, ProcessExitDetails, StreamOutcome, StreamingStateCache,
};
use crate::application::chat_service::chat_service_context::create_assistant_message;
use crate::application::chat_service::chat_service_errors::{ProviderErrorCategory, StreamError};
use crate::application::AppState;
use crate::domain::agents::{AgentHarnessKind, HarnessStreamMode};
use crate::domain::entities::{
    AgentRun, AgentRunUsage, ChatContextType, ChatConversationId, ChatMessage, ChatMessageId,
    ChatTimelineItem, ChatTimelineItemId, ChatTimelineItemStatus, ChatTimelinePage,
    IdeationSessionId, MessageRole, TaskId,
};
use crate::domain::repositories::{AgentRunRepository, ChatTimelineRepository};
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::{
    AssistantContent, AssistantMessage, ContentBlockItem, StreamMessage, StreamProcessor, ToolCall,
};
use crate::infrastructure::agents::{
    CodexFileChange, CodexFileChangeSnapshot, CodexToolCallPhase, CodexUsage, CodexUsageSource,
};
use crate::infrastructure::memory::MemoryAgentRunRepository;
use chrono::{Duration, Utc};
use std::os::unix::process::ExitStatusExt;
use std::process::Stdio;
use std::sync::Arc;
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

struct FailingTimelineRepository;

#[test]
fn completion_tool_result_accepts_success_payloads() {
    assert!(completion_tool_result_accepted(None));
    assert!(completion_tool_result_accepted(Some(
        &serde_json::json!({ "success": true })
    )));
    assert!(completion_tool_result_accepted(Some(
        &serde_json::json!({ "status": "ok" })
    )));
}

#[test]
fn completion_tool_result_rejects_error_payloads() {
    assert!(!completion_tool_result_accepted(Some(
        &serde_json::json!({ "is_error": true })
    )));
    assert!(!completion_tool_result_accepted(Some(
        &serde_json::json!({ "isError": true })
    )));
    assert!(!completion_tool_result_accepted(Some(
        &serde_json::json!({ "success": false })
    )));
    assert!(!completion_tool_result_accepted(Some(
        &serde_json::json!({ "status": "failed" })
    )));
}

#[test]
fn agent_waiting_suppresses_automation_run_conversations() {
    assert!(!is_user_attended_turn_completion(
        ChatContextType::Ideation,
        true,
        false,
    ));
}

#[test]
fn agent_waiting_suppresses_child_and_background_contexts() {
    assert!(!is_user_attended_turn_completion(
        ChatContextType::Ideation,
        false,
        true,
    ));
    assert!(!is_user_attended_turn_completion(
        ChatContextType::Delegation,
        false,
        false,
    ));
    assert!(!is_user_attended_turn_completion(
        ChatContextType::TaskExecution,
        false,
        false,
    ));
}

#[test]
fn agent_waiting_allows_user_attended_interactive_conversations() {
    assert!(is_user_attended_turn_completion(
        ChatContextType::Ideation,
        false,
        false,
    ));
    assert!(is_user_attended_turn_completion(
        ChatContextType::Project,
        false,
        false,
    ));
}

#[async_trait::async_trait]
impl ChatTimelineRepository for FailingTimelineRepository {
    async fn upsert_item(&self, _item: ChatTimelineItem) -> AppResult<ChatTimelineItem> {
        Err(AppError::Infrastructure(
            "timeline write failed".to_string(),
        ))
    }

    async fn get_by_id(&self, _id: &ChatTimelineItemId) -> AppResult<Option<ChatTimelineItem>> {
        Ok(None)
    }

    async fn get_page(
        &self,
        _conversation_id: &ChatConversationId,
        limit: u32,
        before_sequence: Option<i64>,
    ) -> AppResult<ChatTimelinePage> {
        Ok(ChatTimelinePage {
            items: Vec::new(),
            limit,
            before_sequence,
            total_item_count: 0,
            has_older: false,
            oldest_loaded_sequence: None,
            newest_loaded_sequence: None,
        })
    }

    async fn count_by_conversation(&self, _conversation_id: &ChatConversationId) -> AppResult<u32> {
        Ok(0)
    }

    async fn get_by_conversation(
        &self,
        _conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<ChatTimelineItem>> {
        Ok(Vec::new())
    }

    async fn delete_message_items_except_block_indices(
        &self,
        _message_id: &ChatMessageId,
        _retained_block_indices: Vec<i64>,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn mark_message_items_finalized(&self, _message_id: &ChatMessageId) -> AppResult<()> {
        Ok(())
    }
}

async fn spawn_jsonl_process(lines: &[&str]) -> tokio::process::Child {
    let mut payload = String::new();
    for line in lines {
        payload.push_str(line);
        payload.push('\n');
    }

    let mut child = Command::new("cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn codex jsonl fixture");

    let mut stdin = child.stdin.take().expect("capture fixture stdin");
    stdin
        .write_all(payload.as_bytes())
        .await
        .expect("write codex jsonl fixture");
    drop(stdin);

    child
}

async fn spawn_jsonl_process_with_exit_status(
    lines: &[&str],
    exit_status: i32,
) -> tokio::process::Child {
    let mut payload = String::new();
    for line in lines {
        payload.push_str(line);
        payload.push('\n');
    }

    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg("printf '%s' \"$RALPHX_STREAM_LINES\"; exit \"$RALPHX_EXIT_STATUS\"")
        .env("RALPHX_STREAM_LINES", payload)
        .env("RALPHX_EXIT_STATUS", exit_status.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    command
        .spawn()
        .expect("spawn codex jsonl fixture with exit status")
}

async fn spawn_interactive_jsonl_process_that_stays_alive(line: &str) -> tokio::process::Child {
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg("printf '%s\\n' \"$RALPHX_STREAM_LINE\"; sleep 10")
        .env("RALPHX_STREAM_LINE", line)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    command.spawn().expect("spawn interactive jsonl fixture")
}

async fn spawn_codex_jsonl_process_that_stays_alive(lines: &[&str]) -> tokio::process::Child {
    let mut payload = String::new();
    for line in lines {
        payload.push_str(line);
        payload.push('\n');
    }

    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg("printf '%s' \"$RALPHX_STREAM_LINES\"; exec sleep 10")
        .env("RALPHX_STREAM_LINES", payload)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    command.spawn().expect("spawn codex jsonl fixture")
}

async fn run_claude_stream_lines(lines: &[&str]) -> Result<StreamOutcome, StreamError> {
    let child = spawn_jsonl_process(lines).await;
    let conversation_id = ChatConversationId::new();
    let context_id = IdeationSessionId::new();
    let app = mock_builder()
        .build(mock_context(noop_assets()))
        .expect("mock app");
    let app_handle = app.handle().clone();

    process_stream_background::<MockRuntime>(
        child,
        AgentHarnessKind::Claude,
        ChatContextType::Ideation,
        context_id.as_str(),
        &conversation_id,
        Some(app_handle),
        None,
        None,
        None,
        None,
        None,
        None,
        CancellationToken::new(),
        None,
        false,
        StreamingStateCache::new(),
        None,
        None,
        Some("stream-run-id".to_string()),
        None,
        None,
        false,
        false,
    )
    .await
}

#[tokio::test]
async fn claude_stream_error_turn_complete_does_not_wait_for_interactive_timeout() {
    let child = spawn_interactive_jsonl_process_that_stays_alive(
        r#"{"type":"result","session_id":"sess-overloaded","is_error":true,"errors":["API Error: 529 Overloaded. This is a server-side issue, usually temporary - try again in a moment."],"result":"API Error: 529 Overloaded. This is a server-side issue, usually temporary - try again in a moment.","cost_usd":0.0}"#,
    )
    .await;
    let conversation_id = ChatConversationId::new();
    let context_id = IdeationSessionId::new();

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        process_stream_background::<MockRuntime>(
            child,
            AgentHarnessKind::Claude,
            ChatContextType::Ideation,
            context_id.as_str(),
            &conversation_id,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            CancellationToken::new(),
            None,
            false,
            StreamingStateCache::new(),
            None,
            None,
            Some("stream-run-id".to_string()),
            None,
            None,
            false,
            false,
        ),
    )
    .await
    .expect("error TurnComplete should not wait for the interactive line-read timeout");

    let error = result.expect_err("error result should fail the stream");
    assert!(
        matches!(
            error,
            StreamError::ProviderError {
                category: ProviderErrorCategory::Overloaded,
                ..
            }
        ),
        "expected overloaded provider error, got {error:?}"
    );
}

async fn run_codex_stream_lines(lines: &[&str]) -> Result<StreamOutcome, StreamError> {
    let child = spawn_jsonl_process(lines).await;
    let conversation_id = ChatConversationId::new();
    let context_id = IdeationSessionId::new();

    process_codex_stream_background::<MockRuntime>(
        child,
        ChatContextType::Ideation,
        context_id.as_str(),
        &conversation_id,
        None::<tauri::AppHandle<MockRuntime>>,
        None,
        None,
        None,
        None,
        None,
        None,
        CancellationToken::new(),
        StreamingStateCache::new(),
        None,
        None,
        None,
        None,
        None,
        false,
        false,
    )
    .await
}

#[tokio::test]
async fn codex_stream_turn_completed_finishes_without_waiting_for_process_exit() {
    let child = spawn_codex_jsonl_process_that_stays_alive(&[
        r#"{"type":"thread.started","thread_id":"codex-thread-queue"}"#,
        r#"{"type":"item.completed","item":{"type":"agent_message","text":"Done."}}"#,
        r#"{"type":"turn.completed","usage":{"last_token_usage":{"input_tokens":3,"output_tokens":2}}}"#,
    ])
    .await;
    let conversation_id = ChatConversationId::new();
    let context_id = IdeationSessionId::new();

    let outcome = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        process_codex_stream_background::<MockRuntime>(
            child,
            ChatContextType::Ideation,
            context_id.as_str(),
            &conversation_id,
            None::<tauri::AppHandle<MockRuntime>>,
            None,
            None,
            None,
            None,
            None,
            None,
            CancellationToken::new(),
            StreamingStateCache::new(),
            None,
            None,
            None,
            None,
            None,
            false,
            false,
        ),
    )
    .await
    .expect("Codex turn.completed should not wait for process EOF")
    .expect("Codex turn.completed should complete successfully");

    assert_eq!(outcome.response_text, "Done.");
    assert_eq!(outcome.session_id, Some("codex-thread-queue".to_string()));
    assert_eq!(outcome.turns_finalized, 0);
}

#[tokio::test]
async fn codex_stream_accepted_completion_tool_enters_grace_path() {
    let outcome = run_codex_stream_lines(&[
        r#"{"type":"item.completed","item":{"type":"mcp_tool_call","id":"tool-1","server":"ralphx","tool":"execution_complete","arguments":{"task_id":"task-1"},"result":{"success":true}}}"#,
        r#"{"type":"turn.completed","usage":{"last_token_usage":{"input_tokens":3,"output_tokens":2}}}"#,
    ])
    .await
    .expect("accepted completion tool should not fail the stream");

    assert!(outcome.completion_tool_called);
    assert_eq!(outcome.tool_calls.len(), 1);
    assert_eq!(outcome.tool_calls[0].name, "ralphx::execution_complete");
}

#[tokio::test]
async fn codex_stream_started_completion_then_rejected_has_no_completion_authority() {
    let result = run_codex_stream_lines(&[
        r#"{"type":"item.started","item":{"type":"mcp_tool_call","id":"tool-1","server":"ralphx","tool":"execution_complete","arguments":{"task_id":"task-1"}}}"#,
        r#"{"type":"item.completed","item":{"type":"mcp_tool_call","id":"tool-1","server":"ralphx","tool":"execution_complete","error":{"message":"ERROR: validation_failed\n\nDetails: Validation failed: 1 failed, 9 passed"}}}"#,
    ])
    .await
    .expect_err("a rejected completion tool must not inherit authority from item.started");

    match result {
        StreamError::ValidationFailed { message } => {
            assert!(message.contains("validation_failed"));
            assert!(message.contains("1 failed, 9 passed"));
        }
        other => panic!("expected validation failure, got {other:?}"),
    }
}

#[tokio::test]
async fn codex_stream_accepted_completion_suppresses_late_local_diagnostic() {
    let outcome = run_codex_stream_lines(&[
        r#"{"type":"item.completed","item":{"type":"mcp_tool_call","id":"tool-1","server":"ralphx","tool":"execution_complete","arguments":{"task_id":"task-1"},"result":{"success":true}}}"#,
        r#"{"type":"item.completed","item":{"type":"command_execution","id":"cmd-1","status":"failed","aggregated_output":"late cleanup diagnostic","exit_code":1}}"#,
    ])
    .await
    .expect("an accepted completion must outrank a later local diagnostic");

    assert!(outcome.completion_tool_called);
}

#[tokio::test]
async fn codex_empty_nonzero_terminal_exit_is_typed_as_no_output() {
    let child = spawn_jsonl_process_with_exit_status(
        &[r#"{"type":"thread.started","thread_id":"compacted-thread"}"#],
        1,
    )
    .await;
    let conversation_id = ChatConversationId::new();
    let context_id = IdeationSessionId::new();

    let result = process_codex_stream_background::<MockRuntime>(
        child,
        ChatContextType::Ideation,
        context_id.as_str(),
        &conversation_id,
        None::<tauri::AppHandle<MockRuntime>>,
        None,
        None,
        None,
        None,
        None,
        None,
        CancellationToken::new(),
        StreamingStateCache::new(),
        None,
        None,
        None,
        None,
        None,
        false,
        false,
    )
    .await;

    assert!(
        matches!(
            result,
            Err(StreamError::NoOutput {
                context_type: ChatContextType::Ideation
            })
        ),
        "a non-zero terminal exit without diagnostics must not be reduced to AgentExit"
    );
}

#[tokio::test]
async fn codex_empty_success_terminal_exit_is_typed_as_no_output() {
    let child = spawn_jsonl_process_with_exit_status(
        &[r#"{"type":"thread.started","thread_id":"empty-success-thread"}"#],
        0,
    )
    .await;
    let conversation_id = ChatConversationId::new();
    let context_id = IdeationSessionId::new();

    let result = process_codex_stream_background::<MockRuntime>(
        child,
        ChatContextType::Ideation,
        context_id.as_str(),
        &conversation_id,
        None::<tauri::AppHandle<MockRuntime>>,
        None,
        None,
        None,
        None,
        None,
        None,
        CancellationToken::new(),
        StreamingStateCache::new(),
        None,
        None,
        None,
        None,
        None,
        false,
        false,
    )
    .await;

    assert!(
        matches!(result, Err(StreamError::NoOutput { context_type: ChatContextType::Ideation })),
        "a terminal success without text, tool output, or completion signal must not settle as success"
    );
}

#[tokio::test]
async fn codex_event_msg_agent_messages_persist_to_task_execution_transcript() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    let task_id = TaskId::new();

    let pre_assistant = create_assistant_message(
        ChatContextType::TaskExecution,
        task_id.as_str(),
        "",
        conversation_id.clone(),
        &[],
        &[],
    );
    let pre_assistant_id = pre_assistant.id.as_str().to_string();
    state
        .chat_message_repo
        .create(pre_assistant)
        .await
        .expect("seed task execution assistant message");

    let child = spawn_jsonl_process(&[
        r#"{"type":"thread.started","thread_id":"codex-thread-event-msg"}"#,
        r#"{"type":"event_msg","msg":{"type":"agent_message","phase":"commentary","message":"Still working through validation."}}"#,
        r#"{"type":"event_msg","msg":{"type":"agent_message","phase":"final_answer","message":"Done from the final answer."}}"#,
        r#"{"type":"turn.completed","usage":{"last_token_usage":{"input_tokens":3,"output_tokens":2}}}"#,
    ])
    .await;

    let outcome = process_codex_stream_background::<MockRuntime>(
        child,
        ChatContextType::TaskExecution,
        task_id.as_str(),
        &conversation_id,
        None::<tauri::AppHandle<MockRuntime>>,
        None,
        None,
        Some(state.chat_message_repo.clone()),
        Some(state.chat_timeline_repo.clone()),
        Some(pre_assistant_id.clone()),
        None,
        CancellationToken::new(),
        StreamingStateCache::new(),
        None,
        None,
        None,
        None,
        None,
        false,
        false,
    )
    .await
    .expect("Codex event_msg stream should complete");

    assert!(
        outcome
            .response_text
            .contains("Still working through validation."),
        "event_msg commentary text should be part of the stream outcome"
    );
    assert!(
        outcome
            .response_text
            .contains("Done from the final answer."),
        "event_msg final answer text should be part of the stream outcome"
    );

    let persisted = state
        .chat_message_repo
        .get_by_id(&ChatMessageId::from_string(pre_assistant_id.clone()))
        .await
        .expect("load assistant message")
        .expect("assistant message should still exist");
    assert!(
        persisted
            .content
            .contains("Still working through validation."),
        "event_msg commentary text should be persisted into chat_messages"
    );
    assert!(
        persisted.content.contains("Done from the final answer."),
        "event_msg final answer text should be persisted into chat_messages"
    );

    let page = state
        .chat_timeline_repo
        .get_page(&conversation_id, 20, None)
        .await
        .expect("load timeline page");
    let text_concat = page
        .items
        .iter()
        .filter(|item| {
            item.message_id
                .as_ref()
                .is_some_and(|id| id.as_str() == pre_assistant_id)
        })
        .filter_map(|item| item.text.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        text_concat.contains("Done from the final answer."),
        "event_msg assistant text should also be persisted to timeline blocks"
    );
}

#[tokio::test]
async fn claude_stream_turn_complete_persists_assistant_blocks_to_timeline() {
    // Regression: when a project/task chat Claude turn ends via TurnComplete (result event),
    // the assistant content must land in BOTH chat_messages and chat_message_blocks.
    // Previously the TurnComplete handler called update_content on chat_messages but skipped
    // persist_timeline_snapshot, so the timeline-backed chat UI rendered the turn as
    // unanswered even though chat_messages had the response.
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    let context_id = IdeationSessionId::new();

    // Pre-create the assistant placeholder, matching the production spawn flow.
    let pre_assistant = create_assistant_message(
        ChatContextType::Ideation,
        context_id.as_str(),
        "",
        conversation_id.clone(),
        &[],
        &[],
    );
    let pre_assistant_id = pre_assistant.id.as_str().to_string();
    state
        .chat_message_repo
        .create(pre_assistant)
        .await
        .expect("seed pre-assistant message");

    let app = mock_builder()
        .build(mock_context(noop_assets()))
        .expect("mock app");
    let app_handle = app.handle().clone();

    let child = spawn_jsonl_process(&[
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"It is a Tauri desktop app called RalphX."}]},"session_id":"sess-1"}"#,
        r#"{"type":"result","session_id":"sess-1","is_error":false,"result":"It is a Tauri desktop app called RalphX.","cost_usd":0.0}"#,
    ])
    .await;

    process_stream_background::<MockRuntime>(
        child,
        AgentHarnessKind::Claude,
        ChatContextType::Ideation,
        context_id.as_str(),
        &conversation_id,
        Some(app_handle),
        None,
        None,
        Some(state.chat_message_repo.clone()),
        Some(state.chat_timeline_repo.clone()),
        Some(pre_assistant_id.clone()),
        None,
        CancellationToken::new(),
        None,
        false,
        StreamingStateCache::new(),
        None,
        None,
        Some("stream-run-id".to_string()),
        None,
        None,
        false,
        false,
    )
    .await
    .expect("stream should complete");

    let page = state
        .chat_timeline_repo
        .get_page(&conversation_id, 20, None)
        .await
        .expect("load timeline page");
    let assistant_blocks: Vec<_> = page
        .items
        .iter()
        .filter(|item| {
            item.message_id
                .as_ref()
                .is_some_and(|id| id.as_str() == pre_assistant_id)
        })
        .collect();
    assert!(
        !assistant_blocks.is_empty(),
        "TurnComplete must persist assistant content blocks to the timeline so the chat UI \
         (which renders from chat_message_blocks) shows the response. Found 0 blocks for \
         pre_assistant_id={}",
        pre_assistant_id
    );
    assert!(
        assistant_blocks
            .iter()
            .all(|item| item.status == ChatTimelineItemStatus::Finalized),
        "TurnComplete-persisted blocks must be marked Finalized"
    );
    let text_concat = assistant_blocks
        .iter()
        .filter_map(|item| item.text.clone())
        .collect::<Vec<_>>()
        .join("");
    assert!(
        text_concat.contains("Tauri desktop app called RalphX"),
        "Persisted timeline text must carry the assistant response"
    );
}

#[tokio::test]
async fn persist_timeline_snapshot_writes_ordered_blocks_and_finalizes_them() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    let message_id = Some("assistant-message-timeline".to_string());
    let blocks = vec![
        ContentBlockItem::Text {
            text: String::new(),
        },
        ContentBlockItem::Text {
            text: "Working through the change".to_string(),
        },
        ContentBlockItem::ToolUse {
            id: Some("tool-1".to_string()),
            name: "bash".to_string(),
            arguments: serde_json::json!({ "command": "cargo test" }),
            result: Some(serde_json::json!("ok")),
            parent_tool_use_id: None,
            diff_context: Some(serde_json::json!({ "file_path": "src/lib.rs" })),
        },
        ContentBlockItem::ToolUse {
            id: None,
            name: "Read".to_string(),
            arguments: serde_json::json!("src/main.rs"),
            result: None,
            parent_tool_use_id: Some("tool-1".to_string()),
            diff_context: None,
        },
    ];

    let mut streaming_blocks = blocks.clone();
    streaming_blocks.push(ContentBlockItem::Text {
        text: "Obsolete streaming-only progress".to_string(),
    });

    let streaming_items = persist_timeline_snapshot(
        &Some(state.chat_timeline_repo.clone()),
        &conversation_id.as_str(),
        &message_id,
        &streaming_blocks,
        ChatTimelineItemStatus::Streaming,
    )
    .await;
    assert_eq!(streaming_items.len(), 4);
    assert_eq!(streaming_items[0].status, ChatTimelineItemStatus::Streaming);
    assert_eq!(streaming_items[1].tool_call_id.as_deref(), Some("tool-1"));

    let page = state
        .chat_timeline_repo
        .get_page(&conversation_id, 10, None)
        .await
        .expect("load timeline page");
    assert_eq!(page.items.len(), 4);
    assert_eq!(page.items[0].block_index, 1);
    assert_eq!(
        page.items[0].text.as_deref(),
        Some("Working through the change")
    );
    assert_eq!(page.items[1].tool_call_id.as_deref(), Some("tool-1"));
    assert_eq!(page.items[1].tool_name.as_deref(), Some("bash"));
    assert_eq!(page.items[1].tool_status.as_deref(), Some("completed"));
    assert_eq!(page.items[2].tool_call_id, None);
    assert_eq!(page.items[2].tool_name.as_deref(), Some("Read"));
    assert_eq!(page.items[2].tool_status.as_deref(), Some("pending"));
    assert_eq!(
        page.items[2].tool_input_preview.as_deref(),
        Some("src/main.rs")
    );
    assert!(page.items[2].tool_result_preview.is_none());

    let finalized_items = persist_timeline_snapshot(
        &Some(state.chat_timeline_repo.clone()),
        &conversation_id.as_str(),
        &message_id,
        &blocks,
        ChatTimelineItemStatus::Finalized,
    )
    .await;
    assert_eq!(finalized_items.len(), 3);
    assert!(finalized_items
        .iter()
        .all(|item| item.status == ChatTimelineItemStatus::Finalized));

    let finalized = state
        .chat_timeline_repo
        .get_page(&conversation_id, 10, None)
        .await
        .expect("load finalized timeline page");
    assert_eq!(finalized.items.len(), 3);
    assert!(!finalized
        .items
        .iter()
        .any(|item| item.text.as_deref() == Some("Obsolete streaming-only progress")));
    assert!(finalized
        .items
        .iter()
        .all(|item| item.status == ChatTimelineItemStatus::Finalized));
    assert!(finalized
        .items
        .iter()
        .all(|item| item.finalized_at.is_some()));
}

#[tokio::test]
async fn persist_timeline_snapshot_returns_empty_when_any_item_write_fails() {
    let conversation_id = ChatConversationId::new();
    let message_id = Some("assistant-message-write-fails".to_string());
    let repo: Arc<dyn ChatTimelineRepository> = Arc::new(FailingTimelineRepository);
    let blocks = vec![ContentBlockItem::Text {
        text: "will fail".to_string(),
    }];

    let persisted = persist_timeline_snapshot(
        &Some(repo),
        &conversation_id.as_str(),
        &message_id,
        &blocks,
        ChatTimelineItemStatus::Finalized,
    )
    .await;

    assert!(persisted.is_empty());
}

#[tokio::test]
async fn persist_message_text_timeline_item_skips_empty_and_hidden_messages() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();

    let mut empty = ChatMessage::user_in_session(IdeationSessionId::new(), "");
    empty.conversation_id = Some(conversation_id);
    persist_message_text_timeline_item(&Some(state.chat_timeline_repo.clone()), &empty).await;

    let mut recovery = ChatMessage::user_in_session(IdeationSessionId::new(), "recover");
    recovery.conversation_id = Some(conversation_id);
    recovery.metadata = Some(r#"{"recovery_context":true}"#.to_string());
    persist_message_text_timeline_item(&Some(state.chat_timeline_repo.clone()), &recovery).await;

    let mut hidden = ChatMessage::user_in_session(IdeationSessionId::new(), "internal");
    hidden.conversation_id = Some(conversation_id);
    hidden.metadata = Some(r#"{"hidden_from_ui":true}"#.to_string());
    persist_message_text_timeline_item(&Some(state.chat_timeline_repo.clone()), &hidden).await;

    let mut normal = ChatMessage::user_in_session(IdeationSessionId::new(), "hello");
    normal.conversation_id = Some(conversation_id);
    normal.provider_harness = Some(AgentHarnessKind::Codex);
    normal.provider_session_id = Some("thread-user".to_string());
    persist_message_text_timeline_item(&Some(state.chat_timeline_repo.clone()), &normal).await;

    let page = state
        .chat_timeline_repo
        .get_page(&conversation_id, 10, None)
        .await
        .expect("load timeline page");
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].role, MessageRole::User);
    assert_eq!(page.items[0].text.as_deref(), Some("hello"));
    assert_eq!(
        page.items[0].provider_harness,
        Some(AgentHarnessKind::Codex)
    );
    assert_eq!(
        page.items[0].provider_session_id.as_deref(),
        Some("thread-user")
    );
}

#[tokio::test]
async fn timeline_persistence_helpers_ignore_missing_repo_or_message_identity() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    let blocks = vec![ContentBlockItem::Text {
        text: "ignored".to_string(),
    }];

    let missing_repo_items = persist_timeline_snapshot(
        &None,
        &conversation_id.as_str(),
        &Some("assistant-message-missing-repo".to_string()),
        &blocks,
        ChatTimelineItemStatus::Streaming,
    )
    .await;
    let missing_message_items = persist_timeline_snapshot(
        &Some(state.chat_timeline_repo.clone()),
        &conversation_id.as_str(),
        &None,
        &blocks,
        ChatTimelineItemStatus::Streaming,
    )
    .await;
    assert!(missing_repo_items.is_empty());
    assert!(missing_message_items.is_empty());

    let mut no_conversation = ChatMessage::user_in_session(IdeationSessionId::new(), "ignored");
    no_conversation.conversation_id = None;
    persist_message_text_timeline_item(&Some(state.chat_timeline_repo.clone()), &no_conversation)
        .await;
    let mut no_repo = ChatMessage::user_in_session(IdeationSessionId::new(), "ignored");
    no_repo.conversation_id = Some(conversation_id);
    persist_message_text_timeline_item(&None, &no_repo).await;

    let page = state
        .chat_timeline_repo
        .get_page(&conversation_id, 10, None)
        .await
        .expect("load timeline page");
    assert!(page.items.is_empty());
}

#[test]
fn process_exit_details_reports_non_zero_code() {
    let status = ExitStatusExt::from_raw(1 << 8);
    let details = process_exit_details(&status);

    assert_eq!(
        details,
        ProcessExitDetails {
            exit_code: Some(1),
            exit_signal: None,
            success: false,
        }
    );
}

#[test]
fn format_agent_exit_stderr_prefers_stderr_content() {
    let details = ProcessExitDetails {
        exit_code: Some(1),
        exit_signal: None,
        success: false,
    };

    assert_eq!(
        format_agent_exit_stderr(details, "provider exploded"),
        "provider exploded"
    );
}

#[test]
fn format_agent_exit_stderr_uses_signal_name_when_available() {
    let details = ProcessExitDetails {
        exit_code: None,
        exit_signal: Some(9),
        success: false,
    };

    assert_eq!(
        format_agent_exit_stderr(details, ""),
        "Agent process exited with signal 9 (SIGKILL)"
    );
}

#[test]
fn stream_mode_for_harness_routes_known_harnesses() {
    assert_eq!(
        stream_mode_for_harness(AgentHarnessKind::Claude),
        HarnessStreamMode::ClaudeEvents
    );
    assert_eq!(
        stream_mode_for_harness(AgentHarnessKind::Codex),
        HarnessStreamMode::CodexJsonl
    );
}

#[test]
fn provider_session_ref_for_harness_keeps_harness_and_id() {
    let session_ref = provider_session_ref_for_harness(AgentHarnessKind::Codex, "thread-123");

    assert_eq!(session_ref.harness, AgentHarnessKind::Codex);
    assert_eq!(session_ref.provider_session_id, "thread-123");
}

#[test]
fn agent_run_usage_from_codex_usage_maps_cached_input_as_cache_read() {
    let usage = agent_run_usage_from_codex_usage(CodexUsage {
        input_tokens: Some(50),
        cached_input_tokens: Some(40),
        output_tokens: Some(10),
    });

    assert_eq!(usage.input_tokens, Some(50));
    assert_eq!(usage.cache_read_tokens, Some(40));
    assert_eq!(usage.output_tokens, Some(10));
    assert_eq!(usage.cache_creation_tokens, None);
    assert_eq!(usage.estimated_usd, None);
}

#[test]
fn normalize_codex_cumulative_usage_subtracts_per_turn_prior_runs() {
    let conversation_id = ChatConversationId::new();
    let prior_runs = vec![
        codex_usage_run(&conversation_id, "thread-1", 120, 30, 80, 0),
        codex_usage_run(&conversation_id, "thread-1", 200, 40, 150, 1),
    ];
    let current = AgentRunUsage {
        input_tokens: Some(500),
        output_tokens: Some(90),
        cache_creation_tokens: None,
        cache_read_tokens: Some(300),
        estimated_usd: None,
    };

    let normalized = normalize_codex_cumulative_usage_for_persistence(
        current,
        &prior_runs,
        None,
        Some("thread-1"),
    );

    assert_eq!(normalized.input_tokens, Some(180));
    assert_eq!(normalized.output_tokens, Some(20));
    assert_eq!(normalized.cache_read_tokens, Some(70));
}

#[test]
fn normalize_codex_cumulative_usage_uses_latest_prior_when_existing_rows_are_cumulative() {
    let conversation_id = ChatConversationId::new();
    let prior_runs = vec![
        codex_usage_run(
            &conversation_id,
            "thread-1",
            10_000_000,
            10_000,
            9_000_000,
            0,
        ),
        codex_usage_run(
            &conversation_id,
            "thread-1",
            30_000_000,
            40_000,
            29_000_000,
            1,
        ),
        codex_usage_run(
            &conversation_id,
            "thread-1",
            65_000_000,
            100_000,
            63_000_000,
            2,
        ),
    ];
    let current = AgentRunUsage {
        input_tokens: Some(67_362_753),
        output_tokens: Some(109_831),
        cache_creation_tokens: None,
        cache_read_tokens: Some(65_914_240),
        estimated_usd: None,
    };

    let normalized = normalize_codex_cumulative_usage_for_persistence(
        current,
        &prior_runs,
        None,
        Some("thread-1"),
    );

    assert_eq!(normalized.input_tokens, Some(2_362_753));
    assert_eq!(normalized.output_tokens, Some(9_831));
    assert_eq!(normalized.cache_read_tokens, Some(2_914_240));
}

#[test]
fn normalize_codex_cumulative_usage_filters_current_run_and_other_sessions() {
    let conversation_id = ChatConversationId::new();
    let mut prior_same_session = codex_usage_run(&conversation_id, "thread-1", 100, 30, 80, 0);
    prior_same_session.cache_creation_tokens = Some(7);
    prior_same_session.estimated_usd = Some(0.50);

    let mut prior_other_session = codex_usage_run(&conversation_id, "thread-2", 900, 900, 900, 1);
    prior_other_session.cache_creation_tokens = Some(900);
    prior_other_session.estimated_usd = Some(9.00);

    let mut current_run = codex_usage_run(&conversation_id, "thread-1", 300, 100, 200, 2);
    let current_run_id = current_run.id.as_str();
    current_run.cache_creation_tokens = Some(20);
    current_run.estimated_usd = Some(1.25);

    let normalized = normalize_codex_cumulative_usage_for_persistence(
        AgentRunUsage {
            input_tokens: Some(300),
            output_tokens: Some(100),
            cache_creation_tokens: Some(20),
            cache_read_tokens: Some(200),
            estimated_usd: Some(1.25),
        },
        &[prior_same_session, prior_other_session, current_run],
        Some(current_run_id.as_str()),
        Some("thread-1"),
    );

    assert_eq!(normalized.input_tokens, Some(200));
    assert_eq!(normalized.output_tokens, Some(70));
    assert_eq!(normalized.cache_creation_tokens, Some(13));
    assert_eq!(normalized.cache_read_tokens, Some(120));
    assert_eq!(normalized.estimated_usd, Some(0.75));
}

#[tokio::test]
async fn normalize_codex_stream_usage_keeps_turn_delta_without_repo_lookup() {
    let conversation_id = ChatConversationId::new();
    let raw = AgentRunUsage {
        input_tokens: Some(75),
        output_tokens: Some(15),
        cache_creation_tokens: None,
        cache_read_tokens: Some(60),
        estimated_usd: None,
    };

    let normalized = normalize_codex_stream_usage_for_persistence(
        raw.clone(),
        CodexUsageSource::TurnDelta,
        &None,
        &conversation_id,
        None,
        Some("thread-1"),
    )
    .await;

    assert_eq!(normalized, raw);
}

#[tokio::test]
async fn normalize_codex_stream_usage_uses_prior_repo_runs_for_cumulative_snapshots() {
    let conversation_id = ChatConversationId::new();
    let repo_impl = Arc::new(MemoryAgentRunRepository::new());
    let prior = codex_usage_run(&conversation_id, "thread-1", 120, 30, 80, 0);
    repo_impl.create(prior).await.expect("seed prior run");
    let other_session = codex_usage_run(&conversation_id, "thread-2", 900, 900, 900, 1);
    repo_impl
        .create(other_session)
        .await
        .expect("seed other-session run");
    let current_run = codex_usage_run(&conversation_id, "thread-1", 500, 90, 300, 2);
    let current_run_id = current_run.id.as_str();
    repo_impl
        .create(current_run)
        .await
        .expect("seed current run");

    let repo: Arc<dyn AgentRunRepository> = repo_impl;
    let normalized = normalize_codex_stream_usage_for_persistence(
        AgentRunUsage {
            input_tokens: Some(500),
            output_tokens: Some(90),
            cache_creation_tokens: None,
            cache_read_tokens: Some(300),
            estimated_usd: None,
        },
        CodexUsageSource::CumulativeTotal,
        &Some(repo),
        &conversation_id,
        Some(current_run_id.as_str()),
        Some("thread-1"),
    )
    .await;

    assert_eq!(normalized.input_tokens, Some(380));
    assert_eq!(normalized.output_tokens, Some(60));
    assert_eq!(normalized.cache_read_tokens, Some(220));
}

fn codex_usage_run(
    conversation_id: &ChatConversationId,
    provider_session_id: &str,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    started_offset_secs: i64,
) -> AgentRun {
    let mut run = AgentRun::new(*conversation_id);
    run.harness = Some(AgentHarnessKind::Codex);
    run.provider_session_id = Some(provider_session_id.to_string());
    run.input_tokens = Some(input_tokens);
    run.output_tokens = Some(output_tokens);
    run.cache_read_tokens = Some(cache_read_tokens);
    run.started_at = Utc::now() + Duration::seconds(started_offset_secs);
    run
}

#[tokio::test]
async fn claude_stream_assistant_text_with_rate_limit_is_not_provider_error() {
    let outcome = run_claude_stream_lines(&[
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"The local metadata file contains the literal rate_limit string."}]},"session_id":"sess-1"}"#,
    ])
    .await
    .expect("normal assistant text should stay successful");

    assert_eq!(
        outcome.response_text,
        "The local metadata file contains the literal rate_limit string."
    );
    assert!(outcome.tool_calls.is_empty());
}

#[tokio::test]
async fn claude_stream_success_result_completes_interactive_turn() {
    let outcome = run_claude_stream_lines(&[
        r#"{"type":"result","session_id":"sess-1","is_error":false,"result":"Done","cost_usd":0.0}"#,
    ])
    .await
    .expect("successful result should complete the turn");

    assert_eq!(outcome.session_id, Some("sess-1".to_string()));
}

#[tokio::test]
async fn claude_stream_accepted_completion_tool_enters_grace_path() {
    let outcome = run_claude_stream_lines(&[
        r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu-complete","name":"mcp__ralphx__execution_complete","input":{"task_id":"task-1"}}]},"session_id":"sess-1"}"#,
        r#"{"type":"user","message":{"role":"user","content":[{"tool_use_id":"toolu-complete","type":"tool_result","content":{"success":true},"is_error":false}]}}"#,
        r#"{"type":"result","session_id":"sess-1","is_error":false,"result":"Done","cost_usd":0.0}"#,
    ])
    .await
    .expect("accepted completion tool should not fail the stream");

    assert!(outcome.completion_tool_called);
}

#[tokio::test]
async fn claude_stream_accepted_completion_suppresses_late_agent_exit() {
    let outcome = run_claude_stream_lines(&[
        r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu-complete","name":"mcp__ralphx__execution_complete","input":{"task_id":"task-1"}}]},"session_id":"sess-1"}"#,
        r#"{"type":"user","message":{"role":"user","content":[{"tool_use_id":"toolu-complete","type":"tool_result","content":{"success":true},"is_error":false}]}}"#,
        r#"{"type":"result","session_id":"sess-1","is_error":true,"errors":["late process shutdown"],"cost_usd":0.0}"#,
    ])
    .await
    .expect("accepted completion must outrank a later agent-exit diagnostic");

    assert!(outcome.completion_tool_called);
}

#[tokio::test]
async fn claude_stream_rejected_completion_remains_failed() {
    let result = run_claude_stream_lines(&[
        r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu-complete","name":"mcp__ralphx__execution_complete","input":{"task_id":"task-1"}}]},"session_id":"sess-1"}"#,
        r#"{"type":"user","message":{"role":"user","content":[{"tool_use_id":"toolu-complete","type":"tool_result","content":{"success":false},"is_error":true}]}}"#,
        r#"{"type":"result","session_id":"sess-1","is_error":true,"errors":["validation_failed"],"cost_usd":0.0}"#,
    ])
    .await
    .expect_err("a rejected completion result must remain a failed run");

    assert!(matches!(result, StreamError::AgentExit { .. }));
}

#[tokio::test]
async fn claude_stream_runtime_rate_limit_result_still_classifies_as_provider_error() {
    let result = run_claude_stream_lines(&[
        r#"{"type":"result","session_id":"sess-1","is_error":true,"errors":["Error: rate_limit_exceeded"],"cost_usd":0.0}"#,
    ])
    .await
    .expect_err("runtime provider failure should classify");

    match result {
        StreamError::ProviderError { category, .. } => {
            assert_eq!(category, ProviderErrorCategory::RateLimit);
        }
        other => panic!("expected provider rate limit, got {other:?}"),
    }
}

#[tokio::test]
async fn claude_stream_usage_limit_assistant_banner_still_classifies_as_provider_error() {
    let result = run_claude_stream_lines(&[
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"You've hit your limit. Your limit will reset at 2026-05-09 18:00:00"}]},"session_id":"sess-1"}"#,
    ])
    .await
    .expect_err("Claude usage-limit banner should classify");

    match result {
        StreamError::ProviderError { category, .. } => {
            assert_eq!(category, ProviderErrorCategory::RateLimit);
        }
        other => panic!("expected provider rate limit, got {other:?}"),
    }
}

#[tokio::test]
async fn codex_stream_local_command_failures_are_local_tool_failure_not_provider_pause() {
    let result = run_codex_stream_lines(
        &[
            r#"{"type":"item.completed","item":{"type":"command_execution","id":"cmd-1","command":"rg rate_limit missing.rs","status":"failed","aggregated_output":"rg: missing.rs: No such file or directory\nlocal enum rate_limit","exit_code":2}}"#,
            r#"{"type":"item.completed","item":{"type":"command_execution","id":"cmd-2","status":"failed","exit_code":7}}"#,
        ],
    )
    .await
    .expect_err("local command failures should surface as a local tool error");

    match result {
        StreamError::LocalToolFailed { message } => {
            assert!(message.contains("No such file or directory"));
            assert!(message.contains("rate_limit"));
            assert!(message.contains("Codex command_execution failed with exit code 7"));
        }
        other => panic!("expected local command failures to remain LocalToolFailed, got {other:?}"),
    }
}

#[tokio::test]
async fn codex_stream_mcp_tool_failure_with_rate_limit_text_is_local_tool_failure() {
    let result = run_codex_stream_lines(
        &[r#"{"type":"item.completed","item":{"type":"mcp_tool_call","id":"tool-1","server":"ralphx","tool":"delegate_start","error":{"message":"delegate_start failed after reading local rate_limit metadata"}}}"#],
    )
    .await
    .expect_err("local MCP failure should surface as a local tool error");

    match result {
        StreamError::LocalToolFailed { message } => {
            assert!(message.contains("delegate_start failed"));
            assert!(message.contains("rate_limit"));
        }
        other => panic!("expected local MCP failure to remain LocalToolFailed, got {other:?}"),
    }
}

#[tokio::test]
async fn codex_stream_completion_rejection_with_validation_code_is_validation_failed() {
    let result = run_codex_stream_lines(
        &[r#"{"type":"item.completed","item":{"type":"mcp_tool_call","id":"tool-1","server":"ralphx","tool":"execution_complete","error":{"message":"ERROR: validation_failed\n\nDetails: Validation failed: 1 failed, 9 passed"}}}"#],
    )
    .await
    .expect_err("validation rejection should surface as validation failure");

    match result {
        StreamError::ValidationFailed { message } => {
            assert!(message.contains("validation_failed"));
            assert!(message.contains("1 failed, 9 passed"));
        }
        other => panic!("expected validation failure, got {other:?}"),
    }
}

#[tokio::test]
async fn codex_stream_completed_after_local_command_failure_keeps_diagnostic_non_terminal() {
    let outcome = run_codex_stream_lines(&[
        r#"{"type":"item.completed","item":{"type":"command_execution","id":"cmd-1","status":"failed","aggregated_output":"test failed while repairing","exit_code":1}}"#,
        r#"{"type":"item.completed","item":{"type":"agent_message","text":"Repaired the failure."}}"#,
        r#"{"type":"turn.completed"}"#,
    ])
    .await
    .expect("a completed Codex turn must not become AgentExit because an earlier command failed");

    assert_eq!(outcome.response_text, "Repaired the failure.");
}

#[tokio::test]
async fn codex_stream_runtime_rate_limit_error_is_provider_error() {
    let result = run_codex_stream_lines(
        &[r#"{"type":"item.completed","item":{"type":"error","id":"err-1","error":{"message":"Error: rate_limit_exceeded"}}}"#],
    )
    .await
    .expect_err("runtime provider failure should classify");

    match result {
        StreamError::ProviderError { category, .. } => {
            assert_eq!(category, ProviderErrorCategory::RateLimit);
        }
        other => panic!("expected provider rate limit, got {other:?}"),
    }
}

#[tokio::test]
async fn codex_stream_ignores_non_fatal_mcp_resource_probe_error() {
    let outcome = run_codex_stream_lines(
        &[r#"{"type":"item.completed","item":{"type":"mcp_tool_call","id":"tool-1","server":"ralphx","tool":"list_mcp_resources","error":{"message":"resources/list failed for 'ralphx': Mcp error: -32601: Method not found"}}}"#],
    )
    .await
    .expect("resource probe errors should not fail the stream");

    assert_eq!(outcome.response_text, "");
    assert_eq!(outcome.tool_calls.len(), 1);
    assert_eq!(outcome.tool_calls[0].name, "ralphx::list_mcp_resources");
}

#[test]
fn codex_tool_call_content_block_preserves_orderable_tool_payload() {
    let tool_call = ToolCall {
        id: Some("tool-1".to_string()),
        name: "ralphx::get_task_context".to_string(),
        arguments: serde_json::json!({ "task_id": "task-1" }),
        result: Some(serde_json::json!({ "title": "Task" })),
        parent_tool_use_id: Some("toolu-parent-1".to_string()),
        diff_context: Some(crate::infrastructure::agents::claude::DiffContext {
            old_content: Some("before".to_string()),
            old_file_exists: None,
            file_path: "/tmp/example.txt".to_string(),
        }),
        stats: None,
    };

    let block = codex_tool_call_content_block(&tool_call);

    match block {
        ContentBlockItem::ToolUse {
            id,
            name,
            arguments,
            result,
            parent_tool_use_id,
            diff_context,
        } => {
            assert_eq!(id.as_deref(), Some("tool-1"));
            assert_eq!(name, "ralphx::get_task_context");
            assert_eq!(arguments, serde_json::json!({ "task_id": "task-1" }));
            assert_eq!(result, Some(serde_json::json!({ "title": "Task" })));
            assert_eq!(parent_tool_use_id.as_deref(), Some("toolu-parent-1"));
            assert_eq!(
                diff_context,
                Some(serde_json::json!({
                    "old_content": "before",
                    "file_path": "/tmp/example.txt",
                }))
            );
        }
        other => panic!("expected tool_use block, got {other:?}"),
    }
}

#[test]
fn upsert_codex_tool_call_snapshot_updates_existing_tool_call_in_place() {
    let mut tool_calls = vec![ToolCall {
        id: Some("item_1".to_string()),
        name: "ralphx::get_session_plan".to_string(),
        arguments: serde_json::json!({ "session_id": "s1" }),
        result: None,
        parent_tool_use_id: Some("toolu-parent-1".to_string()),
        diff_context: None,
        stats: None,
    }];
    let mut content_blocks = vec![codex_tool_call_content_block(&tool_calls[0])];

    upsert_codex_tool_call_snapshot(
        &mut tool_calls,
        &mut content_blocks,
        ToolCall {
            id: Some("item_1".to_string()),
            name: "ralphx::get_session_plan".to_string(),
            arguments: serde_json::json!({ "session_id": "s1" }),
            result: Some(serde_json::json!({ "plan": null })),
            parent_tool_use_id: Some("toolu-parent-1".to_string()),
            diff_context: Some(crate::infrastructure::agents::claude::DiffContext {
                old_content: Some("before".to_string()),
                old_file_exists: None,
                file_path: "/tmp/example.txt".to_string(),
            }),
            stats: None,
        },
    );

    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0].id.as_deref(), Some("item_1"));
    assert_eq!(
        tool_calls[0].result,
        Some(serde_json::json!({ "plan": null }))
    );
    assert_eq!(
        tool_calls[0].parent_tool_use_id.as_deref(),
        Some("toolu-parent-1")
    );

    assert_eq!(content_blocks.len(), 1);
    match &content_blocks[0] {
        ContentBlockItem::ToolUse {
            id,
            result,
            diff_context,
            ..
        } => {
            assert_eq!(id.as_deref(), Some("item_1"));
            assert_eq!(result, &Some(serde_json::json!({ "plan": null })));
            assert_eq!(
                diff_context,
                &Some(serde_json::json!({
                    "old_content": "before",
                    "file_path": "/tmp/example.txt",
                }))
            );
        }
        other => panic!("expected tool_use block, got {other:?}"),
    }
}

#[test]
fn upsert_codex_tool_call_snapshot_appends_new_tool_ids_in_order() {
    let mut tool_calls = Vec::new();
    let mut content_blocks = Vec::new();

    upsert_codex_tool_call_snapshot(
        &mut tool_calls,
        &mut content_blocks,
        ToolCall {
            id: Some("item_1".to_string()),
            name: "ralphx::get_session_plan".to_string(),
            arguments: serde_json::json!({ "session_id": "s1" }),
            result: None,
            parent_tool_use_id: None,
            diff_context: None,
            stats: None,
        },
    );
    upsert_codex_tool_call_snapshot(
        &mut tool_calls,
        &mut content_blocks,
        ToolCall {
            id: Some("item_2".to_string()),
            name: "ralphx::list_session_proposals".to_string(),
            arguments: serde_json::json!({ "session_id": "s1" }),
            result: None,
            parent_tool_use_id: None,
            diff_context: None,
            stats: None,
        },
    );

    assert_eq!(tool_calls.len(), 2);
    assert_eq!(tool_calls[0].id.as_deref(), Some("item_1"));
    assert_eq!(tool_calls[1].id.as_deref(), Some("item_2"));
    assert_eq!(content_blocks.len(), 2);
}

#[test]
fn resolve_codex_file_change_tool_call_snapshots_turns_update_into_edit() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let file_path = temp_dir.path().join("existing.txt");
    std::fs::write(&file_path, "alpha\n").expect("seed existing file");

    let mut pending = std::collections::HashMap::new();
    let started = resolve_codex_file_change_tool_call_snapshots(
        CodexFileChangeSnapshot {
            id: Some("item_1".to_string()),
            phase: CodexToolCallPhase::Started,
            status: Some("in_progress".to_string()),
            changes: vec![CodexFileChange {
                path: file_path.display().to_string(),
                kind: "update".to_string(),
            }],
        },
        &mut pending,
    );

    assert_eq!(started.len(), 1);
    assert_eq!(started[0].tool_call.name, "file_change");
    assert_eq!(started[0].tool_call.id.as_deref(), Some("item_1:0"));

    std::fs::write(&file_path, "beta\n").expect("update file");

    let completed = resolve_codex_file_change_tool_call_snapshots(
        CodexFileChangeSnapshot {
            id: Some("item_1".to_string()),
            phase: CodexToolCallPhase::Completed,
            status: Some("completed".to_string()),
            changes: vec![CodexFileChange {
                path: file_path.display().to_string(),
                kind: "update".to_string(),
            }],
        },
        &mut pending,
    );

    assert_eq!(completed.len(), 1);
    let tool_call = &completed[0].tool_call;
    assert_eq!(tool_call.name, "edit");
    assert_eq!(tool_call.id.as_deref(), Some("item_1:0"));
    assert_eq!(
        tool_call.arguments,
        serde_json::json!({
            "file_path": file_path.display().to_string(),
            "old_string": "alpha\n",
            "new_string": "beta\n",
        })
    );
    assert_eq!(
        tool_call.result,
        Some(serde_json::json!({
            "status": "completed",
            "kind": "update",
        }))
    );
    assert_eq!(
        tool_call
            .diff_context
            .as_ref()
            .and_then(|ctx| ctx.old_content.as_deref()),
        Some("alpha\n")
    );
    assert_eq!(
        tool_call
            .diff_context
            .as_ref()
            .and_then(|ctx| ctx.old_file_exists),
        Some(true)
    );
}

#[test]
fn resolve_codex_file_change_tool_call_snapshots_turns_add_into_write() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let file_path = temp_dir.path().join("new.txt");

    let mut pending = std::collections::HashMap::new();
    let started = resolve_codex_file_change_tool_call_snapshots(
        CodexFileChangeSnapshot {
            id: Some("item_2".to_string()),
            phase: CodexToolCallPhase::Started,
            status: Some("in_progress".to_string()),
            changes: vec![CodexFileChange {
                path: file_path.display().to_string(),
                kind: "add".to_string(),
            }],
        },
        &mut pending,
    );

    assert_eq!(started.len(), 1);
    assert_eq!(started[0].tool_call.name, "file_change");
    assert_eq!(started[0].tool_call.id.as_deref(), Some("item_2:0"));

    std::fs::write(&file_path, "gamma\n").expect("create file");

    let completed = resolve_codex_file_change_tool_call_snapshots(
        CodexFileChangeSnapshot {
            id: Some("item_2".to_string()),
            phase: CodexToolCallPhase::Completed,
            status: Some("completed".to_string()),
            changes: vec![CodexFileChange {
                path: file_path.display().to_string(),
                kind: "add".to_string(),
            }],
        },
        &mut pending,
    );

    assert_eq!(completed.len(), 1);
    let tool_call = &completed[0].tool_call;
    assert_eq!(tool_call.name, "write");
    assert_eq!(tool_call.id.as_deref(), Some("item_2:0"));
    assert_eq!(
        tool_call.arguments,
        serde_json::json!({
            "file_path": file_path.display().to_string(),
            "content": "gamma\n",
        })
    );
    assert_eq!(
        tool_call.result,
        Some(serde_json::json!({
            "status": "completed",
            "kind": "add",
        }))
    );
    let expected_path = file_path.to_string_lossy().to_string();
    assert_eq!(
        tool_call
            .diff_context
            .as_ref()
            .map(|ctx| ctx.file_path.as_str()),
        Some(expected_path.as_str())
    );
    assert!(tool_call
        .diff_context
        .as_ref()
        .and_then(|ctx| ctx.old_content.as_deref())
        .is_none());
    assert_eq!(
        tool_call
            .diff_context
            .as_ref()
            .and_then(|ctx| ctx.old_file_exists),
        Some(false)
    );
}

#[test]
fn capture_file_diff_baseline_reports_existing_missing_and_unreadable_paths() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let existing_path = temp_dir.path().join("existing.txt");
    std::fs::write(&existing_path, "alpha\n").expect("seed existing file");

    assert_eq!(
        capture_file_diff_baseline(&existing_path.to_string_lossy()),
        (Some("alpha\n".to_string()), Some(true))
    );

    let missing_path = temp_dir.path().join("missing.txt");
    assert_eq!(
        capture_file_diff_baseline(&missing_path.to_string_lossy()),
        (None, Some(false))
    );

    assert_eq!(
        capture_file_diff_baseline(&temp_dir.path().to_string_lossy()),
        (None, Some(true))
    );
}

#[test]
fn resolve_codex_file_change_tool_call_snapshots_turns_missing_update_into_write() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let file_path = temp_dir.path().join("created-by-update.txt");

    let mut pending = std::collections::HashMap::new();
    let started = resolve_codex_file_change_tool_call_snapshots(
        CodexFileChangeSnapshot {
            id: Some("item_3".to_string()),
            phase: CodexToolCallPhase::Started,
            status: Some("in_progress".to_string()),
            changes: vec![CodexFileChange {
                path: file_path.display().to_string(),
                kind: "update".to_string(),
            }],
        },
        &mut pending,
    );

    assert_eq!(started.len(), 1);
    assert_eq!(started[0].tool_call.name, "file_change");

    std::fs::write(&file_path, "created\n").expect("create file");

    let completed = resolve_codex_file_change_tool_call_snapshots(
        CodexFileChangeSnapshot {
            id: Some("item_3".to_string()),
            phase: CodexToolCallPhase::Completed,
            status: Some("completed".to_string()),
            changes: vec![CodexFileChange {
                path: file_path.display().to_string(),
                kind: "update".to_string(),
            }],
        },
        &mut pending,
    );

    assert_eq!(completed.len(), 1);
    let tool_call = &completed[0].tool_call;
    assert_eq!(tool_call.name, "write");
    assert_eq!(
        tool_call.arguments,
        serde_json::json!({
            "file_path": file_path.display().to_string(),
            "content": "created\n",
        })
    );
    assert_eq!(
        tool_call
            .diff_context
            .as_ref()
            .and_then(|ctx| ctx.old_file_exists),
        Some(false)
    );
}

#[tokio::test]
async fn persist_assistant_message_snapshot_keeps_codex_tool_lifecycle_deduped_and_ordered() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    let context_id = IdeationSessionId::new();
    let assistant_message = create_assistant_message(
        ChatContextType::Ideation,
        context_id.as_str(),
        "",
        conversation_id.clone(),
        &[],
        &[],
    );
    let assistant_message_id = assistant_message.id.as_str().to_string();
    state
        .chat_message_repo
        .create(assistant_message)
        .await
        .expect("insert assistant message");

    let repo = Some(state.chat_message_repo.clone());
    let assistant_message_id_opt = Some(assistant_message_id.clone());

    let mut response_text = "First text block".to_string();
    let mut tool_calls = Vec::new();
    let mut content_blocks = vec![ContentBlockItem::Text {
        text: response_text.clone(),
    }];

    persist_assistant_message_snapshot(
        &repo,
        &assistant_message_id_opt,
        &response_text,
        &tool_calls,
        &content_blocks,
    )
    .await;

    upsert_codex_tool_call_snapshot(
        &mut tool_calls,
        &mut content_blocks,
        ToolCall {
            id: Some("item_1".to_string()),
            name: "ralphx::get_task_context".to_string(),
            arguments: serde_json::json!({ "task_id": "task-1" }),
            result: None,
            parent_tool_use_id: Some("toolu-parent-task".to_string()),
            diff_context: None,
            stats: None,
        },
    );

    persist_assistant_message_snapshot(
        &repo,
        &assistant_message_id_opt,
        &response_text,
        &tool_calls,
        &content_blocks,
    )
    .await;

    upsert_codex_tool_call_snapshot(
        &mut tool_calls,
        &mut content_blocks,
        ToolCall {
            id: Some("item_1".to_string()),
            name: "ralphx::get_task_context".to_string(),
            arguments: serde_json::json!({ "task_id": "task-1" }),
            result: Some(serde_json::json!({ "title": "Task" })),
            parent_tool_use_id: Some("toolu-parent-task".to_string()),
            diff_context: None,
            stats: None,
        },
    );

    response_text.push_str("\n\nSecond text block");
    content_blocks.push(ContentBlockItem::Text {
        text: "Second text block".to_string(),
    });

    flush_content_before_error(
        &repo,
        &assistant_message_id_opt,
        &response_text,
        &tool_calls,
        &content_blocks,
    )
    .await;

    let stored = state
        .chat_message_repo
        .get_by_id(&ChatMessageId::from_string(assistant_message_id))
        .await
        .expect("reload message")
        .expect("assistant message should exist");

    assert_eq!(stored.content, "First text block\n\nSecond text block");

    let stored_tool_calls: Vec<ToolCall> = serde_json::from_str(
        stored
            .tool_calls
            .as_deref()
            .expect("tool_calls should be persisted"),
    )
    .expect("tool_calls JSON should parse");
    assert_eq!(stored_tool_calls.len(), 1);
    assert_eq!(stored_tool_calls[0].id.as_deref(), Some("item_1"));
    assert_eq!(
        stored_tool_calls[0].parent_tool_use_id.as_deref(),
        Some("toolu-parent-task")
    );
    assert_eq!(
        stored_tool_calls[0].result,
        Some(serde_json::json!({ "title": "Task" }))
    );

    let stored_blocks: Vec<ContentBlockItem> = serde_json::from_str(
        stored
            .content_blocks
            .as_deref()
            .expect("content_blocks should be persisted"),
    )
    .expect("content_blocks JSON should parse");
    assert_eq!(stored_blocks.len(), 3);
    match &stored_blocks[0] {
        ContentBlockItem::Text { text } => assert_eq!(text, "First text block"),
        other => panic!("expected first block to be text, got {other:?}"),
    }
    match &stored_blocks[1] {
        ContentBlockItem::ToolUse {
            id,
            result,
            parent_tool_use_id,
            ..
        } => {
            assert_eq!(id.as_deref(), Some("item_1"));
            assert_eq!(result, &Some(serde_json::json!({ "title": "Task" })));
            assert_eq!(parent_tool_use_id.as_deref(), Some("toolu-parent-task"));
        }
        other => panic!("expected second block to be tool_use, got {other:?}"),
    }
    match &stored_blocks[2] {
        ContentBlockItem::Text { text } => assert_eq!(text, "Second text block"),
        other => panic!("expected third block to be text, got {other:?}"),
    }
}

#[tokio::test]
async fn persist_assistant_message_snapshot_keeps_claude_tool_result_ordered_and_in_place() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    let context_id = IdeationSessionId::new();
    let assistant_message = create_assistant_message(
        ChatContextType::Ideation,
        context_id.as_str(),
        "",
        conversation_id.clone(),
        &[],
        &[],
    );
    let assistant_message_id = assistant_message.id.as_str().to_string();
    state
        .chat_message_repo
        .create(assistant_message)
        .await
        .expect("insert assistant message");

    let repo = Some(state.chat_message_repo.clone());
    let assistant_message_id_opt = Some(assistant_message_id.clone());
    let mut processor = StreamProcessor::new();

    processor.process_message(StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![AssistantContent::Text {
                text: "First text block".to_string(),
            }],
            stop_reason: None,
            usage: None,
        },
        session_id: None,
    });
    persist_assistant_message_snapshot(
        &repo,
        &assistant_message_id_opt,
        &processor.response_text,
        &processor.tool_calls,
        &processor.content_blocks,
    )
    .await;

    processor.process_message(StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![AssistantContent::ToolUse {
                id: "toolu_1".to_string(),
                name: "bash".to_string(),
                input: serde_json::json!({ "command": "pwd" }),
            }],
            stop_reason: None,
            usage: None,
        },
        session_id: None,
    });
    persist_assistant_message_snapshot(
        &repo,
        &assistant_message_id_opt,
        &processor.response_text,
        &processor.tool_calls,
        &processor.content_blocks,
    )
    .await;

    let parsed_tool_result = StreamProcessor::parse_line(
        r#"{"type":"user","message":{"role":"user","content":[{"tool_use_id":"toolu_1","type":"tool_result","content":"/Users/test/project","is_error":false}]}}"#,
    )
    .expect("tool_result line should parse");
    processor.process_parsed_line(parsed_tool_result);

    processor.process_message(StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![AssistantContent::Text {
                text: "Second text block".to_string(),
            }],
            stop_reason: None,
            usage: None,
        },
        session_id: None,
    });

    flush_content_before_error(
        &repo,
        &assistant_message_id_opt,
        &processor.response_text,
        &processor.tool_calls,
        &processor.content_blocks,
    )
    .await;

    let stored = state
        .chat_message_repo
        .get_by_id(&ChatMessageId::from_string(assistant_message_id))
        .await
        .expect("reload message")
        .expect("assistant message should exist");

    assert_eq!(stored.content, "First text blockSecond text block");

    let stored_tool_calls: Vec<ToolCall> = serde_json::from_str(
        stored
            .tool_calls
            .as_deref()
            .expect("tool_calls should be persisted"),
    )
    .expect("tool_calls JSON should parse");
    assert_eq!(stored_tool_calls.len(), 1);
    assert_eq!(stored_tool_calls[0].id.as_deref(), Some("toolu_1"));
    assert_eq!(
        stored_tool_calls[0].result,
        Some(serde_json::json!("/Users/test/project"))
    );

    let stored_blocks: Vec<ContentBlockItem> = serde_json::from_str(
        stored
            .content_blocks
            .as_deref()
            .expect("content_blocks should be persisted"),
    )
    .expect("content_blocks JSON should parse");
    assert_eq!(stored_blocks.len(), 3);
    match &stored_blocks[0] {
        ContentBlockItem::Text { text } => assert_eq!(text, "First text block"),
        other => panic!("expected first block to be text, got {other:?}"),
    }
    match &stored_blocks[1] {
        ContentBlockItem::ToolUse { id, result, .. } => {
            assert_eq!(id.as_deref(), Some("toolu_1"));
            assert_eq!(result, &Some(serde_json::json!("/Users/test/project")));
        }
        other => panic!("expected second block to be tool_use, got {other:?}"),
    }
    match &stored_blocks[2] {
        ContentBlockItem::Text { text } => assert_eq!(text, "Second text block"),
        other => panic!("expected third block to be text, got {other:?}"),
    }
}
