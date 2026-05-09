use super::{
    codex_tool_call_content_block, flush_content_before_error, format_agent_exit_stderr,
    persist_assistant_message_snapshot, process_codex_stream_background, process_exit_details,
    provider_session_ref_for_harness, resolve_codex_file_change_tool_call_snapshots,
    stream_mode_for_harness, upsert_codex_tool_call_snapshot, ProcessExitDetails, StreamOutcome,
    StreamingStateCache,
};
use crate::application::chat_service::chat_service_context::create_assistant_message;
use crate::application::chat_service::chat_service_errors::{ProviderErrorCategory, StreamError};
use crate::application::AppState;
use crate::domain::agents::{AgentHarnessKind, HarnessStreamMode};
use crate::domain::entities::{
    ChatContextType, ChatConversationId, ChatMessageId, IdeationSessionId,
};
use crate::infrastructure::agents::claude::{
    AssistantContent, AssistantMessage, ContentBlockItem, StreamMessage, StreamProcessor, ToolCall,
};
use crate::infrastructure::agents::{CodexFileChange, CodexFileChangeSnapshot, CodexToolCallPhase};
use std::os::unix::process::ExitStatusExt;
use std::process::Stdio;
use tauri::test::MockRuntime;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

async fn spawn_codex_jsonl_process(lines: &[&str]) -> tokio::process::Child {
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

async fn run_codex_stream_lines(lines: &[&str]) -> Result<StreamOutcome, StreamError> {
    let child = spawn_codex_jsonl_process(lines).await;
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

#[tokio::test]
async fn codex_stream_local_command_failures_are_agent_exit_not_provider_pause() {
    let result = run_codex_stream_lines(
        &[
            r#"{"type":"item.completed","item":{"type":"command_execution","id":"cmd-1","command":"rg rate_limit missing.rs","status":"failed","aggregated_output":"rg: missing.rs: No such file or directory\nlocal enum rate_limit","exit_code":2}}"#,
            r#"{"type":"item.completed","item":{"type":"command_execution","id":"cmd-2","status":"failed","exit_code":7}}"#,
        ],
    )
    .await
    .expect_err("local command failures should surface as an agent error");

    match result {
        StreamError::AgentExit { stderr, .. } => {
            assert!(stderr.contains("No such file or directory"));
            assert!(stderr.contains("rate_limit"));
            assert!(stderr.contains("Codex command_execution failed with exit code 7"));
        }
        other => panic!("expected local command failures to remain AgentExit, got {other:?}"),
    }
}

#[tokio::test]
async fn codex_stream_mcp_tool_failure_with_rate_limit_text_is_agent_exit() {
    let result = run_codex_stream_lines(
        &[r#"{"type":"item.completed","item":{"type":"mcp_tool_call","id":"tool-1","server":"ralphx","tool":"delegate_start","error":{"message":"delegate_start failed after reading local rate_limit metadata"}}}"#],
    )
    .await
    .expect_err("local MCP failure should surface as an agent error");

    match result {
        StreamError::AgentExit { stderr, .. } => {
            assert!(stderr.contains("delegate_start failed"));
            assert!(stderr.contains("rate_limit"));
        }
        other => panic!("expected local MCP failure to remain AgentExit, got {other:?}"),
    }
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
