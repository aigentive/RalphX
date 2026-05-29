use super::*;

#[test]
fn test_tool_call_serialization() {
    let tool_call = ToolCall {
        id: Some("toolu_01ABC".to_string()),
        name: "create_task_proposal".to_string(),
        arguments: serde_json::json!({"title": "Test task"}),
        result: None,
        parent_tool_use_id: Some("toolu_parent_123".to_string()),
        diff_context: None,
        stats: None,
    };

    let json = serde_json::to_string(&tool_call).unwrap();
    assert!(json.contains("toolu_01ABC"));
    assert!(json.contains("create_task_proposal"));
    assert!(json.contains("parent_tool_use_id"));
    // diff_context: None should be skipped via skip_serializing_if
    assert!(!json.contains("diff_context"));

    let parsed: ToolCall = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.name, "create_task_proposal");
    assert_eq!(parsed.parent_tool_use_id.as_deref(), Some("toolu_parent_123"));
    assert!(parsed.diff_context.is_none());
}

#[test]
fn test_tool_call_with_diff_context_serialization() {
    let tool_call = ToolCall {
        id: Some("toolu_02DEF".to_string()),
        name: "Edit".to_string(),
        arguments: serde_json::json!({"file_path": "src/main.rs", "old_string": "old", "new_string": "new"}),
        result: None,
        parent_tool_use_id: None,
        diff_context: Some(DiffContext {
            old_content: Some("fn main() {\n    old\n}\n".to_string()),
            old_file_exists: None,
            file_path: "/project/src/main.rs".to_string(),
        }),
        stats: None,
    };

    let json = serde_json::to_string(&tool_call).unwrap();
    assert!(json.contains("diff_context"));
    assert!(json.contains("old_content"));
    assert!(json.contains("/project/src/main.rs"));

    let parsed: ToolCall = serde_json::from_str(&json).unwrap();
    assert!(parsed.diff_context.is_some());
    let ctx = parsed.diff_context.unwrap();
    assert_eq!(ctx.file_path, "/project/src/main.rs");
    assert!(ctx.old_content.is_some());
}

#[test]
fn test_tool_call_diff_context_new_file() {
    let tool_call = ToolCall {
        id: Some("toolu_03GHI".to_string()),
        name: "Write".to_string(),
        arguments: serde_json::json!({"file_path": "src/new.rs", "content": "fn new() {}"}),
        result: None,
        parent_tool_use_id: None,
        diff_context: Some(DiffContext {
            old_content: None,
            old_file_exists: Some(false),
            file_path: "/project/src/new.rs".to_string(),
        }),
        stats: None,
    };

    let json = serde_json::to_string(&tool_call).unwrap();
    let parsed: ToolCall = serde_json::from_str(&json).unwrap();
    let ctx = parsed.diff_context.unwrap();
    assert!(ctx.old_content.is_none());
    assert_eq!(ctx.old_file_exists, Some(false));
    assert_eq!(ctx.file_path, "/project/src/new.rs");
}

#[test]
fn test_parse_usage_text_basic() {
    let text = "Some output\nagentId: a7db0f4 (for resuming...)\n<usage>total_tokens: 12345\ntool_uses: 8\nduration_ms: 45000</usage>";
    let (agent_id, duration, tokens, tools) = parse_usage_text(text);

    assert_eq!(agent_id, Some("a7db0f4".to_string()));
    assert_eq!(duration, Some(45000));
    assert_eq!(tokens, Some(12345));
    assert_eq!(tools, Some(8));
}

#[test]
fn test_parse_usage_text_no_usage_block() {
    let text = "Just some plain text output\nagentId: abc123";
    let (agent_id, duration, tokens, tools) = parse_usage_text(text);

    assert_eq!(agent_id, Some("abc123".to_string()));
    assert_eq!(duration, None);
    assert_eq!(tokens, None);
    assert_eq!(tools, None);
}

#[test]
fn test_parse_usage_text_no_agent_id() {
    let text = "<usage>total_tokens: 500\ntool_uses: 2\nduration_ms: 3000</usage>";
    let (agent_id, duration, tokens, tools) = parse_usage_text(text);

    assert_eq!(agent_id, None);
    assert_eq!(duration, Some(3000));
    assert_eq!(tokens, Some(500));
    assert_eq!(tools, Some(2));
}

#[test]
fn test_value_to_text_string() {
    let val = serde_json::json!("plain text result");
    assert_eq!(value_to_text(&val), "plain text result");
}

#[test]
fn test_value_to_text_content_blocks() {
    let val = serde_json::json!([
        {"type": "text", "text": "output line 1"},
        {"type": "tool_use", "id": "t1", "name": "Read"},
        {"type": "text", "text": "agentId: abc\n<usage>total_tokens: 100\ntool_uses: 1\nduration_ms: 2000</usage>"}
    ]);
    let text = value_to_text(&val);
    assert!(text.contains("output line 1"));
    assert!(text.contains("<usage>"));
    assert!(text.contains("agentId: abc"));
}

// ============================================================================
// ToolCallStats serialization tests
// ============================================================================

