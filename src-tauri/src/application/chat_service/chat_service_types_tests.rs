use crate::application::chat_service::{
    AgentErrorPayload, AgentMessageQueuedPayload, AgentMessageRenderReadyPayload,
    AgentRunCompletedPayload, AgentRunStartedPayload, AgentThinkingPayload,
};
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{
    ChatConversationId, ChatMessage, ChatMessageId, ChatTimelineItem, ChatTimelineItemKind,
    ChatTimelineItemStatus, MessageRole, ProjectId,
};

use super::retains_full_raw_tool_payload;

#[test]
fn full_raw_payload_allowlist_matches_renderer_hydration_tools() {
    for name in [
        "edit",
        "mcp__ralphx__write",
        "ralphx::ask_user_question",
        "mcp__ralphx_internal__delegate_start",
        "delegate_wait",
        "delegate_cancel",
        "delegate_terminal",
    ] {
        assert!(retains_full_raw_tool_payload(name), "{name}");
    }

    for name in ["bash", "Read", "mcp__ralphx__get_artifact"] {
        assert!(!retains_full_raw_tool_payload(name), "{name}");
    }
}

#[test]
fn agent_thinking_payload_serializes_committed_streaming_and_settled_contracts() {
    let streaming = AgentThinkingPayload {
        text: "partial reasoning".to_string(),
        run_id: Some("run-1".to_string()),
        block_index: Some(0),
        conversation_id: "conversation-1".to_string(),
        context_type: "task_execution".to_string(),
        context_id: "task-1".to_string(),
        seq: 7,
        append_to_previous: true,
        duration_ms: None,
        reasoning_tokens: None,
        is_settled: false,
    };
    let settled = AgentThinkingPayload {
        text: String::new(),
        duration_ms: Some(1_500),
        is_settled: true,
        ..streaming.clone()
    };
    let codex_settled = AgentThinkingPayload {
        text: String::new(),
        append_to_previous: true,
        reasoning_tokens: Some(426),
        is_settled: true,
        ..streaming.clone()
    };

    let expected_streaming: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/agent_thinking_payload.streaming.json"
    )))
    .expect("streaming fixture must be valid JSON");
    let expected_settled: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/agent_thinking_payload.settled.json"
    )))
    .expect("settled fixture must be valid JSON");
    let expected_codex_settled: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/agent_thinking_payload.codex_settled.json"
    )))
    .expect("Codex settled fixture must be valid JSON");

    assert_eq!(serde_json::to_value(streaming).unwrap(), expected_streaming);
    assert_eq!(serde_json::to_value(settled).unwrap(), expected_settled);
    assert_eq!(
        serde_json::to_value(codex_settled).unwrap(),
        expected_codex_settled
    );
}

/// The allowlist is intentionally triplicated (application helper plus both
/// repository write guards) because infrastructure must not import
/// application-layer code. This parity matrix keeps the three copies from
/// drifting: an edit to one that is not mirrored fails here.
#[test]
fn full_raw_payload_allowlist_copies_agree_across_all_implementations() {
    let matrix = [
        "edit",
        "Edit",
        "write",
        "WRITE",
        "tools::edit",
        "ask_user_question",
        "mcp__ralphx__ask_user_question",
        "ralphx::ask_user_question",
        "ralphx_internal:ask_user_question",
        "delegate_start",
        "delegate_wait",
        "delegate_cancel",
        "delegate_terminal",
        "mcp__ralphx_internal__delegate_start",
        "bash",
        "Read",
        "grep",
        "apply_patch",
        "MultiEdit",
        "str_replace_editor",
        "mcp__ralphx__get_artifact",
        "editors",
        "rewrite",
        "",
        "  edit  ",
    ];
    for name in matrix {
        let application = retains_full_raw_tool_payload(name);
        let sqlite =
            crate::infrastructure::sqlite::sqlite_chat_timeline_repo::retains_full_raw_tool_payload(
                name,
            );
        let memory =
            crate::infrastructure::memory::memory_chat_timeline_repo::retains_full_raw_tool_payload(
                name,
            );
        assert_eq!(application, sqlite, "sqlite copy drifted for {name:?}");
        assert_eq!(application, memory, "memory copy drifted for {name:?}");
    }
}

