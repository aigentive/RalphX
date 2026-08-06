use serde_json::json;
use std::process::Stdio;
use std::sync::Arc;

use crate::application::chat_service::tool_result_preview::{
    build_live_tool_argument_preview, build_live_tool_result_preview,
    build_live_tool_result_preview_for_tool_call, build_live_tool_result_preview_for_tool_id,
    build_tool_result_preview_payload, live_tool_result_activity_content,
    live_tool_result_activity_metadata, preview_tool_result_object,
    should_skip_tool_result_preview, tool_detail_ref,
};
use crate::application::chat_service::{
    process_stream_background, AgentToolCallPayload, AgentToolCallPreviewFields,
    StreamingStateCache,
};
use crate::commands::unified_chat_commands::preview_tool_payloads_for_message;
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{ChatContextType, ChatConversationId};
use crate::infrastructure::agents::claude::{DiffContext, ToolCall};
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
    assert_eq!(tool_call["result_preview_paths"], json!(["$"]));
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
fn preview_tool_payloads_keeps_ask_user_question_results_structured() {
    let answer_result = json!({
        "answers": [
            {
                "id": "scope",
                "request_id": "req-1",
                "header": "Scope",
                "question": "Which area should we focus on?",
                "options": [
                    { "label": "Backend", "value": "backend" },
                    { "label": "Frontend", "value": "frontend" },
                    { "label": "Both", "value": "both" }
                ],
                "selected_options": ["backend"],
                "text": null,
                "skipped": false
            }
        ],
        "request_id": "req-1",
        "question_count": 1
    });
    let tool_calls = json!([
        {
            "id": "ask-1",
            "name": "mcp__ralphx__ask_user_question",
            "arguments": { "question": "Which area should we focus on?" },
            "result": answer_result.clone(),
        }
    ]);
    let content_blocks = json!([
        {
            "type": "tool_use",
            "id": "ask-1",
            "name": "mcp__ralphx__ask_user_question",
            "arguments": { "question": "Which area should we focus on?" },
            "result": answer_result,
        }
    ]);

    let (previewed_tool_calls, previewed_content_blocks) = preview_tool_payloads_for_message(
        "conv-1",
        "msg-1",
        Some(tool_calls),
        Some(content_blocks),
    );
    let previewed_tool_calls = previewed_tool_calls.unwrap();
    let previewed_content_blocks = previewed_content_blocks.unwrap();
    let tool_call = &previewed_tool_calls[0];
    let content_block = &previewed_content_blocks[0];

    assert_eq!(
        tool_call["result"]["answers"][0]["question"],
        "Which area should we focus on?"
    );
    assert_eq!(
        content_block["result"]["answers"][0]["selected_options"],
        json!(["backend"])
    );
    assert!(tool_call.get("result_preview_truncated").is_none());
    assert!(content_block.get("result_preview_truncated").is_none());
    assert!(tool_call.get("detail_ref").is_none());
    assert!(content_block.get("detail_ref").is_none());
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
    assert_eq!(preview.paths, vec!["$".to_string()]);
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
            .get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap()
            .contains(&format!("{key} 10")));
        assert_eq!(preview.paths, vec![format!("$.{key}")]);
    }

    let task_result = json!({
        "success": true,
        "task": {
            "task_number": 7,
            "title": "Validate ledger labels",
            "details": (1..=600).map(|index| format!("detail {index}")).collect::<Vec<_>>().join("\n")
        }
    });
    let preview =
        build_tool_result_preview_payload(Some("ralphx::complete_agent_task"), &task_result, None)
            .unwrap();
    assert_eq!(preview.result["task"]["task_number"], 7);
    assert_eq!(preview.result["task"]["title"], "Validate ledger labels");
    assert!(preview.result["task"]["details"]
        .as_str()
        .unwrap()
        .contains("detail 10"));
    assert_eq!(preview.paths, vec!["$.task.details".to_string()]);

    let nested_content = json!({
        "content": [
            { "type": "text", "text": "nested 1" },
            { "type": "text", "text": (2..=12).map(|index| format!("nested {index}")).collect::<Vec<_>>().join("\n") }
        ]
    });
    let preview = build_tool_result_preview_payload(Some("bash"), &nested_content, None).unwrap();
    assert!(preview.result["content"][1]["text"]
        .as_str()
        .unwrap()
        .contains("nested 10"));
    assert_eq!(preview.paths, vec!["$.content[1].text".to_string()]);
}

