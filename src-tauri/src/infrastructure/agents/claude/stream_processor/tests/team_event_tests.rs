use super::*;

#[test]
fn test_detect_team_created_from_tool_result() {
    let result = serde_json::json!({
        "team_name": "my-team",
        "team_file_path": "/home/user/.claude/teams/my-team.json",
        "lead_agent_id": "abc123"
    });
    let event = StreamProcessor::detect_team_event("toolu_1", &result);
    assert!(event.is_some());
    match event.unwrap() {
        StreamEvent::TeamCreated {
            team_name,
            config_path,
        } => {
            assert_eq!(team_name, "my-team");
            assert_eq!(config_path, "/home/user/.claude/teams/my-team.json");
        }
        other => panic!("Expected TeamCreated, got {:?}", other),
    }
}

#[test]
fn test_detect_teammate_spawned_from_tool_result() {
    let result = serde_json::json!({
        "status": "teammate_spawned",
        "name": "researcher",
        "teammate_id": "researcher@my-team",
        "agent_id": "def456",
        "model": "sonnet",
        "color": "green",
        "prompt": "Research WebSocket vs SSE transport options",
        "agent_type": "general-purpose"
    });
    let event = StreamProcessor::detect_team_event("toolu_2", &result);
    assert!(event.is_some());
    match event.unwrap() {
        StreamEvent::TeammateSpawned {
            teammate_name,
            team_name,
            agent_id,
            model,
            color,
            prompt,
            agent_type,
        } => {
            assert_eq!(teammate_name, "researcher");
            assert_eq!(team_name, "my-team");
            assert_eq!(agent_id, "def456");
            assert_eq!(model, "sonnet");
            assert_eq!(color, "green");
            assert_eq!(prompt, "Research WebSocket vs SSE transport options");
            assert_eq!(agent_type, "general-purpose");
        }
        other => panic!("Expected TeammateSpawned, got {:?}", other),
    }
}

#[test]
fn test_detect_team_message_sent_from_tool_result() {
    let result = serde_json::json!({
        "success": true,
        "routing": {
            "sender": "team-lead",
            "target": "researcher",
            "content": "Please investigate the bug"
        }
    });
    let event = StreamProcessor::detect_team_event("toolu_3", &result);
    assert!(event.is_some());
    match event.unwrap() {
        StreamEvent::TeamMessageSent {
            sender,
            recipient,
            content,
            message_type,
        } => {
            assert_eq!(sender, "team-lead");
            assert_eq!(recipient, Some("researcher".to_string()));
            assert_eq!(content, "Please investigate the bug");
            assert_eq!(message_type, "message");
        }
        other => panic!("Expected TeamMessageSent, got {:?}", other),
    }
}

#[test]
fn test_detect_team_message_sent_broadcast() {
    let result = serde_json::json!({
        "success": true,
        "routing": {
            "sender": "team-lead",
            "content": "All stop — blocking issue found"
        }
    });
    let event = StreamProcessor::detect_team_event("toolu_4", &result);
    assert!(event.is_some());
    match event.unwrap() {
        StreamEvent::TeamMessageSent {
            sender,
            recipient,
            message_type,
            ..
        } => {
            assert_eq!(sender, "team-lead");
            assert!(
                recipient.is_none(),
                "Broadcast should have no single recipient"
            );
            assert_eq!(message_type, "broadcast");
        }
        other => panic!("Expected TeamMessageSent (broadcast), got {:?}", other),
    }
}

#[test]
fn test_detect_team_deleted_from_tool_result() {
    let result = serde_json::json!({
        "team_deleted": true,
        "team_name": "my-team"
    });
    let event = StreamProcessor::detect_team_event("toolu_5", &result);
    assert!(event.is_some());
    match event.unwrap() {
        StreamEvent::TeamDeleted { team_name } => {
            assert_eq!(team_name, "my-team");
        }
        other => panic!("Expected TeamDeleted, got {:?}", other),
    }
}

#[test]
fn test_detect_team_deleted_with_deleted_flag() {
    let result = serde_json::json!({
        "deleted": true,
        "team_name": "other-team"
    });
    let event = StreamProcessor::detect_team_event("toolu_6", &result);
    assert!(event.is_some());
    match event.unwrap() {
        StreamEvent::TeamDeleted { team_name } => {
            assert_eq!(team_name, "other-team");
        }
        other => panic!("Expected TeamDeleted, got {:?}", other),
    }
}

#[test]
fn test_detect_no_team_event_for_regular_result() {
    let result = serde_json::json!({
        "output": "Hello world",
        "exit_code": 0
    });
    let event = StreamProcessor::detect_team_event("toolu_7", &result);
    assert!(
        event.is_none(),
        "Regular tool result should not produce a team event"
    );
}

