use super::*;

#[test]
fn test_task_started_emitted_verbose_mode() {
    let mut processor = StreamProcessor::new();

    let msg = StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![AssistantContent::ToolUse {
                id: "toolu_task1".to_string(),
                name: "Task".to_string(),
                input: serde_json::json!({
                    "description": "Search codebase",
                    "subagent_type": "Explore",
                    "model": "sonnet",
                    "prompt": "Find all files"
                }),
            }],
            stop_reason: None,
            usage: None,
        },
        session_id: None,
    };

    let events = processor.process_message(msg);

    // Should emit: TaskStarted, ToolCallCompleted
    assert_eq!(events.len(), 2);
    match &events[0] {
        StreamEvent::TaskStarted {
            tool_use_id,
            tool_name,
            description,
            subagent_type,
            model,
        } => {
            assert_eq!(tool_use_id, "toolu_task1");
            assert_eq!(tool_name, "Task");
            assert_eq!(description, &Some("Search codebase".to_string()));
            assert_eq!(subagent_type, &Some("Explore".to_string()));
            assert_eq!(model, &Some("sonnet".to_string()));
        }
        other => panic!("Expected TaskStarted, got {:?}", other),
    }
    assert!(matches!(&events[1], StreamEvent::ToolCallCompleted { .. }));
}

#[test]
fn test_task_started_emitted_streaming_mode() {
    let mut processor = StreamProcessor::new();

    // Start Task tool call
    processor.process_message(StreamMessage::ContentBlockStart {
        index: Some(0),
        content_block: ContentBlock {
            block_type: "tool_use".to_string(),
            id: Some("toolu_task2".to_string()),
            name: Some("Task".to_string()),
            text: None,
            input: None,
        },
    });

    // Input delta
    processor.process_message(StreamMessage::ContentBlockDelta {
        index: Some(0),
        delta: ContentDelta {
            delta_type: "input_json_delta".to_string(),
            text: None,
            partial_json: Some(
                r#"{"description":"Run tests","subagent_type":"Bash","model":"haiku"}"#.to_string(),
            ),
        },
    });

    // Stop
    let events = processor.process_message(StreamMessage::ContentBlockStop { index: Some(0) });

    // Should emit: TaskStarted, ToolCallCompleted
    assert_eq!(events.len(), 2);
    match &events[0] {
        StreamEvent::TaskStarted {
            tool_use_id,
            tool_name,
            description,
            subagent_type,
            model,
        } => {
            assert_eq!(tool_use_id, "toolu_task2");
            assert_eq!(tool_name, "Task");
            assert_eq!(description, &Some("Run tests".to_string()));
            assert_eq!(subagent_type, &Some("Bash".to_string()));
            assert_eq!(model, &Some("haiku".to_string()));
        }
        other => panic!("Expected TaskStarted, got {:?}", other),
    }
}

#[test]
fn test_task_started_with_standard_args() {
    let mut processor = StreamProcessor::new();

    // Verbose mode: Task tool with standard args
    let events = processor.process_message(StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![AssistantContent::ToolUse {
                id: "toolu_notm".to_string(),
                name: "Task".to_string(),
                input: serde_json::json!({
                    "prompt": "search files",
                    "subagent_type": "Explore",
                    "description": "Find config"
                }),
            }],
            stop_reason: None,
            usage: None,
        },
        session_id: None,
    });

    // Should emit TaskStarted in verbose mode
    assert!(!events.is_empty());
    match &events[0] {
        StreamEvent::TaskStarted { .. } => {}
        other => panic!("Expected TaskStarted, got {:?}", other),
    }
}

#[test]
fn test_task_completed_emitted_on_tool_result() {
    let mut processor = StreamProcessor::new();

    // First, register a Task tool_use
    let task_msg = StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![AssistantContent::ToolUse {
                id: "toolu_task3".to_string(),
                name: "Task".to_string(),
                input: serde_json::json!({
                    "description": "Search files",
                    "subagent_type": "Explore"
                }),
            }],
            stop_reason: None,
            usage: None,
        },
        session_id: None,
    };
    processor.process_message(task_msg);

    // Now send the tool_result with metadata
    let result_msg = StreamMessage::User {
        message: UserMessage {
            content: vec![UserContent::ToolResult {
                tool_use_id: "toolu_task3".to_string(),
                content: serde_json::json!({
                    "tool_use_result": {
                        "agentId": "agent-abc-123",
                        "totalDurationMs": 12500,
                        "totalTokens": 4500,
                        "totalToolUseCount": 8
                    }
                }),
                is_error: false,
            }],
        },
    };

    let events = processor.process_message(result_msg);

    // Should emit: TaskCompleted, ToolResultReceived
    assert_eq!(events.len(), 2);
    match &events[0] {
        StreamEvent::TaskCompleted {
            tool_use_id,
            agent_id,
            total_duration_ms,
            total_tokens,
            total_tool_use_count,
        } => {
            assert_eq!(tool_use_id, "toolu_task3");
            assert_eq!(agent_id, &Some("agent-abc-123".to_string()));
            assert_eq!(total_duration_ms, &Some(12500));
            assert_eq!(total_tokens, &Some(4500));
            assert_eq!(total_tool_use_count, &Some(8));
        }
        other => panic!("Expected TaskCompleted, got {:?}", other),
    }
    assert!(matches!(&events[1], StreamEvent::ToolResultReceived { .. }));
}

