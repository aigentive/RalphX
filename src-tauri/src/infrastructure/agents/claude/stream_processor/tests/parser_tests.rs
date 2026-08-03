use super::*;

#[test]
fn parse_line_unwraps_stream_event_text_delta() {
    let line = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hi"}},"session_id":"sess-1","uuid":"u-1"}"#;

    let parsed = StreamProcessor::parse_line(line).expect("wrapped text delta should parse");
    let StreamMessage::ContentBlockDelta { delta, .. } = parsed.message else {
        panic!("expected ContentBlockDelta");
    };
    assert_eq!(delta.text.as_deref(), Some("Hi"));
}

#[test]
fn parse_line_unwraps_stream_event_thinking_delta() {
    let line = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Consider this"}},"session_id":"sess-1","uuid":"u-1"}"#;

    let parsed = StreamProcessor::parse_line(line).expect("wrapped thinking delta should parse");
    let StreamMessage::ContentBlockDelta { delta, .. } = parsed.message else {
        panic!("expected ContentBlockDelta");
    };
    assert_eq!(delta.delta_type, "thinking_delta");
    assert_eq!(delta.text.as_deref(), Some("Consider this"));
}

#[test]
fn parse_and_process_thinking_tokens_progress() {
    let line = r#"{"type":"system","subtype":"thinking_tokens","estimated_tokens":95,"estimated_tokens_delta":45}"#;
    let parsed = StreamProcessor::parse_line(line).expect("thinking token progress should parse");
    let StreamMessage::System {
        estimated_tokens,
        estimated_tokens_delta,
        ..
    } = &parsed.message
    else {
        panic!("expected system message");
    };
    assert_eq!(*estimated_tokens, Some(95));
    assert_eq!(*estimated_tokens_delta, Some(45));

    let events = StreamProcessor::new().process_parsed_line(parsed);
    assert!(matches!(
        events.as_slice(),
        [StreamEvent::ThinkingProgress {
            estimated_tokens: 95,
            estimated_tokens_delta: Some(45),
        }]
    ));
}

#[test]
fn parse_line_unwraps_stream_event_content_block_start_stop() {
    let start = r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu-1","name":"Read"}}}"#;
    let stop = r#"{"type":"stream_event","event":{"type":"content_block_stop","index":0}}"#;

    let start = StreamProcessor::parse_line(start).expect("wrapped block start should parse");
    let stop = StreamProcessor::parse_line(stop).expect("wrapped block stop should parse");
    assert!(matches!(
        start.message,
        StreamMessage::ContentBlockStart { .. }
    ));
    assert!(matches!(
        stop.message,
        StreamMessage::ContentBlockStop { .. }
    ));
}

#[test]
fn parse_line_preserves_parent_tool_use_id_from_stream_event_envelope() {
    let line = r#"{"type":"stream_event","parent_tool_use_id":"toolu-parent","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hi"}}}"#;

    let parsed = StreamProcessor::parse_line(line).expect("wrapped delta should parse");
    assert_eq!(parsed.parent_tool_use_id.as_deref(), Some("toolu-parent"));
}

#[test]
fn test_parse_text_delta() {
    let line = r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"Hello"}}"#;
    let parsed = StreamProcessor::parse_line(line);

    let parsed = parsed.expect("Expected Some(ParsedLine)");
    assert!(parsed.parent_tool_use_id.is_none());
    assert!(
        matches!(parsed.message, StreamMessage::ContentBlockDelta { .. }),
        "Expected ContentBlockDelta, got different variant"
    );
    let StreamMessage::ContentBlockDelta { delta, .. } = parsed.message else {
        unreachable!()
    };
    assert_eq!(delta.delta_type, "text_delta");
    assert_eq!(delta.text, Some("Hello".to_string()));
}

#[test]
fn test_parse_line_with_data_prefix() {
    let line = r#"data: {"type":"content_block_delta","delta":{"type":"text_delta","text":"Hi"}}"#;
    let parsed = StreamProcessor::parse_line(line);

    let parsed = parsed.expect("Expected Some(ParsedLine)");
    assert!(matches!(
        parsed.message,
        StreamMessage::ContentBlockDelta { .. }
    ));
}

