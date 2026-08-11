use super::*;

#[test]
fn test_processor_tool_call() {
    let mut processor = StreamProcessor::new();

    // Tool call start
    let start = StreamMessage::ContentBlockStart {
        index: Some(0),
        content_block: ContentBlock {
            block_type: "tool_use".to_string(),
            id: Some("toolu_123".to_string()),
            name: Some("create_task".to_string()),
            text: None,
            input: None,
        },
    };

    // Tool call input delta
    let delta = StreamMessage::ContentBlockDelta {
        index: Some(0),
        delta: ContentDelta {
            delta_type: "input_json_delta".to_string(),
            text: None,
            partial_json: Some(r#"{"title":"Test"}"#.to_string()),
        },
    };

    // Tool call stop
    let stop = StreamMessage::ContentBlockStop { index: Some(0) };

    let events1 = processor.process_message(start);
    let events2 = processor.process_message(delta);
    let events3 = processor.process_message(stop);

    assert!(matches!(events1[0], StreamEvent::ToolCallStarted { .. }));
    assert!(events2.is_empty()); // input_json_delta doesn't emit events
    assert!(matches!(events3[0], StreamEvent::ToolCallCompleted { .. }));

    let result = processor.finish();
    assert_eq!(result.tool_calls.len(), 1);
    assert_eq!(result.tool_calls[0].name, "create_task");
    assert_eq!(result.tool_calls[0].id, Some("toolu_123".to_string()));
}

#[test]
fn test_processor_tool_result() {
    let mut processor = StreamProcessor::new();

    // First, send an assistant message with a tool use
    let assistant_msg = StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![AssistantContent::ToolUse {
                id: "toolu_789".to_string(),
                name: "bash".to_string(),
                input: serde_json::json!({"command": "pwd"}),
            }],
            stop_reason: None,
            usage: None,
        },
        session_id: None,
    };

    let events1 = processor.process_message(assistant_msg);
    assert_eq!(events1.len(), 1);
    assert!(
        matches!(&events1[0], StreamEvent::ToolCallCompleted { ref tool_call, .. } if tool_call.name == "bash")
    );

    // Verify tool call is stored with no result
    assert_eq!(processor.tool_calls.len(), 1);
    assert!(processor.tool_calls[0].result.is_none());

    // Now send a user message with tool result
    let user_msg = StreamMessage::User {
        message: UserMessage {
            content: vec![UserContent::ToolResult {
                tool_use_id: "toolu_789".to_string(),
                content: serde_json::json!("/Users/test/project"),
                is_error: false,
            }],
        },
    };

    let events2 = processor.process_message(user_msg);
    assert_eq!(events2.len(), 1);
    assert!(matches!(
        &events2[0],
        StreamEvent::ToolResultReceived { tool_use_id, .. } if tool_use_id == "toolu_789"
    ));

    // Verify tool call now has result
    let result = processor.finish();
    assert_eq!(result.tool_calls.len(), 1);
    assert_eq!(result.tool_calls[0].name, "bash");
    assert!(result.tool_calls[0].result.is_some());
    assert_eq!(
        result.tool_calls[0].result,
        Some(serde_json::json!("/Users/test/project"))
    );
}

/// Regression test: teammate Result events must NOT overwrite the lead's session_id.
#[test]
fn test_result_session_id_ignored_when_parent_tool_use_id_set() {
    let mut processor = StreamProcessor::new();

    // Teammate result event: has a parent_tool_use_id → session_id must be ignored
    let teammate_result = ParsedLine {
        message: StreamMessage::Result {
            result: Some("teammate done".to_string()),
            session_id: Some("teammate-session-id".to_string()),
            is_error: false,
            errors: Vec::new(),
            subtype: None,
            cost_usd: 0.001,
            usage: None,
        },
        parent_tool_use_id: Some("toolu_task_abc".to_string()),
        is_synthetic: false,
        tool_use_result: None,
    };

    let events = processor.process_parsed_line(teammate_result);

    // No SessionId event should be emitted for a teammate result
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, StreamEvent::SessionId(_))),
        "Expected no SessionId event from teammate result, but got one"
    );
    assert!(
        processor.session_id.is_none(),
        "session_id must not be set from teammate result event"
    );

    // Lead result event: no parent_tool_use_id → session_id must be captured
    let lead_result = ParsedLine {
        message: StreamMessage::Result {
            result: Some("lead done".to_string()),
            session_id: Some("lead-session-id".to_string()),
            is_error: false,
            errors: Vec::new(),
            subtype: None,
            cost_usd: 0.05,
            usage: None,
        },
        parent_tool_use_id: None,
        is_synthetic: false,
        tool_use_result: None,
    };

    let events = processor.process_parsed_line(lead_result);

    // SessionId event must be emitted for the lead result
    assert!(
        events
            .iter()
            .any(|e| matches!(e, StreamEvent::SessionId(id) if id == "lead-session-id")),
        "Expected SessionId(\"lead-session-id\") event from lead result"
    );
    assert_eq!(
        processor.session_id,
        Some("lead-session-id".to_string()),
        "session_id must be set from lead result event"
    );
}