#[test]
fn test_non_task_tool_use_does_not_emit_task_started() {
    let mut processor = StreamProcessor::new();

    let msg = StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![AssistantContent::ToolUse {
                id: "toolu_other".to_string(),
                name: "Grep".to_string(),
                input: serde_json::json!({"pattern": "test"}),
            }],
            stop_reason: None,
            usage: None,
        },
        session_id: None,
    };

    let events = processor.process_message(msg);
    // Should only emit ToolCallCompleted, NOT TaskStarted
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0], StreamEvent::ToolCallCompleted { .. }));
}

#[test]
fn test_task_completed_parses_usage_text_format() {
    let mut processor = StreamProcessor::new();

    // Register a Task tool_use
    processor.process_message(StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![AssistantContent::ToolUse {
                id: "toolu_task_text".to_string(),
                name: "Task".to_string(),
                input: serde_json::json!({
                    "description": "Search codebase",
                    "subagent_type": "Explore"
                }),
            }],
            stop_reason: None,
            usage: None,
        },
        session_id: None,
    });

    // Send tool_result as plain text with <usage> block (actual Claude CLI format)
    let result_msg = StreamMessage::User {
        message: UserMessage {
            content: vec![UserContent::ToolResult {
                tool_use_id: "toolu_task_text".to_string(),
                content: serde_json::json!(
                    "Found 3 matching files in src/components/\nagentId: a7db0f4 (for resuming to continue this agent's work if needed)\n<usage>total_tokens: 44969\ntool_uses: 12\nduration_ms: 7900</usage>"
                ),
                is_error: false,
            }],
        },
    };

    let events = processor.process_message(result_msg);

    // Should emit: TaskCompleted, ToolResultReceived
    assert_eq!(events.len(), 2);
    match &events[0] {
        StreamEvent::TaskCompleted {
            tool_use_id,
            agent_id,
            total_duration_ms,
            total_tokens,
            total_tool_use_count,
        } => {
            assert_eq!(tool_use_id, "toolu_task_text");
            assert_eq!(agent_id, &Some("a7db0f4".to_string()));
            assert_eq!(total_duration_ms, &Some(7900));
            assert_eq!(total_tokens, &Some(44969));
            assert_eq!(total_tool_use_count, &Some(12));
        }
        other => panic!("Expected TaskCompleted, got {:?}", other),
    }
    assert!(matches!(&events[1], StreamEvent::ToolResultReceived { .. }));
}

#[test]
fn test_task_completed_parses_content_blocks_format() {
    let mut processor = StreamProcessor::new();

    // Register a Task tool_use
    processor.process_message(StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![AssistantContent::ToolUse {
                id: "toolu_task_blocks".to_string(),
                name: "Task".to_string(),
                input: serde_json::json!({
                    "description": "Run tests",
                    "subagent_type": "Bash"
                }),
            }],
            stop_reason: None,
            usage: None,
        },
        session_id: None,
    });

    // Send tool_result as content block array (text blocks with usage info)
    let result_msg = StreamMessage::User {
        message: UserMessage {
            content: vec![UserContent::ToolResult {
                tool_use_id: "toolu_task_blocks".to_string(),
                content: serde_json::json!([
                    {"type": "text", "text": "All tests passed.\n"},
                    {"type": "text", "text": "agentId: ff0011 (for resuming...)\n<usage>total_tokens: 8000\ntool_uses: 3\nduration_ms: 15000</usage>"}
                ]),
                is_error: false,
            }],
        },
    };

    let events = processor.process_message(result_msg);

    assert_eq!(events.len(), 2);
    match &events[0] {
        StreamEvent::TaskCompleted {
            tool_use_id,
            agent_id,
            total_duration_ms,
            total_tokens,
            total_tool_use_count,
        } => {
            assert_eq!(tool_use_id, "toolu_task_blocks");
            assert_eq!(agent_id, &Some("ff0011".to_string()));
            assert_eq!(total_duration_ms, &Some(15000));
            assert_eq!(total_tokens, &Some(8000));
            assert_eq!(total_tool_use_count, &Some(3));
        }
        other => panic!("Expected TaskCompleted, got {:?}", other),
    }
}

#[test]
fn test_task_completed_no_stats_still_emits_event() {
    let mut processor = StreamProcessor::new();

    // Register a Task tool_use
    processor.process_message(StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![AssistantContent::ToolUse {
                id: "toolu_task_nostats".to_string(),
                name: "Task".to_string(),
                input: serde_json::json!({
                    "description": "Simple task",
                    "subagent_type": "Bash"
                }),
            }],
            stop_reason: None,
            usage: None,
        },
        session_id: None,
    });

    // Send tool_result with no stats at all
    let result_msg = StreamMessage::User {
        message: UserMessage {
            content: vec![UserContent::ToolResult {
                tool_use_id: "toolu_task_nostats".to_string(),
                content: serde_json::json!("Just some plain output with no stats"),
                is_error: false,
            }],
        },
    };

    let events = processor.process_message(result_msg);

    // TaskCompleted should still be emitted, just with None stats
    assert_eq!(events.len(), 2);
    match &events[0] {
        StreamEvent::TaskCompleted {
            tool_use_id,
            agent_id,
            total_duration_ms,
            total_tokens,
            total_tool_use_count,
        } => {
            assert_eq!(tool_use_id, "toolu_task_nostats");
            assert_eq!(agent_id, &None);
            assert_eq!(total_duration_ms, &None);
            assert_eq!(total_tokens, &None);
            assert_eq!(total_tool_use_count, &None);
        }
        other => panic!("Expected TaskCompleted, got {:?}", other),
    }
}
