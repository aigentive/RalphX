use serde_json::json;
use std::process::Stdio;
use std::sync::Arc;

use crate::application::chat_service::tool_result_preview::{
    build_live_tool_result_preview, build_live_tool_result_preview_for_tool_call,
    build_live_tool_result_preview_for_tool_id, build_tool_result_preview_payload,
    live_tool_result_activity_content, live_tool_result_activity_metadata,
    preview_tool_result_object, should_skip_tool_result_preview, tool_detail_ref,
};
use crate::application::chat_service::{
    process_stream_background, AgentToolCallPayload, AgentToolCallPreviewFields,
    StreamingStateCache,
};
use crate::commands::unified_chat_commands::preview_tool_payloads_for_message;
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{ChatContextType, ChatConversationId};
use crate::infrastructure::agents::claude::ToolCall;
use crate::infrastructure::memory::MemoryChatMessageRepository;
use tauri::test::MockRuntime;
use tokio_util::sync::CancellationToken;

#[test]
fn preview_tool_payloads_truncates_large_tool_results() {
    let long_result = (1..=14)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let tool_calls = json!([
        {
            "id": "tool-1",
            "name": "bash",
            "arguments": { "command": "printf" },
            "result": long_result,
        }
    ]);

    let (previewed_tool_calls, _) =
        preview_tool_payloads_for_message("conv-1", "msg-1", Some(tool_calls), None);
    let previewed_tool_calls = previewed_tool_calls.unwrap();
    let tool_call = &previewed_tool_calls[0];
    let preview = tool_call["result"].as_str().unwrap();

    assert_eq!(preview.lines().count(), 10);
    assert!(preview.contains("line 1"));
    assert!(preview.contains("line 10"));
    assert!(!preview.contains("line 11"));
    assert_eq!(tool_call["result_preview_truncated"], true);
    assert_eq!(tool_call["result_preview_line_count"], 14);
    assert_eq!(tool_call["result_preview_omitted_lines"], 4);
    assert_eq!(tool_call["detail_ref"]["conversation_id"], "conv-1");
    assert_eq!(tool_call["detail_ref"]["message_id"], "msg-1");
    assert_eq!(tool_call["detail_ref"]["tool_call_id"], "tool-1");
}

#[test]
fn preview_tool_payloads_keeps_small_tool_results_full() {
    let tool_calls = json!([
        {
            "id": "tool-1",
            "name": "bash",
            "arguments": { "command": "pwd" },
            "result": "short output",
        }
    ]);

    let (previewed_tool_calls, _) =
        preview_tool_payloads_for_message("conv-1", "msg-1", Some(tool_calls), None);
    let previewed_tool_calls = previewed_tool_calls.unwrap();
    let tool_call = &previewed_tool_calls[0];

    assert_eq!(tool_call["result"], "short output");
    assert!(tool_call.get("result_preview_truncated").is_none());
    assert!(tool_call.get("detail_ref").is_none());
}

#[test]
fn preview_tool_payloads_keeps_task_results_structured() {
    let task_result = json!({
        "subagent_type": "Explore",
        "content": (1..=14).map(|index| format!("line {index}")).collect::<Vec<_>>().join("\n"),
    });
    let tool_calls = json!([
        {
            "id": "task-1",
            "name": "Task",
            "arguments": { "description": "inspect" },
            "result": task_result,
        }
    ]);

    let (previewed_tool_calls, _) =
        preview_tool_payloads_for_message("conv-1", "msg-1", Some(tool_calls), None);
    let previewed_tool_calls = previewed_tool_calls.unwrap();
    let tool_call = &previewed_tool_calls[0];

    assert!(tool_call["result"].is_object());
    assert!(tool_call.get("result_preview_truncated").is_none());
    assert!(tool_call.get("detail_ref").is_none());
}

#[test]
fn preview_tool_payloads_adds_content_block_detail_refs() {
    let content_blocks = json!([
        { "type": "text", "text": "before" },
        {
            "type": "tool_use",
            "id": "tool-block-1",
            "name": "read",
            "arguments": { "file_path": "big.txt" },
            "result": "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk"
        }
    ]);

    let (_, previewed_content_blocks) =
        preview_tool_payloads_for_message("conv-1", "msg-1", None, Some(content_blocks));
    let previewed_content_blocks = previewed_content_blocks.unwrap();
    let block = &previewed_content_blocks[1];

    assert_eq!(block["result_preview_truncated"], true);
    assert_eq!(block["detail_ref"]["conversation_id"], "conv-1");
    assert_eq!(block["detail_ref"]["message_id"], "msg-1");
    assert_eq!(block["detail_ref"]["tool_call_id"], "tool-block-1");
    assert_eq!(block["detail_ref"]["content_block_index"], 1);
}