#[test]
fn agent_run_started_payload_serde_snake_case() {
    let payload = AgentRunStartedPayload {
        run_id: "run-1".to_string(),
        conversation_id: "conv-1".to_string(),
        context_type: "task_execution".to_string(),
        context_id: "task-1".to_string(),
        run_chain_id: None,
        parent_run_id: None,
        agent_name: Some("ralphx-workspace-reviewer".to_string()),
        launch_role: Some("workspace_reviewer".to_string()),
        started_at: Some("2026-07-31T00:00:00Z".to_string()),
        effective_model_id: Some("claude-sonnet-4-6".to_string()),
        effective_model_label: Some("Sonnet 4.6".to_string()),
        provider_harness: Some("claude".to_string()),
        provider_session_id: Some("session-123".to_string()),
        service_tier: None,
    };

    let value = serde_json::to_value(&payload).expect("serialization failed");

    assert_eq!(value["effective_model_id"], "claude-sonnet-4-6");
    assert_eq!(value["effective_model_label"], "Sonnet 4.6");
    assert_eq!(value["provider_harness"], "claude");
    assert_eq!(value["provider_session_id"], "session-123");
    assert_eq!(value["agent_name"], "ralphx-workspace-reviewer");
    assert_eq!(value["launch_role"], "workspace_reviewer");
    assert_eq!(value["started_at"], "2026-07-31T00:00:00Z");

    assert_eq!(value["run_id"], "run-1");
    assert_eq!(value["conversation_id"], "conv-1");
    assert_eq!(value["context_type"], "task_execution");
    assert_eq!(value["context_id"], "task-1");
    assert!(value.get("effectiveModelId").is_none());
    assert!(value.get("effectiveModelLabel").is_none());
    assert!(value.get("providerHarness").is_none());
    assert!(value.get("providerSessionId").is_none());
}

#[test]
fn agent_run_started_payload_serde_skips_none_fields() {
    let payload = AgentRunStartedPayload {
        run_id: "run-1".to_string(),
        conversation_id: "conv-1".to_string(),
        context_type: "task_execution".to_string(),
        context_id: "task-1".to_string(),
        run_chain_id: None,
        parent_run_id: None,
        agent_name: None,
        launch_role: None,
        started_at: None,
        effective_model_id: None,
        effective_model_label: None,
        provider_harness: None,
        provider_session_id: None,
        service_tier: None,
    };

    let value = serde_json::to_value(&payload).expect("serialization failed");

    // None fields with skip_serializing_if should be absent
    assert!(value.get("effective_model_id").is_none());
    assert!(value.get("effective_model_label").is_none());
    assert!(value.get("provider_harness").is_none());
    assert!(value.get("provider_session_id").is_none());
    assert!(value.get("run_chain_id").is_none());
    assert!(value.get("parent_run_id").is_none());
    assert!(value.get("agent_name").is_none());
    assert!(value.get("launch_role").is_none());
    assert!(value.get("started_at").is_none());
}

#[test]
fn agent_run_started_payload_helper_serializes_provider_metadata() {
    let payload = AgentRunStartedPayload::with_provider_session(
        "run-1",
        "conv-1",
        "task_execution",
        "task-1",
        None,
        None,
        Some("gpt-4.5".to_string()),
        Some("GPT-4.5".to_string()),
        Some(AgentHarnessKind::Codex),
        Some("thread-123".to_string()),
    );

    assert_eq!(payload.provider_harness, Some("codex".to_string()));
    assert_eq!(payload.provider_session_id, Some("thread-123".to_string()));
    assert_eq!(payload.effective_model_id, Some("gpt-4.5".to_string()));
    assert_eq!(payload.effective_model_label, Some("GPT-4.5".to_string()));
}