#[test]
fn test_parse_tool_use_start() {
    let line = r#"{"type":"content_block_start","content_block":{"type":"tool_use","id":"toolu_123","name":"create_task_proposal"}}"#;
    let parsed = StreamProcessor::parse_line(line);

    let parsed = parsed.expect("Expected Some(ParsedLine)");
    assert!(
        matches!(parsed.message, StreamMessage::ContentBlockStart { .. }),
        "Expected ContentBlockStart, got different variant"
    );
    let StreamMessage::ContentBlockStart { content_block, .. } = parsed.message else {
        unreachable!()
    };
    assert_eq!(content_block.block_type, "tool_use");
    assert_eq!(content_block.name, Some("create_task_proposal".to_string()));
    assert_eq!(content_block.id, Some("toolu_123".to_string()));
}

#[test]
fn test_parse_result() {
    let line = r#"{"type":"result","session_id":"550e8400-e29b-41d4-a716-446655440000","result":"Done","is_error":false,"cost_usd":0.05}"#;
    let parsed = StreamProcessor::parse_line(line);

    let parsed = parsed.expect("Expected Some(ParsedLine)");
    assert!(
        matches!(parsed.message, StreamMessage::Result { .. }),
        "Expected Result, got different variant"
    );
    let StreamMessage::Result { session_id, .. } = parsed.message else {
        unreachable!()
    };
    assert_eq!(
        session_id,
        Some("550e8400-e29b-41d4-a716-446655440000".to_string())
    );
}

#[test]
fn claude_result_usage_deserializes_terminal_cache_aliases() {
    let line = r#"{"type":"result","session_id":"usage-result","result":"Done","is_error":false,"usage":{"input_tokens":13,"output_tokens":1434,"cache_creation_input_tokens":127826,"cache_read_input_tokens":1099251}}"#;
    let parsed = StreamProcessor::parse_line(line).expect("result should parse");

    let StreamMessage::Result { usage, .. } = parsed.message else {
        panic!("expected result message");
    };
    let usage = usage.expect("terminal result usage");
    assert_eq!(usage.input_tokens, Some(13));
    assert_eq!(usage.output_tokens, Some(1_434));
    assert_eq!(usage.cache_creation_tokens, Some(127_826));
    assert_eq!(usage.cache_read_tokens, Some(1_099_251));
}

#[test]
fn test_parse_assistant_message() {
    let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Hello world"}],"stop_reason":"end_turn","usage":{"input_tokens":123,"output_tokens":45,"cache_read_input_tokens":67}},"session_id":"sess-123"}"#;
    let parsed = StreamProcessor::parse_line(line);

    let parsed = parsed.expect("Expected Some(ParsedLine)");
    assert!(
        matches!(parsed.message, StreamMessage::Assistant { .. }),
        "Expected Assistant message, got different variant"
    );
    let StreamMessage::Assistant {
        message,
        session_id,
    } = parsed.message
    else {
        unreachable!()
    };
    assert_eq!(session_id, Some("sess-123".to_string()));
    assert_eq!(message.content.len(), 1);
    assert_eq!(
        message
            .usage
            .as_ref()
            .and_then(|usage| usage.get("input_tokens"))
            .and_then(|value| value.as_u64()),
        Some(123)
    );
    assert!(
        matches!(&message.content[0], AssistantContent::Text { .. }),
        "Expected Text content, got different variant"
    );
    let AssistantContent::Text { text } = &message.content[0] else {
        unreachable!()
    };
    assert_eq!(text, "Hello world");
}

#[test]
fn test_parse_thinking_content() {
    let line = r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"Let me think..."}],"stop_reason":"end_turn"},"session_id":"sess-789"}"#;
    let parsed = StreamProcessor::parse_line(line);

    let parsed = parsed.expect("Expected Some(ParsedLine)");
    assert!(
        matches!(parsed.message, StreamMessage::Assistant { .. }),
        "Expected Assistant message, got different variant"
    );
    let StreamMessage::Assistant { message, .. } = parsed.message else {
        unreachable!()
    };
    assert_eq!(message.content.len(), 1);
    assert!(
        matches!(&message.content[0], AssistantContent::Thinking { .. }),
        "Expected Thinking content, got different variant"
    );
    let AssistantContent::Thinking { thinking } = &message.content[0] else {
        unreachable!()
    };
    assert_eq!(thinking, "Let me think...");
}

