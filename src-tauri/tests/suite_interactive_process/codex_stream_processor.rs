use ralphx_lib::infrastructure::agents::{
    extract_codex_agent_message, extract_codex_command_execution, extract_codex_error_message,
    extract_codex_thread_id, extract_codex_tool_call_snapshot, extract_codex_usage,
    parse_codex_event_line, CodexToolCallPhase,
};

#[test]
fn parse_codex_event_line_extracts_agent_message() {
    let event = parse_codex_event_line(
        r#"{
            "type":"item.completed",
            "item":{"id":"item_0","type":"agent_message","text":"Draft plan ready"}
        }"#,
    )
    .expect("event should parse");

    assert_eq!(
        extract_codex_agent_message(&event).as_deref(),
        Some("Draft plan ready")
    );
}

#[test]
fn extract_codex_thread_id_reads_thread_started_events() {
    let event = parse_codex_event_line(
        r#"{
            "type":"thread.started",
            "thread_id":"019d6078-21bc-73c1-a1cf-b415c8dab35b"
        }"#,
    )
    .expect("event should parse");

    assert_eq!(
        extract_codex_thread_id(&event).as_deref(),
        Some("019d6078-21bc-73c1-a1cf-b415c8dab35b")
    );
}

#[test]
fn extract_codex_usage_reads_provider_reported_reasoning_total() {
    let event = parse_codex_event_line(
        r#"{
            "type":"turn.completed",
            "usage":{
                "input_tokens":53565,
                "cached_input_tokens":30208,
                "output_tokens":558,
                "reasoning_output_tokens":170
            }
        }"#,
    )
    .expect("event should parse");

    let usage = extract_codex_usage(&event).expect("turn usage should extract");

    assert_eq!(usage.usage.reasoning_output_tokens, Some(170));
}

#[test]
fn extract_codex_tool_call_snapshot_maps_started_mcp_tool_calls() {
    let event = parse_codex_event_line(
        r#"{
            "type":"item.started",
            "item":{
                "id":"item_1",
                "type":"mcp_tool_call",
                "server":"ralphx",
                "tool":"respond",
                "arguments":{"response":"ok"},
                "status":"in_progress"
            }
        }"#,
    )
    .expect("event should parse");

    let snapshot =
        extract_codex_tool_call_snapshot(&event).expect("tool call snapshot should extract");

    assert_eq!(snapshot.phase, CodexToolCallPhase::Started);
    assert_eq!(snapshot.tool_call.id.as_deref(), Some("item_1"));
    assert_eq!(snapshot.tool_call.name, "ralphx::respond");
    assert_eq!(snapshot.tool_call.arguments["response"], "ok");
    assert_eq!(snapshot.tool_call.result, None);
}

#[test]
fn extract_codex_tool_call_snapshot_maps_completed_mcp_tool_calls() {
    let event = parse_codex_event_line(
        r#"{
            "type":"item.completed",
            "item":{
                "id":"item_1",
                "type":"mcp_tool_call",
                "server":"ralphx",
                "tool":"respond",
                "arguments":{"response":"ok"},
                "result":{"content":[{"type":"text","text":"{\"recorded\":true}"}]},
                "status":"completed"
            }
        }"#,
    )
    .expect("event should parse");

    let snapshot =
        extract_codex_tool_call_snapshot(&event).expect("tool call snapshot should extract");

    assert_eq!(snapshot.phase, CodexToolCallPhase::Completed);
    assert_eq!(snapshot.tool_call.id.as_deref(), Some("item_1"));
    assert_eq!(snapshot.tool_call.name, "ralphx::respond");
    assert_eq!(snapshot.tool_call.arguments["response"], "ok");
    assert_eq!(
        snapshot.tool_call.result.expect("result should exist")["content"][0]["type"],
        "text"
    );
}

#[test]
fn extract_codex_tool_call_snapshot_ignores_non_lifecycle_events() {
    let event = parse_codex_event_line(
        r#"{
            "type":"item.delta",
            "item":{
                "id":"item_1",
                "type":"mcp_tool_call",
                "server":"ralphx",
                "tool":"respond",
                "arguments":{"response":"ok"}
            }
        }"#,
    )
    .expect("event should parse");

    assert!(
        extract_codex_tool_call_snapshot(&event).is_none(),
        "only started/completed should produce tool snapshots"
    );
}

#[test]
fn extract_codex_command_execution_captures_output_and_exit_code() {
    let event = parse_codex_event_line(
        r#"{
            "type":"item.completed",
            "item":{
                "id":"item_2",
                "type":"command_execution",
                "status":"completed",
                "aggregated_output":"cargo test ok",
                "exit_code":0
            }
        }"#,
    )
    .expect("event should parse");

    let execution =
        extract_codex_command_execution(&event).expect("command execution should extract");

    assert_eq!(execution.id.as_deref(), Some("item_2"));
    assert_eq!(execution.status.as_deref(), Some("completed"));
    assert_eq!(
        execution.aggregated_output.as_deref(),
        Some("cargo test ok")
    );
    assert_eq!(execution.exit_code, Some(0));
}

#[test]
fn extract_codex_error_message_reads_tool_call_and_error_items() {
    let tool_error = parse_codex_event_line(
        r#"{
            "type":"item.completed",
            "item":{
                "id":"item_3",
                "type":"mcp_tool_call",
                "error":{"message":"backend failure"}
            }
        }"#,
    )
    .expect("event should parse");
    let terminal_error = parse_codex_event_line(
        r#"{
            "type":"item.completed",
            "item":{
                "id":"item_4",
                "type":"error",
                "error":{"message":"command crashed"}
            }
        }"#,
    )
    .expect("event should parse");

    assert_eq!(
        extract_codex_error_message(&tool_error).as_deref(),
        Some("backend failure")
    );
    assert_eq!(
        extract_codex_error_message(&terminal_error).as_deref(),
        Some("command crashed")
    );
}

#[test]
fn extract_codex_usage_reads_turn_completed_usage() {
    let event = parse_codex_event_line(
        r#"{
            "type":"turn.completed",
            "usage":{
                "input_tokens":1200,
                "cached_input_tokens":300,
                "output_tokens":450
            }
        }"#,
    )
    .expect("event should parse");

    let usage = extract_codex_usage(&event).expect("usage should extract");

    assert_eq!(usage.usage.input_tokens, Some(1200));
    assert_eq!(usage.usage.cached_input_tokens, Some(300));
    assert_eq!(usage.usage.output_tokens, Some(450));
}
