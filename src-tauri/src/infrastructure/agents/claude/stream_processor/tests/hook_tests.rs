use super::*;

#[test]
fn test_hook_started_from_system_message() {
    let mut processor = StreamProcessor::new();

    let msg = StreamMessage::System {
        message: Some("Running hook...".to_string()),
        session_id: None,
        subtype: Some("hook_started".to_string()),
        hook_id: Some("hook-abc-123".to_string()),
        hook_name: Some("rule-audit.sh".to_string()),
        hook_event: Some("SessionStart".to_string()),
        output: None,
        exit_code: None,
        outcome: None,
        estimated_tokens: None,
        estimated_tokens_delta: None,
    };

    let events = processor.process_message(msg);
    assert_eq!(events.len(), 1);
    match &events[0] {
        StreamEvent::HookStarted {
            hook_id,
            hook_name,
            hook_event,
        } => {
            assert_eq!(hook_id, "hook-abc-123");
            assert_eq!(hook_name, "rule-audit.sh");
            assert_eq!(hook_event, "SessionStart");
        }
        other => panic!("Expected HookStarted, got {:?}", other),
    }
}

#[test]
fn test_hook_completed_from_system_message() {
    let mut processor = StreamProcessor::new();

    let msg = StreamMessage::System {
        message: Some("Hook completed".to_string()),
        session_id: Some("sess-456".to_string()),
        subtype: Some("hook_response".to_string()),
        hook_id: Some("hook-def-456".to_string()),
        hook_name: Some("lint-fix.sh".to_string()),
        hook_event: Some("PostToolUse".to_string()),
        output: Some("Fixed 3 lint issues".to_string()),
        exit_code: Some(0),
        outcome: Some("success".to_string()),
        estimated_tokens: None,
        estimated_tokens_delta: None,
    };

    let events = processor.process_message(msg);
    // Should emit SessionId + HookCompleted
    assert_eq!(events.len(), 2);
    assert!(matches!(&events[0], StreamEvent::SessionId(id) if id == "sess-456"));
    match &events[1] {
        StreamEvent::HookCompleted {
            hook_id,
            hook_name,
            hook_event,
            output,
            exit_code,
            outcome,
        } => {
            assert_eq!(hook_id, "hook-def-456");
            assert_eq!(hook_name, "lint-fix.sh");
            assert_eq!(hook_event, "PostToolUse");
            assert_eq!(output, &Some("Fixed 3 lint issues".to_string()));
            assert_eq!(exit_code, &Some(0));
            assert_eq!(outcome, &Some("success".to_string()));
        }
        other => panic!("Expected HookCompleted, got {:?}", other),
    }
}

#[test]
fn test_hook_completed_with_error() {
    let mut processor = StreamProcessor::new();

    let msg = StreamMessage::System {
        message: None,
        session_id: None,
        subtype: Some("hook_response".to_string()),
        hook_id: Some("hook-err-789".to_string()),
        hook_name: Some("enforce-rule-manager.sh".to_string()),
        hook_event: Some("Stop".to_string()),
        output: Some("Rule manager has pending issues".to_string()),
        exit_code: Some(1),
        outcome: Some("error".to_string()),
        estimated_tokens: None,
        estimated_tokens_delta: None,
    };

    let events = processor.process_message(msg);
    assert_eq!(events.len(), 1);
    match &events[0] {
        StreamEvent::HookCompleted {
            exit_code, outcome, ..
        } => {
            assert_eq!(exit_code, &Some(1));
            assert_eq!(outcome, &Some("error".to_string()));
        }
        other => panic!("Expected HookCompleted, got {:?}", other),
    }
}

#[test]
fn test_hook_block_from_synthetic_user_message() {
    let mut processor = StreamProcessor::new();

    let parsed = ParsedLine {
        message: StreamMessage::User {
            message: UserMessage {
                content: vec![UserContent::Text {
                    text: "Stop hook blocked: enforce-rule-manager.sh\nRule manager audit found issues that need fixing.".to_string(),
                }],
            },
        },
        parent_tool_use_id: None,
        is_synthetic: true,
        tool_use_result: None,
    };

    let events = processor.process_parsed_line(parsed);
    assert_eq!(events.len(), 1);
    match &events[0] {
        StreamEvent::HookBlock { reason } => {
            assert!(reason.contains("Stop hook blocked"));
            assert!(reason.contains("enforce-rule-manager.sh"));
        }
        other => panic!("Expected HookBlock, got {:?}", other),
    }
}

#[test]
fn test_non_synthetic_user_text_ignored() {
    let mut processor = StreamProcessor::new();

    // Non-synthetic user text should NOT emit HookBlock
    let parsed = ParsedLine {
        message: StreamMessage::User {
            message: UserMessage {
                content: vec![UserContent::Text {
                    text: "Some regular user text".to_string(),
                }],
            },
        },
        parent_tool_use_id: None,
        is_synthetic: false,
        tool_use_result: None,
    };

    let events = processor.process_parsed_line(parsed);
    assert!(
        events.is_empty(),
        "Non-synthetic text should not emit events"
    );
}

#[test]
fn test_hook_started_missing_required_fields() {
    let mut processor = StreamProcessor::new();

    // Missing hook_name — should NOT emit HookStarted
    let msg = StreamMessage::System {
        message: None,
        session_id: None,
        subtype: Some("hook_started".to_string()),
        hook_id: Some("hook-123".to_string()),
        hook_name: None,
        hook_event: Some("SessionStart".to_string()),
        output: None,
        exit_code: None,
        outcome: None,
        estimated_tokens: None,
        estimated_tokens_delta: None,
    };

    let events = processor.process_message(msg);
    assert!(
        events.is_empty(),
        "HookStarted should not emit with missing hook_name"
    );
}

#[test]
fn test_hook_completed_optional_fields_none() {
    let mut processor = StreamProcessor::new();

    let msg = StreamMessage::System {
        message: None,
        session_id: None,
        subtype: Some("hook_response".to_string()),
        hook_id: Some("hook-opt-1".to_string()),
        hook_name: Some("my-hook.sh".to_string()),
        hook_event: Some("PostToolUse".to_string()),
        output: None,
        exit_code: None,
        outcome: None,
        estimated_tokens: None,
        estimated_tokens_delta: None,
    };

    let events = processor.process_message(msg);
    assert_eq!(events.len(), 1);
    match &events[0] {
        StreamEvent::HookCompleted {
            output,
            exit_code,
            outcome,
            ..
        } => {
            assert_eq!(output, &None);
            assert_eq!(exit_code, &None);
            assert_eq!(outcome, &None);
        }
        other => panic!("Expected HookCompleted, got {:?}", other),
    }
}