#[test]
fn test_tool_call_stats_serialized_as_camel_case() {
    let tool_call = ToolCall {
        id: Some("toolu_stats1".to_string()),
        name: "Task".to_string(),
        arguments: serde_json::json!({"prompt": "do something"}),
        result: Some(serde_json::json!("task output")),
        parent_tool_use_id: None,
        diff_context: None,
        stats: Some(ToolCallStats {
            model: Some("claude-sonnet-4-6".to_string()),
            total_tokens: Some(12345),
            total_tool_uses: Some(8),
            duration_ms: Some(45000),
        }),
    };

    let json = serde_json::to_string(&tool_call).unwrap();

    // Verify field is present
    assert!(json.contains("\"stats\""), "stats field should be present");
    // Verify camelCase serialization (not snake_case)
    assert!(json.contains("\"totalTokens\""), "should use camelCase totalTokens");
    assert!(json.contains("\"totalToolUses\""), "should use camelCase totalToolUses");
    assert!(json.contains("\"durationMs\""), "should use camelCase durationMs");
    // Verify snake_case is NOT used
    assert!(!json.contains("\"total_tokens\""), "should NOT use snake_case total_tokens");
    assert!(!json.contains("\"total_tool_uses\""), "should NOT use snake_case total_tool_uses");
    assert!(!json.contains("\"duration_ms\""), "should NOT use snake_case duration_ms");
    // Verify values
    assert!(json.contains("12345"));
    assert!(json.contains("45000"));

    // Round-trip: deserialized stats should match
    let parsed: ToolCall = serde_json::from_str(&json).unwrap();
    let stats = parsed.stats.expect("stats should round-trip");
    assert_eq!(stats.model, Some("claude-sonnet-4-6".to_string()));
    assert_eq!(stats.total_tokens, Some(12345));
    assert_eq!(stats.total_tool_uses, Some(8));
    assert_eq!(stats.duration_ms, Some(45000));
}

#[test]
fn test_tool_call_without_stats_field_is_absent() {
    let tool_call = ToolCall {
        id: Some("toolu_nostats".to_string()),
        name: "Read".to_string(),
        arguments: serde_json::json!({"file_path": "src/main.rs"}),
        result: Some(serde_json::json!("file contents")),
        parent_tool_use_id: None,
        diff_context: None,
        stats: None,
    };

    let json = serde_json::to_string(&tool_call).unwrap();

    // Field must be absent (not present as null) — ensures old rows remain compatible
    assert!(!json.contains("\"stats\""), "stats field should be absent when None");

    // Deserializing old JSON (no stats key) should yield stats: None
    let old_json = r#"{"id":"toolu_old","name":"Read","arguments":{},"result":"output"}"#;
    let old: ToolCall = serde_json::from_str(old_json).unwrap();
    assert!(old.stats.is_none(), "old rows without stats key should deserialize to None");
    assert!(old.parent_tool_use_id.is_none());
}

#[test]
fn test_task_completed_injects_stats_into_tool_call() {
    // Integration test: stream a Task tool call + result through StreamProcessor,
    // then verify stats are present in processor.tool_calls after TaskCompleted.
    let mut processor = StreamProcessor::new();

    // Register the Task tool_use
    processor.process_message(StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![AssistantContent::ToolUse {
                id: "toolu_integ1".to_string(),
                name: "Task".to_string(),
                input: serde_json::json!({
                    "description": "Run integration test",
                    "subagent_type": "general-purpose"
                }),
            }],
            stop_reason: None,
            usage: None,
        },
        session_id: None,
    });

    // Deliver the tool result with structured metadata (JSON path)
    processor.process_message(StreamMessage::User {
        message: UserMessage {
            content: vec![UserContent::ToolResult {
                tool_use_id: "toolu_integ1".to_string(),
                content: serde_json::json!({
                    "tool_use_result": {
                        "agentId": "integ-agent-001",
                        "totalDurationMs": 30000_u64,
                        "totalTokens": 9876_u64,
                        "totalToolUseCount": 5_u64
                    }
                }),
                is_error: false,
            }],
        },
    });

    // Verify stats were injected into the ToolCall struct
    let tool_call = processor
        .tool_calls
        .iter()
        .find(|tc| tc.id.as_deref() == Some("toolu_integ1"))
        .expect("Tool call toolu_integ1 should be in processor.tool_calls");

    let stats = tool_call
        .stats
        .as_ref()
        .expect("Stats should be injected into ToolCall after TaskCompleted");

    assert_eq!(stats.total_tokens, Some(9876));
    assert_eq!(stats.total_tool_uses, Some(5));
    assert_eq!(stats.duration_ms, Some(30000));

    // Verify serialization produces the camelCase JSON that will be stored in DB
    let json = serde_json::to_value(tool_call).unwrap();
    let stats_json = &json["stats"];
    assert!(stats_json.is_object(), "stats should serialize as an object");
    assert_eq!(stats_json["totalTokens"], 9876);
    assert_eq!(stats_json["totalToolUses"], 5);
    assert_eq!(stats_json["durationMs"], 30000);
}
