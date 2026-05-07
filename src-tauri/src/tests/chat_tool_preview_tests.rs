use serde_json::json;

use crate::application::chat_service::tool_result_preview::{
    build_tool_result_preview_payload, tool_detail_ref,
};
use crate::commands::unified_chat_commands::preview_tool_payloads_for_message;

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