#[test]
fn agent_run_completed_payload_sets_legacy_claude_alias_only_for_claude() {
    let claude_payload = AgentRunCompletedPayload::with_provider_session(
        "conv-1",
        "ideation",
        "session-1",
        Some(AgentHarnessKind::Claude),
        Some("claude-session-123".to_string()),
        None,
    );
    let codex_payload = AgentRunCompletedPayload::with_provider_session(
        "conv-2",
        "ideation",
        "session-2",
        Some(AgentHarnessKind::Codex),
        Some("codex-thread-123".to_string()),
        None,
    );

    assert_eq!(
        claude_payload.claude_session_id,
        Some("claude-session-123".to_string())
    );
    assert_eq!(claude_payload.provider_harness, Some("claude".to_string()));
    assert_eq!(
        claude_payload.provider_session_id,
        Some("claude-session-123".to_string())
    );

    assert_eq!(codex_payload.claude_session_id, None);
    assert_eq!(codex_payload.provider_harness, Some("codex".to_string()));
    assert_eq!(
        codex_payload.provider_session_id,
        Some("codex-thread-123".to_string())
    );
}

#[test]
fn agent_run_completed_payload_serializes_run_id_for_terminal_correlation() {
    let payload = AgentRunCompletedPayload::with_provider_session_and_run_id(
        Some("run-1".to_string()),
        "conv-1",
        "project",
        "project-1",
        Some(AgentHarnessKind::Codex),
        Some("thread-123".to_string()),
        None,
    );

    let value = serde_json::to_value(&payload).expect("serialization failed");

    assert_eq!(value["run_id"], "run-1");
    assert!(value.get("agentRunId").is_none());
}

#[test]
fn agent_error_payload_serializes_agent_run_id_for_terminal_correlation() {
    let payload = AgentErrorPayload {
        conversation_id: Some("conv-1".to_string()),
        context_type: "task_execution".to_string(),
        context_id: "task-1".to_string(),
        agent_run_id: Some("run-1".to_string()),
        error: "boom".to_string(),
        stderr: Some("boom".to_string()),
    };

    let value = serde_json::to_value(&payload).expect("serialization failed");

    assert_eq!(value["agent_run_id"], "run-1");
    assert!(value.get("agentRunId").is_none());
}

#[test]
fn agent_message_queued_payload_serializes_attachment_ids() {
    let payload = AgentMessageQueuedPayload {
        message_id: "queued-1".to_string(),
        content: "queued with file".to_string(),
        context_type: "project".to_string(),
        context_id: "conversation-1".to_string(),
        conversation_id: Some("conversation-1".to_string()),
        created_at: "2026-01-24T10:00:00Z".to_string(),
        attachment_ids: vec!["att-1".to_string()],
    };

    let value = serde_json::to_value(&payload).expect("serialization failed");

    assert_eq!(value["attachment_ids"], serde_json::json!(["att-1"]));
    assert!(value.get("attachmentIds").is_none());
}