/// Integration test: verify team events are detected from top-level tool_use_result
/// in the actual Claude Code stream-json format (where content is text, not structured JSON).
#[test]
fn test_teammate_spawned_from_real_stream_format() {
    let mut processor = StreamProcessor::new();

    // Register the Task tool call (assistant message)
    processor.process_message(StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![AssistantContent::ToolUse {
                id: "toolu_019oPFhAfvpV3c1Zaw1S9V5e".to_string(),
                name: "Task".to_string(),
                input: serde_json::json!({
                    "name": "team-tabs",
                    "team_name": "tabs-vs-spaces-debate",
                    "subagent_type": "general-purpose",
                    "prompt": "You are team-tabs..."
                }),
            }],
            stop_reason: None,
            usage: None,
        },
        session_id: None,
    });

    // Actual Claude Code stream format: content is text array, tool_use_result is top-level
    let parsed = ParsedLine {
        message: StreamMessage::User {
            message: UserMessage {
                content: vec![UserContent::ToolResult {
                    tool_use_id: "toolu_019oPFhAfvpV3c1Zaw1S9V5e".to_string(),
                    content: serde_json::json!([
                        {"type": "text", "text": "Spawned successfully.\nagent_id: team-tabs@tabs-vs-spaces-debate\nname: team-tabs"}
                    ]),
                    is_error: false,
                }],
            },
        },
        parent_tool_use_id: None,
        is_synthetic: false,
        tool_use_result: Some(serde_json::json!({
            "status": "teammate_spawned",
            "name": "team-tabs",
            "teammate_id": "team-tabs@tabs-vs-spaces-debate",
            "agent_id": "team-tabs@tabs-vs-spaces-debate",
            "agent_type": "general-purpose",
            "model": "claude-opus-4-6",
            "color": "blue",
            "prompt": "You are team-tabs...",
            "team_name": "tabs-vs-spaces-debate"
        })),
    };

    let events = processor.process_parsed_line(parsed);

    // Should emit: TaskCompleted, ToolResultReceived, TeammateSpawned
    let teammate_event = events
        .iter()
        .find(|e| matches!(e, StreamEvent::TeammateSpawned { .. }));
    assert!(
        teammate_event.is_some(),
        "Expected TeammateSpawned event from tool_use_result, got: {:?}",
        events
    );
    match teammate_event.unwrap() {
        StreamEvent::TeammateSpawned {
            teammate_name,
            team_name,
            model,
            color,
            prompt,
            ..
        } => {
            assert_eq!(teammate_name, "team-tabs");
            assert_eq!(team_name, "tabs-vs-spaces-debate");
            assert_eq!(model, "claude-opus-4-6");
            assert_eq!(color, "blue");
            assert_eq!(prompt, "You are team-tabs...");
        }
        _ => unreachable!(),
    }
}

/// Integration test: verify TeamCreated detected from top-level tool_use_result
#[test]
fn test_team_created_from_real_stream_format() {
    let mut processor = StreamProcessor::new();

    // Register TeamCreate tool call
    processor.process_message(StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![AssistantContent::ToolUse {
                id: "toolu_team_create_real".to_string(),
                name: "TeamCreate".to_string(),
                input: serde_json::json!({"team_name": "tabs-vs-spaces-debate"}),
            }],
            stop_reason: None,
            usage: None,
        },
        session_id: None,
    });

    // Real format: content is text (JSON-as-string), tool_use_result is structured
    let parsed = ParsedLine {
        message: StreamMessage::User {
            message: UserMessage {
                content: vec![UserContent::ToolResult {
                    tool_use_id: "toolu_team_create_real".to_string(),
                    content: serde_json::json!([
                        {"type": "text", "text": "{\n  \"team_name\": \"tabs-vs-spaces-debate\",\n  \"team_file_path\": \"/home/.claude/teams/tabs-vs-spaces-debate/config.json\",\n  \"lead_agent_id\": \"team-lead@tabs-vs-spaces-debate\"\n}"}
                    ]),
                    is_error: false,
                }],
            },
        },
        parent_tool_use_id: None,
        is_synthetic: false,
        tool_use_result: Some(serde_json::json!({
            "team_name": "tabs-vs-spaces-debate",
            "team_file_path": "/home/.claude/teams/tabs-vs-spaces-debate/config.json",
            "lead_agent_id": "team-lead@tabs-vs-spaces-debate"
        })),
    };

    let events = processor.process_parsed_line(parsed);
    let team_event = events
        .iter()
        .find(|e| matches!(e, StreamEvent::TeamCreated { .. }));
    assert!(
        team_event.is_some(),
        "Expected TeamCreated from tool_use_result, got: {:?}",
        events
    );
}

#[test]
fn test_detect_no_team_event_for_string_result() {
    let result = serde_json::json!("Just a plain string result");
    let event = StreamProcessor::detect_team_event("toolu_8", &result);
    assert!(
        event.is_none(),
        "String result should not produce a team event"
    );
}

#[test]
fn test_team_event_emitted_after_tool_result_received() {
    let mut processor = StreamProcessor::new();

    // Register a tool call (simulating TeamCreate being called)
    processor.process_message(StreamMessage::Assistant {
        message: AssistantMessage {
            content: vec![AssistantContent::ToolUse {
                id: "toolu_team_create".to_string(),
                name: "TeamCreate".to_string(),
                input: serde_json::json!({"team_name": "test-team"}),
            }],
            stop_reason: None,
            usage: None,
        },
        session_id: None,
    });

    // Send tool result that looks like TeamCreate output
    let result_msg = StreamMessage::User {
        message: UserMessage {
            content: vec![UserContent::ToolResult {
                tool_use_id: "toolu_team_create".to_string(),
                content: serde_json::json!({
                    "team_name": "test-team",
                    "team_file_path": "/home/user/.claude/teams/test-team.json",
                    "lead_agent_id": "lead123"
                }),
                is_error: false,
            }],
        },
    };

    let events = processor.process_message(result_msg);

    // Should emit: ToolResultReceived, TeamCreated
    assert_eq!(events.len(), 2);
    assert!(matches!(&events[0], StreamEvent::ToolResultReceived { .. }));
    match &events[1] {
        StreamEvent::TeamCreated {
            team_name,
            config_path,
        } => {
            assert_eq!(team_name, "test-team");
            assert_eq!(config_path, "/home/user/.claude/teams/test-team.json");
        }
        other => panic!("Expected TeamCreated, got {:?}", other),
    }
}