/// Regression test: teammate Assistant messages must NOT overwrite the lead's session_id.
#[test]
fn test_assistant_session_id_ignored_when_parent_tool_use_id_set() {
    let mut processor = StreamProcessor::new();

    // Teammate assistant message: has a parent_tool_use_id → session_id must be ignored
    let teammate_msg = ParsedLine {
        message: StreamMessage::Assistant {
            message: AssistantMessage {
                content: vec![AssistantContent::Text {
                    text: "teammate response".to_string(),
                }],
                stop_reason: Some("end_turn".to_string()),
                usage: None,
            },
            session_id: Some("teammate-assistant-session".to_string()),
        },
        parent_tool_use_id: Some("toolu_task_xyz".to_string()),
        is_synthetic: false,
        tool_use_result: None,
    };

    let events = processor.process_parsed_line(teammate_msg);

    assert!(
        !events
            .iter()
            .any(|e| matches!(e, StreamEvent::SessionId(_))),
        "Expected no SessionId event from teammate assistant message, but got one"
    );
    assert!(
        processor.session_id.is_none(),
        "session_id must not be set from teammate assistant message"
    );

    // Lead assistant message: no parent_tool_use_id → session_id must be captured
    let lead_msg = ParsedLine {
        message: StreamMessage::Assistant {
            message: AssistantMessage {
                content: vec![AssistantContent::Text {
                    text: "lead response".to_string(),
                }],
                stop_reason: Some("end_turn".to_string()),
                usage: None,
            },
            session_id: Some("lead-assistant-session".to_string()),
        },
        parent_tool_use_id: None,
        is_synthetic: false,
        tool_use_result: None,
    };

    let events = processor.process_parsed_line(lead_msg);

    assert!(
        events
            .iter()
            .any(|e| matches!(e, StreamEvent::SessionId(id) if id == "lead-assistant-session")),
        "Expected SessionId(\"lead-assistant-session\") event from lead assistant message"
    );
    assert_eq!(
        processor.session_id,
        Some("lead-assistant-session".to_string()),
        "session_id must be set from lead assistant message"
    );
}

#[test]
fn test_parent_tool_use_id_propagates_to_tool_call_started() {
    let mut processor = StreamProcessor::new();

    let parsed = ParsedLine {
        message: StreamMessage::ContentBlockStart {
            index: Some(0),
            content_block: ContentBlock {
                block_type: "tool_use".to_string(),
                id: Some("toolu_sub1".to_string()),
                name: Some("Grep".to_string()),
                text: None,
                input: None,
            },
        },
        parent_tool_use_id: Some("toolu_parent".to_string()),
        is_synthetic: false,
        tool_use_result: None,
    };

    let events = processor.process_parsed_line(parsed);
    assert_eq!(events.len(), 1);
    match &events[0] {
        StreamEvent::ToolCallStarted {
            name,
            id,
            parent_tool_use_id,
        } => {
            assert_eq!(name, "Grep");
            assert_eq!(id, &Some("toolu_sub1".to_string()));
            assert_eq!(parent_tool_use_id, &Some("toolu_parent".to_string()));
        }
        other => panic!("Expected ToolCallStarted, got {:?}", other),
    }
}

#[test]
fn test_parent_tool_use_id_propagates_to_tool_call_completed() {
    let mut processor = StreamProcessor::new();

    // Start tool call
    processor.process_message(StreamMessage::ContentBlockStart {
        index: Some(0),
        content_block: ContentBlock {
            block_type: "tool_use".to_string(),
            id: Some("toolu_sub2".to_string()),
            name: Some("Read".to_string()),
            text: None,
            input: None,
        },
    });

    // Delta
    processor.process_message(StreamMessage::ContentBlockDelta {
        index: Some(0),
        delta: ContentDelta {
            delta_type: "input_json_delta".to_string(),
            text: None,
            partial_json: Some(r#"{"file":"test.rs"}"#.to_string()),
        },
    });

    // Stop with parent_tool_use_id
    let parsed = ParsedLine {
        message: StreamMessage::ContentBlockStop { index: Some(0) },
        parent_tool_use_id: Some("toolu_parent".to_string()),
        is_synthetic: false,
        tool_use_result: None,
    };

    let events = processor.process_parsed_line(parsed);
    assert_eq!(events.len(), 1);
    match &events[0] {
        StreamEvent::ToolCallCompleted {
            tool_call,
            parent_tool_use_id,
        } => {
            assert_eq!(tool_call.name, "Read");
            assert_eq!(parent_tool_use_id, &Some("toolu_parent".to_string()));
        }
        other => panic!("Expected ToolCallCompleted, got {:?}", other),
    }
}

#[test]
fn test_parent_tool_use_id_propagates_to_tool_result() {
    let mut processor = StreamProcessor::new();

    // Register a tool call
    processor.process_message(StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![AssistantContent::ToolUse {
                id: "toolu_sub_result".to_string(),
                name: "Grep".to_string(),
                input: serde_json::json!({"pattern": "foo"}),
            }],
            stop_reason: None,
            usage: None,
        },
        session_id: None,
    });

    // Send tool result with parent_tool_use_id
    let parsed = ParsedLine {
        message: StreamMessage::User {
            message: UserMessage {
                content: vec![UserContent::ToolResult {
                    tool_use_id: "toolu_sub_result".to_string(),
                    content: serde_json::json!("found 3 matches"),
                    is_error: false,
                }],
            },
        },
        parent_tool_use_id: Some("toolu_parent_task".to_string()),
        is_synthetic: false,
        tool_use_result: None,
    };

    let events = processor.process_parsed_line(parsed);
    assert_eq!(events.len(), 1);
    match &events[0] {
        StreamEvent::ToolResultReceived {
            tool_use_id,
            parent_tool_use_id,
            ..
        } => {
            assert_eq!(tool_use_id, "toolu_sub_result");
            assert_eq!(parent_tool_use_id, &Some("toolu_parent_task".to_string()));
        }
        other => panic!("Expected ToolResultReceived, got {:?}", other),
    }
}