#[test]
fn live_preview_payloads_include_detail_refs() {
    let result = json!((1..=12)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n"));
    let detail_ref = tool_detail_ref("conv-1", "msg-1", Some("tool-live-1"), None);
    let preview =
        build_tool_result_preview_payload(Some("bash"), &result, Some(detail_ref)).unwrap();

    assert_eq!(preview.result.as_str().unwrap().lines().count(), 10);
    assert_eq!(preview.line_count, 12);
    assert_eq!(preview.omitted_lines, 2);
    assert_eq!(preview.detail_ref.unwrap()["tool_call_id"], "tool-live-1");
}

#[test]
fn live_preview_payloads_keep_delegation_results_structured() {
    let result = json!({
        "subagent_type": "Explore",
        "content": (1..=12).map(|index| format!("line {index}")).collect::<Vec<_>>().join("\n"),
    });

    let preview = build_tool_result_preview_payload(Some("Task"), &result, None);

    assert!(preview.is_none());
}

#[test]
fn preview_tool_payloads_handles_non_array_payloads_and_non_tool_blocks() {
    let (tool_calls, content_blocks) = preview_tool_payloads_for_message(
        "conv-1",
        "msg-1",
        Some(json!({ "not": "an-array" })),
        Some(json!([
            { "type": "text", "text": "not a tool" },
            "not an object",
            { "type": "tool_use", "name": "read", "result": null }
        ])),
    );

    assert_eq!(tool_calls.unwrap(), json!({ "not": "an-array" }));
    let blocks = content_blocks.unwrap();
    assert!(blocks[0].get("result_preview_truncated").is_none());
    assert!(blocks[2].get("result_preview_truncated").is_none());
}

#[test]
fn preview_tool_payloads_ignores_already_previewed_results() {
    let tool_calls = json!([
        {
            "id": "tool-1",
            "name": "bash",
            "arguments": {},
            "result": "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\nline 9\nline 10\nline 11",
            "result_preview_truncated": true
        }
    ]);

    let (previewed_tool_calls, _) =
        preview_tool_payloads_for_message("conv-1", "msg-1", Some(tool_calls), None);

    let tool_call = &previewed_tool_calls.unwrap()[0];
    assert!(tool_call["result"].as_str().unwrap().contains("line 11"));
    assert!(tool_call.get("detail_ref").is_none());
}

#[test]
fn live_preview_payloads_extract_text_from_result_shapes() {
    let array_result = json!([
        { "type": "text", "text": "a" },
        { "type": "text", "text": "b" }
    ]);
    assert!(
        build_tool_result_preview_payload(Some("bash"), &array_result, None).is_none(),
        "short text arrays should not be previewed"
    );

    for key in [
        "text",
        "content",
        "output",
        "aggregated_output",
        "aggregatedOutput",
    ] {
        let mut object = serde_json::Map::new();
        object.insert(
            key.to_string(),
            json!((1..=12)
                .map(|index| format!("{key} {index}"))
                .collect::<Vec<_>>()
                .join("\n")),
        );
        let result = serde_json::Value::Object(object);
        let preview = build_tool_result_preview_payload(Some("bash"), &result, None).unwrap();
        assert!(preview
            .result
            .as_str()
            .unwrap()
            .contains(&format!("{key} 10")));
    }

    let nested_content = json!({
        "content": [
            { "type": "text", "text": "nested 1" },
            { "type": "text", "text": (2..=12).map(|index| format!("nested {index}")).collect::<Vec<_>>().join("\n") }
        ]
    });
    let preview = build_tool_result_preview_payload(Some("bash"), &nested_content, None).unwrap();
    assert!(preview.result.as_str().unwrap().contains("nested 10"));
}

#[test]
fn live_preview_payloads_handle_json_fallback_and_char_limit() {
    let long_value = json!({
        "items": (0..500).map(|index| json!({ "index": index, "value": "x".repeat(20) })).collect::<Vec<_>>()
    });
    let preview = build_live_tool_result_preview(Some("bash"), &long_value, None);

    assert!(preview.is_previewed());
    assert!(preview.result.as_str().unwrap().chars().count() <= 4_000);

    let non_text_value = json!(["plain", "array", 1, true]);
    assert!(!build_live_tool_result_preview(Some("bash"), &non_text_value, None).is_previewed());
    assert!(!build_live_tool_result_preview(None, &long_value, None).is_previewed());
}

#[test]
fn preview_helpers_cover_fallback_and_skip_edges() {
    let nested_content = json!({
        "content": [
            { "type": "image", "source": "ignored" },
            { "type": "text", "text": (1..=12).map(|index| format!("nested {index}")).collect::<Vec<_>>().join("\n") }
        ]
    });
    let nested_preview =
        build_tool_result_preview_payload(Some("bash"), &nested_content, None).unwrap();
    assert!(nested_preview
        .result
        .as_str()
        .unwrap()
        .contains("nested 10"));

    assert!(
        !build_live_tool_result_preview(Some("bash"), &json!(true), None).is_previewed(),
        "primitive JSON fallback stays full when it is small"
    );
    assert!(
        !build_live_tool_result_preview(Some("bash"), &json!(""), None).is_previewed(),
        "empty string results should not allocate a preview"
    );

    let long_single_line = json!("x".repeat(4_100));
    let char_limited = build_live_tool_result_preview(Some("bash"), &long_single_line, None);
    assert!(char_limited.is_previewed());
    assert_eq!(char_limited.result.as_str().unwrap().chars().count(), 4_000);

    assert!(!should_skip_tool_result_preview(None));

    let mut missing_result = serde_json::Map::new();
    missing_result.insert("name".to_string(), json!("bash"));
    assert!(!preview_tool_result_object(&mut missing_result, None));
}

#[test]
fn live_preview_fields_flatten_into_agent_tool_call_payload_json() {
    let result = json!((1..=12)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n"));
    let detail_ref = tool_detail_ref("conv-1", "msg-1", Some("tool-1"), None);
    let preview =
        build_tool_result_preview_payload(Some("bash"), &result, Some(detail_ref)).unwrap();
    let payload = AgentToolCallPayload {
        tool_name: "bash".to_string(),
        tool_id: Some("tool-1".to_string()),
        arguments: json!({ "command": "cat big.log" }),
        result: Some(preview.result.clone()),
        preview: AgentToolCallPreviewFields::from_tool_result_preview(Some(&preview)),
        conversation_id: "conv-1".to_string(),
        context_type: "project".to_string(),
        context_id: "project-1".to_string(),
        diff_context: None,
        parent_tool_use_id: None,
        seq: 7,
    };

    let value = serde_json::to_value(payload).unwrap();

    assert_eq!(value["result_preview_truncated"], true);
    assert_eq!(value["result_preview_line_count"], 12);
    assert_eq!(value["result_preview_omitted_lines"], 2);
    assert_eq!(value["detail_ref"]["tool_call_id"], "tool-1");
    assert!(value.get("preview").is_none());
}

#[test]
fn live_preview_for_tool_id_uses_matching_tool_and_detail_ref() {
    let tool_calls = vec![
        ToolCall {
            id: Some("tool-small".to_string()),
            name: "Task".to_string(),
            arguments: json!({}),
            result: None,
            parent_tool_use_id: None,
            diff_context: None,
            stats: None,
        },
        ToolCall {
            id: Some("tool-heavy".to_string()),
            name: "bash".to_string(),
            arguments: json!({ "command": "cat big.log" }),
            result: None,
            parent_tool_use_id: None,
            diff_context: None,
            stats: None,
        },
    ];
    let result = json!((1..=12)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n"));

    let preview = build_live_tool_result_preview_for_tool_id(
        &tool_calls,
        Some("conv-1"),
        Some("msg-1"),
        "tool-heavy",
        &result,
    );

    assert!(preview.is_previewed());
    assert_eq!(preview.result.as_str().unwrap().lines().count(), 10);
    let detail_ref = &preview
        .preview
        .as_ref()
        .unwrap()
        .detail_ref
        .as_ref()
        .unwrap();
    assert_eq!(detail_ref["conversation_id"], "conv-1");
    assert_eq!(detail_ref["message_id"], "msg-1");
    assert_eq!(detail_ref["tool_call_id"], "tool-heavy");

    let unmatched = build_live_tool_result_preview_for_tool_id(
        &tool_calls,
        Some("conv-1"),
        Some("msg-1"),
        "missing-tool",
        &result,
    );
    assert!(!unmatched.is_previewed());
}

#[test]
fn live_preview_for_tool_call_builds_completed_event_payload() {
    let tool_call = ToolCall {
        id: Some("tool-heavy".to_string()),
        name: "bash".to_string(),
        arguments: json!({ "command": "cat big.log" }),
        result: Some(json!((1..=12)
            .map(|index| format!("line {index}"))
            .collect::<Vec<_>>()
            .join("\n"))),
        parent_tool_use_id: None,
        diff_context: None,
        stats: None,
    };
    let preview =
        build_live_tool_result_preview_for_tool_call("conv-1", Some("msg-1"), &tool_call).unwrap();

    let payload = AgentToolCallPayload::from_completed_tool_call(
        &tool_call,
        Some(&preview),
        "conv-1",
        "project",
        "project-1",
        Some(json!({ "file_path": "big.log" })),
        Some("parent-tool".to_string()),
        9,
    );
    let value = serde_json::to_value(payload).unwrap();

    assert_eq!(value["tool_name"], "bash");
    assert_eq!(value["result"].as_str().unwrap().lines().count(), 10);
    assert_eq!(value["result_preview_truncated"], true);
    assert_eq!(value["detail_ref"]["message_id"], "msg-1");
    assert_eq!(value["diff_context"]["file_path"], "big.log");
    assert_eq!(value["parent_tool_use_id"], "parent-tool");
    assert_eq!(value["seq"], 9);
}

#[test]
fn live_tool_result_payload_helpers_use_preview_result() {
    let result = json!((1..=12)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n"));
    let preview = build_live_tool_result_preview(
        Some("bash"),
        &result,
        Some(tool_detail_ref("conv-1", "msg-1", Some("tool-heavy"), None)),
    );

    let payload = AgentToolCallPayload::from_live_tool_result(
        "tool-heavy",
        &preview,
        "conv-1",
        "project",
        "project-1",
        Some("parent-tool".to_string()),
        11,
    );
    let value = serde_json::to_value(payload).unwrap();

    assert_eq!(value["tool_name"], "result:tool-heavy");
    assert_eq!(value["result"].as_str().unwrap().lines().count(), 10);
    assert_eq!(value["result_preview_truncated"], true);
    assert_eq!(value["detail_ref"]["tool_call_id"], "tool-heavy");
    assert_eq!(value["parent_tool_use_id"], "parent-tool");

    let content = live_tool_result_activity_content(&preview);
    let metadata = live_tool_result_activity_metadata("tool-heavy", &preview);

    assert!(content.contains("line 10"));
    assert_eq!(metadata["tool_use_id"], "tool-heavy");
    assert_eq!(metadata["result_preview_truncated"], true);
}

#[tokio::test]
async fn stream_background_previews_heavy_live_tool_result() {
    let app = crate::testing::create_mock_app();
    let conversation_id = ChatConversationId::from_string("conv-live-preview".to_string());
    let long_result = (1..=12)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let assistant = json!({
        "type": "assistant",
        "message": {
            "content": [{
                "type": "tool_use",
                "id": "toolu_heavy",
                "name": "bash",
                "input": { "command": "cat big.log" }
            }]
        }
    });
    let tool_result = json!({
        "type": "user",
        "message": {
            "content": [{
                "type": "tool_result",
                "tool_use_id": "toolu_heavy",
                "content": long_result,
                "is_error": false
            }]
        }
    });
    let stream = format!("{assistant}\n{tool_result}\n");
    let child = tokio::process::Command::new("python3")
        .arg("-c")
        .arg("import sys; sys.stdout.write(sys.argv[1])")
        .arg(stream)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn stream fixture");

    let outcome = process_stream_background::<MockRuntime>(
        child,
        AgentHarnessKind::Claude,
        ChatContextType::TaskExecution,
        "task-live-preview",
        &conversation_id,
        Some(app.handle().clone()),
        None,
        None,
        Some(Arc::new(MemoryChatMessageRepository::new())),
        Some("msg-live-preview".to_string()),
        None,
        CancellationToken::new(),
        None,
        false,
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
    .expect("stream should process");

    assert_eq!(outcome.tool_calls.len(), 1);
    assert_eq!(outcome.tool_calls[0].name, "bash");
    assert_eq!(
        outcome.tool_calls[0].result.as_ref().unwrap(),
        &json!(long_result)
    );
}