#[test]
fn message_render_ready_payload_serializes_canonical_message_and_timeline() {
    let conversation_id = ChatConversationId::from_string("11111111-1111-1111-1111-111111111111");
    let message_id = ChatMessageId::from_string("msg-1");
    let content_blocks = serde_json::json!([
        { "type": "text", "text": "Done" },
        {
            "type": "tool_use",
            "id": "toolu-1",
            "name": "Read",
            "arguments": { "file_path": "src/app.ts" },
            "result": "ok"
        }
    ]);
    let tool_calls = serde_json::json!([
        {
            "id": "toolu-1",
            "name": "Read",
            "arguments": { "file_path": "src/app.ts" },
            "result": "ok"
        }
    ]);
    let mut message =
        ChatMessage::user_in_project(ProjectId::from_string("project-1".to_string()), "Done");
    message.id = message_id.clone();
    message.conversation_id = Some(conversation_id.clone());
    message.role = MessageRole::Orchestrator;
    message.tool_calls = Some(tool_calls.to_string());
    message.content_blocks = Some(content_blocks.to_string());
    message.provider_harness = Some(AgentHarnessKind::Codex);
    message.provider_session_id = Some("thread-1".to_string());

    let mut item = ChatTimelineItem::for_message_block(
        message_id,
        conversation_id,
        1,
        MessageRole::Orchestrator,
        ChatTimelineItemKind::ToolUse,
    );
    item.sequence = 42;
    item.status = ChatTimelineItemStatus::Finalized;
    item.tool_call_id = Some("toolu-1".to_string());
    item.tool_name = Some("Read".to_string());
    item.input_json = Some(serde_json::json!({ "file_path": "src/app.ts" }).to_string());
    item.result_json = Some(serde_json::json!("ok").to_string());
    item.raw_block_json =
        Some(serde_json::json!({ "diff_context": { "file_path": "src/app.ts" } }).to_string());
    item.finalized_at = Some(item.updated_at);

    let payload =
        AgentMessageRenderReadyPayload::from_message_and_timeline_items(&message, vec![item])
            .expect("payload");
    let value = serde_json::to_value(payload).expect("serialization failed");

    assert_eq!(value["message"]["id"], "msg-1");
    assert_eq!(
        value["message"]["conversation_id"],
        "11111111-1111-1111-1111-111111111111"
    );
    assert_eq!(value["message"]["provider_harness"], "codex");
    assert_eq!(value["message"]["content_blocks"][0]["text"], "Done");
    assert_eq!(value["timeline_items"][0]["message_id"], "msg-1");
    assert_eq!(value["timeline_items"][0]["sequence"], 42);
    assert_eq!(value["timeline_items"][0]["tool_call"]["id"], "toolu-1");
    assert_eq!(
        value["timeline_items"][0]["tool_call"]["detail_ref"]["timeline_item_id"],
        "block:msg-1:1"
    );
    assert_eq!(
        value["timeline_items"][0]["tool_call"]["diff_context"]["file_path"],
        "src/app.ts"
    );
}

#[test]
fn message_render_ready_payload_handles_empty_and_text_timeline_items() {
    let conversation_id = ChatConversationId::from_string("22222222-2222-2222-2222-222222222222");
    let message_id = ChatMessageId::from_string("msg-text");
    let mut message =
        ChatMessage::user_in_project(ProjectId::from_string("project-1".to_string()), "Done");
    message.id = message_id.clone();
    message.conversation_id = Some(conversation_id.clone());
    message.role = MessageRole::Orchestrator;
    message.tool_calls = Some("not-json".to_string());
    message.content_blocks = Some("not-json".to_string());

    assert!(
        AgentMessageRenderReadyPayload::from_message_and_timeline_items(&message, Vec::new())
            .is_none()
    );

    let mut item = ChatTimelineItem::for_message_block(
        message_id,
        conversation_id,
        0,
        MessageRole::Orchestrator,
        ChatTimelineItemKind::Text,
    );
    item.sequence = 7;
    item.status = ChatTimelineItemStatus::Finalized;
    item.text = Some("Done".to_string());
    item.finalized_at = Some(item.updated_at);

    let payload =
        AgentMessageRenderReadyPayload::from_message_and_timeline_items(&message, vec![item])
            .expect("payload");
    let value = serde_json::to_value(payload).expect("serialization failed");

    assert!(value["message"]["tool_calls"].is_null());
    assert!(value["message"]["content_blocks"].is_null());
    assert_eq!(value["timeline_items"][0]["kind"], "text");
    assert_eq!(value["timeline_items"][0]["content"], "Done");
    assert_eq!(
        value["timeline_items"][0]["content_blocks"][0]["type"],
        "text"
    );
    assert_eq!(
        value["timeline_items"][0]["content_blocks"][0]["text"],
        "Done"
    );
    assert!(value["timeline_items"][0]["tool_call"].is_null());
}