#[test]
fn live_preview_payloads_preserve_parseable_json_mcp_text_content() {
    let artifact_content = "Detailed artifact line.\n".repeat(600);
    let artifact = json!({
        "id": "artifact-preview-1",
        "title": "Previewable Artifact",
        "artifact_type": "design_doc",
        "content": artifact_content,
        "version": 3
    });
    let result = json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&artifact).expect("artifact json")
        }]
    });

    let preview =
        build_tool_result_preview_payload(Some("mcp__ralphx__get_artifact"), &result, None)
            .expect("artifact result should be previewed");
    let preview_text = preview.result["content"][0]["text"]
        .as_str()
        .expect("preview text content");
    let parsed_preview: serde_json::Value =
        serde_json::from_str(preview_text).expect("preview text remains valid JSON");

    assert_eq!(parsed_preview["title"], "Previewable Artifact");
    assert_eq!(parsed_preview["artifact_type"], "design_doc");
    assert_eq!(parsed_preview["version"], 3);
    assert!(
        parsed_preview["content"]
            .as_str()
            .expect("content preview string")
            .len()
            < artifact_content.len()
    );
    assert_eq!(preview.paths, vec!["$.content[0].text.content".to_string()]);
}

#[test]
fn live_preview_payloads_handle_json_fallback_and_char_limit() {
    let long_value = json!({
        "items": (0..500).map(|index| json!({ "index": index, "value": "x".repeat(20) })).collect::<Vec<_>>()
    });
    let preview = build_live_tool_result_preview(Some("bash"), &long_value, None);

    assert!(preview.is_previewed());
    let preview_items = preview.result["items"].as_array().unwrap();
    assert_eq!(preview_items.len(), 51);
    assert_eq!(preview_items[50]["__ralphx_preview_truncated"], true);
    assert_eq!(preview_items[50]["__ralphx_preview_omitted_items"], 450);
    assert_eq!(
        preview.preview.as_ref().unwrap().paths,
        vec!["$.items[50:]".to_string()]
    );

    let non_text_value = json!(["plain", "array", 1, true]);
    assert!(!build_live_tool_result_preview(Some("bash"), &non_text_value, None).is_previewed());
    assert!(!build_live_tool_result_preview(None, &long_value, None).is_previewed());
}