#[test]
fn test_parse_line_extracts_parent_tool_use_id() {
    let line = r#"{"type":"assistant","parent_tool_use_id":"toolu_01CdYLhs","message":{"content":[{"type":"text","text":"subagent text"}],"stop_reason":"end_turn"}}"#;
    let parsed = StreamProcessor::parse_line(line).expect("Expected Some(ParsedLine)");

    assert_eq!(
        parsed.parent_tool_use_id,
        Some("toolu_01CdYLhs".to_string())
    );
    assert!(matches!(parsed.message, StreamMessage::Assistant { .. }));
}

#[test]
fn test_parse_line_null_parent_tool_use_id() {
    let line = r#"{"type":"assistant","parent_tool_use_id":null,"message":{"content":[{"type":"text","text":"parent text"}],"stop_reason":"end_turn"}}"#;
    let parsed = StreamProcessor::parse_line(line).expect("Expected Some(ParsedLine)");

    assert!(parsed.parent_tool_use_id.is_none());
}

#[test]
fn test_parse_line_extracts_is_synthetic() {
    // Synthetic user message
    let line = r#"{"type":"user","isSynthetic":true,"message":{"content":[{"type":"text","text":"Hook blocked"}]}}"#;
    let parsed = StreamProcessor::parse_line(line).expect("Expected Some(ParsedLine)");
    assert!(parsed.is_synthetic);

    // Non-synthetic message (no isSynthetic field)
    let line2 =
        r#"{"type":"user","message":{"content":[{"type":"text","text":"Normal message"}]}}"#;
    let parsed2 = StreamProcessor::parse_line(line2).expect("Expected Some(ParsedLine)");
    assert!(!parsed2.is_synthetic);

    // Explicit isSynthetic: false
    let line3 = r#"{"type":"user","isSynthetic":false,"message":{"content":[{"type":"text","text":"Not synthetic"}]}}"#;
    let parsed3 = StreamProcessor::parse_line(line3).expect("Expected Some(ParsedLine)");
    assert!(!parsed3.is_synthetic);
}

#[test]
fn test_parse_system_hook_started_json() {
    let line = r#"{"type":"system","subtype":"hook_started","hook_id":"h1","hook_name":"audit.sh","hook_event":"SessionStart","message":"Starting hook"}"#;
    let parsed = StreamProcessor::parse_line(line).expect("Expected Some(ParsedLine)");
    assert!(matches!(parsed.message, StreamMessage::System { .. }));

    let StreamMessage::System {
        subtype,
        hook_id,
        hook_name,
        hook_event,
        ..
    } = parsed.message
    else {
        unreachable!()
    };
    assert_eq!(subtype, Some("hook_started".to_string()));
    assert_eq!(hook_id, Some("h1".to_string()));
    assert_eq!(hook_name, Some("audit.sh".to_string()));
    assert_eq!(hook_event, Some("SessionStart".to_string()));
}

#[test]
fn test_parse_system_hook_response_json() {
    let line = r#"{"type":"system","subtype":"hook_response","hook_id":"h2","hook_name":"lint.sh","hook_event":"PostToolUse","output":"All clean","exit_code":0,"outcome":"success"}"#;
    let parsed = StreamProcessor::parse_line(line).expect("Expected Some(ParsedLine)");

    let StreamMessage::System {
        subtype,
        hook_id,
        hook_name,
        hook_event,
        output,
        exit_code,
        outcome,
        ..
    } = parsed.message
    else {
        unreachable!()
    };
    assert_eq!(subtype, Some("hook_response".to_string()));
    assert_eq!(hook_id, Some("h2".to_string()));
    assert_eq!(hook_name, Some("lint.sh".to_string()));
    assert_eq!(hook_event, Some("PostToolUse".to_string()));
    assert_eq!(output, Some("All clean".to_string()));
    assert_eq!(exit_code, Some(0));
    assert_eq!(outcome, Some("success".to_string()));
}