#[test]
fn live_preview_payloads_mark_oversized_objects_and_fallbacks() {
    let capped_object = json!((0..85)
        .map(|index| (format!("field_{index:03}"), json!("value")))
        .collect::<serde_json::Map<String, serde_json::Value>>());
    let capped_preview = build_live_tool_result_preview(Some("bash"), &capped_object, None);

    assert!(capped_preview.is_previewed());
    assert_eq!(
        capped_preview.result["__ralphx_preview_truncated"],
        json!(true)
    );
    assert_eq!(capped_preview.result["__ralphx_preview_omitted_fields"], 5);
    assert_eq!(
        capped_preview.preview.as_ref().unwrap().paths,
        vec!["$.*".to_string()]
    );

    let shallow_object = json!((0..20)
        .map(|index| (format!("field_{index:03}"), json!(index)))
        .collect::<serde_json::Map<String, serde_json::Value>>());
    let fallback_preview = build_live_tool_result_preview(Some("bash"), &shallow_object, None);

    assert!(fallback_preview.is_previewed());
    assert_eq!(
        fallback_preview.result["__ralphx_preview_truncated"],
        json!(true)
    );
    assert!(fallback_preview.result["preview_text"]
        .as_str()
        .unwrap()
        .contains("field_000"));
    assert_eq!(
        fallback_preview.preview.as_ref().unwrap().paths,
        vec!["$".to_string()]
    );
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
    assert!(nested_preview.result["content"][1]["text"]
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
        run_id: Some("run-1".to_string()),
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
    assert_eq!(value["result_preview_paths"], json!(["$"]));
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
        None,
        "conv-1",
        "project",
        "project-1",
        Some("run-1"),
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
fn live_completed_edit_payload_previews_arguments_with_detail_ref() {
    let tool_call = ToolCall {
        id: Some("tool-edit".to_string()),
        name: "Edit".to_string(),
        arguments: json!({
            "file_path": "src/app.ts",
            "old_string": "export const value = 1;\nexport const label = \"old\";\n",
            "new_string": "export const value = 1;\nexport const label = \"new\";\n",
        }),
        result: None,
        parent_tool_use_id: None,
        diff_context: None,
        stats: None,
    };
    let argument_preview = build_live_tool_argument_preview(
        &tool_call,
        None,
        Some(tool_detail_ref("conv-1", "msg-1", Some("tool-edit"), None)),
    )
    .expect("edit arguments should preview");

    let payload = AgentToolCallPayload::from_completed_tool_call(
        &tool_call,
        None,
        Some(&argument_preview),
        "conv-1",
        "project",
        "project-1",
        None,
        None,
        None,
        10,
    );
    let value = serde_json::to_value(payload).unwrap();

    assert_eq!(value["tool_name"], "Edit");
    assert_eq!(value["arguments"]["file_path"], "src/app.ts");
    assert!(value["arguments"].get("old_string").is_none());
    assert!(value["arguments"].get("new_string").is_none());
    assert_eq!(value["arguments_preview_truncated"], true);
    assert_eq!(value["diff_preview"]["file_path"], "src/app.ts");
    assert_eq!(value["detail_ref"]["message_id"], "msg-1");
    assert_eq!(value["detail_ref"]["tool_call_id"], "tool-edit");
}

#[test]
fn live_completed_write_payload_previews_confirmed_new_file_as_added_diff() {
    let tool_call = ToolCall {
        id: Some("tool-write-new".to_string()),
        name: "write".to_string(),
        arguments: json!({
            "file_path": "src/new.rs",
            "content": "pub fn new() {}\n",
        }),
        result: None,
        parent_tool_use_id: None,
        diff_context: Some(DiffContext {
            old_content: None,
            old_file_exists: Some(false),
            file_path: "src/new.rs".to_string(),
        }),
        stats: None,
    };
    let diff_context = serde_json::to_value(tool_call.diff_context.as_ref().unwrap()).unwrap();
    let argument_preview = build_live_tool_argument_preview(
        &tool_call,
        Some(&diff_context),
        Some(tool_detail_ref(
            "conv-1",
            "msg-1",
            Some("tool-write-new"),
            None,
        )),
    )
    .expect("new-file write arguments should preview");

    let payload = AgentToolCallPayload::from_completed_tool_call(
        &tool_call,
        None,
        Some(&argument_preview),
        "conv-1",
        "project",
        "project-1",
        None,
        Some(diff_context),
        None,
        10,
    );
    let value = serde_json::to_value(payload).unwrap();

    assert_eq!(value["tool_name"], "write");
    assert_eq!(value["arguments"]["file_path"], "src/new.rs");
    assert!(value["arguments"].get("content").is_none());
    assert_eq!(value["diff_context"]["old_file_exists"], false);
    assert_eq!(value["diff_preview"]["old_total_lines"], 0);
    assert_eq!(value["diff_preview"]["new_total_lines"], 2);
    assert_eq!(
        value["diff_preview"]["hunks"][0]["lines"][0]["kind"],
        "addition"
    );
}

#[test]
fn live_tool_argument_preview_canonicalizes_tool_names_and_diff_languages() {
    let cases = [
        ("mcp__ralphx__edit", "src/app.js", "javascript"),
        ("mcp__ralphx_internal__edit", "src/lib.rs", "rust"),
        ("ralphx::edit", "src/style.css", "css"),
        ("ralphx_internal::edit", "src/index.html", "html"),
        ("ralphx:edit", "package.json", "json"),
        ("ralphx_internal:edit", "README.md", "markdown"),
        ("edit", "LICENSE", "text"),
    ];

    for (tool_name, file_path, expected_language) in cases {
        let tool_call = ToolCall {
            id: Some(format!("{tool_name}-{file_path}")),
            name: tool_name.to_string(),
            arguments: json!({
                "file_path": file_path,
                "old_string": "old\n",
                "new_string": "new\n",
            }),
            result: None,
            parent_tool_use_id: None,
            diff_context: None,
            stats: None,
        };
        let preview = build_live_tool_argument_preview(&tool_call, None, None)
            .expect("canonical edit argument preview");

        assert_eq!(preview.arguments, json!({ "file_path": file_path }));
        assert_eq!(
            preview
                .diff_preview
                .as_ref()
                .and_then(|diff| diff.get("language"))
                .and_then(serde_json::Value::as_str),
            Some(expected_language)
        );
    }
}

#[test]
fn live_tool_argument_preview_handles_unchanged_and_invalid_arguments() {
    let unchanged = ToolCall {
        id: Some("tool-edit-unchanged".to_string()),
        name: "edit".to_string(),
        arguments: json!({
            "file_path": "src/unchanged.ts",
            "old_string": "same\n",
            "new_string": "same\n",
        }),
        result: None,
        parent_tool_use_id: None,
        diff_context: None,
        stats: None,
    };
    let unchanged_preview = build_live_tool_argument_preview(&unchanged, None, None)
        .expect("unchanged edit arguments should still preview metadata");
    assert_eq!(
        unchanged_preview.diff_preview.as_ref().unwrap()["hunks"],
        json!([])
    );

    for arguments in [
        json!({ "old_string": "old", "new_string": "new" }),
        json!({ "file_path": "src/app.ts", "new_string": "new" }),
        json!({ "file_path": "src/app.ts", "old_string": "old" }),
    ] {
        let tool_call = ToolCall {
            id: Some("tool-edit-invalid".to_string()),
            name: "edit".to_string(),
            arguments,
            result: None,
            parent_tool_use_id: None,
            diff_context: None,
            stats: None,
        };
        assert!(build_live_tool_argument_preview(&tool_call, None, None).is_none());
    }

    for arguments in [
        json!({ "content": "body" }),
        json!({ "file_path": "src/app.ts" }),
    ] {
        let tool_call = ToolCall {
            id: Some("tool-write-invalid".to_string()),
            name: "write".to_string(),
            arguments,
            result: None,
            parent_tool_use_id: None,
            diff_context: None,
            stats: None,
        };
        assert!(build_live_tool_argument_preview(&tool_call, None, None).is_none());
    }
}

#[test]
fn live_tool_argument_preview_truncates_write_content_when_baseline_is_unknown() {
    let content = (1..=12)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let tool_call = ToolCall {
        id: Some("tool-write-large".to_string()),
        name: "write".to_string(),
        arguments: json!({
            "file_path": "src/generated.txt",
            "content": content,
        }),
        result: None,
        parent_tool_use_id: None,
        diff_context: None,
        stats: None,
    };
    let preview = build_live_tool_argument_preview(&tool_call, None, None)
        .expect("large final content should be previewed");

    let preview_content = preview.arguments["content"].as_str().unwrap();
    assert_eq!(preview_content.lines().count(), 10);
    assert!(preview_content.contains("line 10"));
    assert!(!preview_content.contains("line 11"));
    assert_eq!(preview.line_count, 12);
    assert_eq!(preview.omitted_lines, 2);
    assert!(preview.diff_preview.is_none());

    let long_single_line = ToolCall {
        id: Some("tool-write-long-line".to_string()),
        name: "write".to_string(),
        arguments: json!({
            "file_path": "src/generated.txt",
            "content": "x".repeat(4_100),
        }),
        result: None,
        parent_tool_use_id: None,
        diff_context: None,
        stats: None,
    };
    let long_line_preview = build_live_tool_argument_preview(&long_single_line, None, None)
        .expect("long final content should be character capped");
    assert_eq!(
        long_line_preview.arguments["content"]
            .as_str()
            .unwrap()
            .chars()
            .count(),
        4_000
    );
}

#[test]
fn live_tool_argument_preview_accepts_camel_case_new_file_context() {
    let tool_call = ToolCall {
        id: Some("tool-write-camel-context".to_string()),
        name: "write".to_string(),
        arguments: json!({
            "file_path": "src/new.ts",
            "content": "export const value = 1;\n",
        }),
        result: None,
        parent_tool_use_id: None,
        diff_context: None,
        stats: None,
    };
    let diff_context = json!({
        "filePath": "src/new.ts",
        "oldFileExists": false,
    });
    let preview = build_live_tool_argument_preview(&tool_call, Some(&diff_context), None)
        .expect("camel-case new-file context should produce an added diff");

    assert_eq!(preview.arguments, json!({ "file_path": "src/new.ts" }));
    assert_eq!(
        preview.diff_context.as_ref().unwrap()["oldFileExists"],
        false
    );
    assert_eq!(preview.diff_preview.as_ref().unwrap()["old_total_lines"], 0);
    assert_eq!(
        preview.diff_preview.as_ref().unwrap()["hunks"][0]["lines"][0]["kind"],
        "addition"
    );
}

#[test]
fn preview_tool_payloads_skips_already_previewed_or_unnamed_arguments() {
    let tool_calls = json!([
        {
            "id": "tool-write-previewed",
            "name": "write",
            "arguments": {
                "file_path": "src/generated.txt",
                "content": "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\nline 9\nline 10\nline 11"
            },
            "arguments_preview_truncated": true
        },
        {
            "id": "tool-write-unnamed",
            "arguments": {
                "file_path": "src/generated.txt",
                "content": "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\nline 9\nline 10\nline 11"
            }
        }
    ]);

    let (previewed_tool_calls, _) =
        preview_tool_payloads_for_message("conv-1", "msg-1", Some(tool_calls), None);
    let previewed_tool_calls = previewed_tool_calls.unwrap();

    assert!(previewed_tool_calls[0]["arguments"]["content"]
        .as_str()
        .unwrap()
        .contains("line 11"));
    assert!(previewed_tool_calls[0].get("detail_ref").is_none());
    assert!(previewed_tool_calls[1]["arguments"]["content"]
        .as_str()
        .unwrap()
        .contains("line 11"));
    assert!(previewed_tool_calls[1]
        .get("arguments_preview_truncated")
        .is_none());
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
        Some("run-1"),
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
        None::<std::path::PathBuf>,
        Some(app.handle().clone()),
        None,
        None,
        Some(Arc::new(MemoryChatMessageRepository::new())),
        None,
        Some("msg-live-preview".to_string()),
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
        None,
        None,
        None,
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
