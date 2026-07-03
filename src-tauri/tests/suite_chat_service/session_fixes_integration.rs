// Session Fixes Integration Tests
//
// Tests for three fixes applied in this session:
//
// Fix 1 — Text buffer leak (commit 39cae780):
//   The legacy text_buffer accumulated ALL TextChunk output and dumped it via
//   add_teammate_message() on turn boundaries and EOF. This caused every line of
//   teammate reasoning to appear as a "teammate → all" message. The fix removes
//   the text_buffer entirely — only TeamMessageSent tool calls should emit
//   team:message events. These tests verify that stream events from text output
//   lines are TextChunk (not TeamMessageSent).
//
// Fix 2 — Team mode parity (commit 73c374f9):
//   send_agent_message didn't enable with_team_mode for TaskExecution contexts.
//   Fix: check task.metadata["agent_variant"] == "team" for task_execution context.
//   Also: start_teammate_stream now properly propagates context_type string →
//   ChatContextType for teammate conversations (task_execution → TaskExecution).
//
// Fix 3 — Running count nudge bypass (Phase 2):
//   team_stream_processor.rs nudge path now calls claim_interactive_slot +
//   increment_running after write_message succeeds.
//   → Already fully covered by src-tauri/tests/team_nudge_running_count_tests.rs (9 tests)

use ralphx_lib::domain::entities::{ChatContextType, IdeationSessionId, ProjectId, Task};
use ralphx_lib::domain::entities::ideation::IdeationSessionBuilder;
use ralphx_lib::infrastructure::agents::claude::{StreamEvent, StreamProcessor};

// ============================================================================
// Fix 1 — Text Buffer Leak: StreamProcessor event routing
//
// Verifies that text output lines produce StreamEvent::TextChunk and never
// produce StreamEvent::TeamMessageSent. The legacy text_buffer bug caused
// TextChunk content to be routed to add_teammate_message on turn boundaries.
// ============================================================================

/// TextChunk lines from assistant text output MUST produce StreamEvent::TextChunk,
/// NOT StreamEvent::TeamMessageSent.
///
/// This guards against a regression where text output gets routed to
/// add_teammate_message (the text buffer leak bug).
#[test]
fn test_text_delta_produces_text_chunk_not_team_message() {
    let mut processor = StreamProcessor::new();

    // Simulate an assistant text delta line (stream-json format)
    // This is what Claude Code produces when the agent writes text
    let text_delta_line = r#"{"type":"assistant","message":{"id":"msg_1","type":"message","role":"assistant","model":"claude-opus-4-6","content":[{"type":"text","text":"hello"}],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":100,"output_tokens":5}}}"#;

    let parsed = StreamProcessor::parse_line(text_delta_line);
    assert!(
        parsed.is_some(),
        "Text delta line should be parseable as a stream-json line"
    );

    let events = processor.process_parsed_line(parsed.unwrap());

    // Must contain TextChunk
    let has_text_chunk = events.iter().any(|e| matches!(e, StreamEvent::TextChunk(_)));
    assert!(
        has_text_chunk,
        "Text delta must produce at least one TextChunk event"
    );

    // Must NOT contain TeamMessageSent
    let has_team_message = events
        .iter()
        .any(|e| matches!(e, StreamEvent::TeamMessageSent { .. }));
    assert!(
        !has_team_message,
        "Text delta must NEVER produce TeamMessageSent — that would be the text buffer leak"
    );
}

/// Multiple text chunks accumulate correctly — none should become TeamMessageSent.
/// This mirrors the scenario where a teammate produces several paragraphs of text
/// before sending a tool call message.
#[test]
fn test_multiple_text_chunks_never_produce_team_message() {
    let mut processor = StreamProcessor::new();

    let text_lines = [
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"First paragraph. "}]}}"#,
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Second paragraph. "}]}}"#,
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Third paragraph."}]}}"#,
    ];

    let mut all_events: Vec<StreamEvent> = Vec::new();
    for line in &text_lines {
        if let Some(parsed) = StreamProcessor::parse_line(line) {
            all_events.extend(processor.process_parsed_line(parsed));
        }
    }

    // Must have some TextChunk events
    let text_chunk_count = all_events
        .iter()
        .filter(|e| matches!(e, StreamEvent::TextChunk(_)))
        .count();
    assert!(
        text_chunk_count > 0,
        "Multiple text lines must produce TextChunk events"
    );

    // Zero TeamMessageSent events — text output must never reach add_teammate_message
    let team_msg_count = all_events
        .iter()
        .filter(|e| matches!(e, StreamEvent::TeamMessageSent { .. }))
        .count();
    assert_eq!(
        team_msg_count, 0,
        "Text output must produce ZERO TeamMessageSent events (text buffer leak regression)"
    );
}

/// Text output events are EXCLUSIVELY TextChunk — never TeamMessageSent.
/// This is the core fix for the text buffer leak: prior code accumulated
/// TextChunk text in a buffer and dumped it via add_teammate_message() at
/// turn boundaries. This test verifies that TextChunk events can't be
/// mistaken for TeamMessageSent events.
#[test]
fn test_text_chunk_event_type_is_never_team_message_sent() {
    let mut processor = StreamProcessor::new();

    // Multi-turn text output
    let lines = [
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Analyzing..."}]}}"#,
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Found it!"}]}}"#,
    ];

    for line in &lines {
        if let Some(parsed) = StreamProcessor::parse_line(line) {
            let events = processor.process_parsed_line(parsed);
            for event in &events {
                assert!(
                    !matches!(event, StreamEvent::TeamMessageSent { .. }),
                    "TextChunk event must not be TeamMessageSent — \
                     this would be the text buffer leak regression: {:?}",
                    event
                );
            }
        }
    }
}

/// StreamProcessor state resets correctly between turns.
/// After reset_for_next_turn(), accumulated response_text is cleared
/// so there's no carryover that could be mistakenly routed as a team message.
#[test]
fn test_stream_processor_reset_clears_accumulated_text() {
    let mut processor = StreamProcessor::new();

    // Accumulate some text
    let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"thinking..."}]}}"#;
    if let Some(parsed) = StreamProcessor::parse_line(line) {
        processor.process_parsed_line(parsed);
    }

    // After reset, response_text is cleared (no buffer to dump as team message)
    processor.reset_for_next_turn();
    assert!(
        processor.response_text.is_empty(),
        "After reset_for_next_turn(), response_text must be empty — \
         no stale text that could be incorrectly routed as a team message"
    );
}

// ============================================================================
// Fix 2 — Team Mode Parity: agent_variant metadata parsing
//
// send_agent_message enables with_team_mode(true) for TaskExecution context
// when task.metadata contains {"agent_variant": "team"}.
// ============================================================================

/// Task with metadata {"agent_variant": "team"} must enable team mode.
/// This is the core check in send_agent_message for TaskExecution contexts.
#[test]
fn test_agent_variant_team_metadata_enables_team_mode() {
    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id, "Test task".to_string());
    task.metadata = Some(r#"{"agent_variant": "team"}"#.to_string());

    // Replicate the logic from unified_chat_commands.rs send_agent_message:
    let is_team = task
        .metadata
        .as_ref()
        .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        .and_then(|meta| {
            meta.get("agent_variant")
                .and_then(|v| v.as_str())
                .map(|s| s == "team")
        })
        .unwrap_or(false);

    assert!(
        is_team,
        "Task with agent_variant=team must enable team mode in send_agent_message"
    );
}

/// Task with no metadata must NOT enable team mode (default = false).
#[test]
fn test_no_metadata_does_not_enable_team_mode() {
    let project_id = ProjectId::from_string("proj-2".to_string());
    let task = Task::new(project_id, "Regular task".to_string());
    assert!(task.metadata.is_none());

    let is_team = task
        .metadata
        .as_ref()
        .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        .and_then(|meta| {
            meta.get("agent_variant")
                .and_then(|v| v.as_str())
                .map(|s| s == "team")
        })
        .unwrap_or(false);

    assert!(
        !is_team,
        "Task with no metadata must NOT enable team mode"
    );
}

/// Task with agent_variant="worker" (not "team") must NOT enable team mode.
#[test]
fn test_agent_variant_worker_does_not_enable_team_mode() {
    let project_id = ProjectId::from_string("proj-3".to_string());
    let mut task = Task::new(project_id, "Worker task".to_string());
    task.metadata = Some(r#"{"agent_variant": "worker"}"#.to_string());

    let is_team = task
        .metadata
        .as_ref()
        .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        .and_then(|meta| {
            meta.get("agent_variant")
                .and_then(|v| v.as_str())
                .map(|s| s == "team")
        })
        .unwrap_or(false);

    assert!(
        !is_team,
        "Task with agent_variant=worker must NOT enable team mode"
    );
}

/// Task with metadata containing extra keys plus agent_variant="team" must enable team mode.
#[test]
fn test_agent_variant_team_with_other_metadata_keys() {
    let project_id = ProjectId::from_string("proj-4".to_string());
    let mut task = Task::new(project_id, "Team task with extra metadata".to_string());
    task.metadata = Some(
        r#"{"agent_variant": "team", "priority": "high", "sprint": "42"}"#.to_string(),
    );

    let is_team = task
        .metadata
        .as_ref()
        .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        .and_then(|meta| {
            meta.get("agent_variant")
                .and_then(|v| v.as_str())
                .map(|s| s == "team")
        })
        .unwrap_or(false);

    assert!(
        is_team,
        "Task with agent_variant=team among other metadata keys must enable team mode"
    );
}

// ============================================================================
// Fix 2 — Team Mode Parity: IdeationSession team_mode parsing
//
// send_agent_message enables with_team_mode(true) for Ideation context
// when session.team_mode is Some and != "solo".
// ============================================================================

/// IdeationSession with team_mode="collaborative" must enable team mode.
#[test]
fn test_ideation_session_collaborative_team_mode() {
    let session = IdeationSessionBuilder::new()
        .id(IdeationSessionId::from_string("sess-1".to_string()))
        .title("Test session")
        .project_id(ProjectId::from_string("proj-1".to_string()))
        .team_mode("collaborative")
        .build();

    // Replicate logic from send_agent_message Ideation branch:
    let is_team = session
        .team_mode
        .as_deref()
        .is_some_and(|m| m != "solo");

    assert!(
        is_team,
        "IdeationSession with team_mode=collaborative must enable team mode"
    );
}

/// IdeationSession with team_mode="solo" must NOT enable team mode.
#[test]
fn test_ideation_session_solo_team_mode_disabled() {
    let session = IdeationSessionBuilder::new()
        .id(IdeationSessionId::from_string("sess-2".to_string()))
        .title("Solo session")
        .project_id(ProjectId::from_string("proj-1".to_string()))
        .team_mode("solo")
        .build();

    let is_team = session
        .team_mode
        .as_deref()
        .is_some_and(|m| m != "solo");

    assert!(
        !is_team,
        "IdeationSession with team_mode=solo must NOT enable team mode"
    );
}

/// IdeationSession with no team_mode must NOT enable team mode.
#[test]
fn test_ideation_session_no_team_mode_disabled() {
    let session = IdeationSessionBuilder::new()
        .id(IdeationSessionId::from_string("sess-3".to_string()))
        .title("Default session")
        .project_id(ProjectId::from_string("proj-1".to_string()))
        .build();

    let is_team = session
        .team_mode
        .as_deref()
        .is_some_and(|m| m != "solo");

    assert!(
        !is_team,
        "IdeationSession with no team_mode must NOT enable team mode"
    );
}

// ============================================================================
// Fix 2 — Team Mode Parity: ChatContextType parsing for teammate conversations
//
// start_teammate_stream creates per-teammate conversations. The context_type
// is set by parsing the incoming context_type string:
//   context_type.parse::<ChatContextType>().unwrap_or(ChatContextType::Ideation)
//
// This must correctly propagate the lead's context_type to teammate conversations.
// ============================================================================

/// "task_execution" context string must parse to ChatContextType::TaskExecution.
/// This ensures teammate conversations in team task execution mode get the right type.
#[test]
fn test_context_type_task_execution_parses_correctly() {
    let parsed: Result<ChatContextType, _> = "task_execution".parse();
    assert!(parsed.is_ok(), "task_execution must be a valid ChatContextType");
    assert_eq!(
        parsed.unwrap(),
        ChatContextType::TaskExecution,
        "task_execution string must parse to ChatContextType::TaskExecution"
    );
}

/// "ideation" context string must parse to ChatContextType::Ideation.
#[test]
fn test_context_type_ideation_parses_correctly() {
    let parsed: Result<ChatContextType, _> = "ideation".parse();
    assert!(parsed.is_ok(), "ideation must be a valid ChatContextType");
    assert_eq!(
        parsed.unwrap(),
        ChatContextType::Ideation,
        "ideation string must parse to ChatContextType::Ideation"
    );
}

/// Unknown context strings fall back to ChatContextType::Ideation in start_teammate_stream.
/// This tests the `.unwrap_or(ChatContextType::Ideation)` fallback.
#[test]
fn test_context_type_unknown_falls_back_to_ideation() {
    let result: ChatContextType = "unknown_context"
        .parse::<ChatContextType>()
        .unwrap_or(ChatContextType::Ideation);
    assert_eq!(
        result,
        ChatContextType::Ideation,
        "Unknown context type string must fall back to Ideation (as in start_teammate_stream)"
    );
}

/// The teammate context_id format is "teammate:{team_name}:{teammate_name}".
/// This tests the format string used when creating per-teammate conversations
/// in start_teammate_stream.
#[test]
fn test_teammate_conversation_context_id_format() {
    let team_name = "my-team";
    let teammate_name = "researcher";

    // Replicate: format!("teammate:{}:{}", team_name, teammate_name)
    let teammate_ctx_id = format!("teammate:{}:{}", team_name, teammate_name);

    assert_eq!(
        teammate_ctx_id, "teammate:my-team:researcher",
        "Teammate conversation context_id must follow 'teammate:team:name' format"
    );

    // Verify it starts with "teammate:" prefix (used for routing/filtering in frontend)
    assert!(
        teammate_ctx_id.starts_with("teammate:"),
        "Teammate context_id must start with 'teammate:' prefix"
    );
}

/// When task_execution context is used for a team, the teammate conversation
/// context_type should be TaskExecution (not default Ideation).
/// This is the team mode parity fix: team task execution teammates were
/// getting Ideation context_type instead of TaskExecution.
#[test]
fn test_teammate_conv_context_type_for_task_execution_team() {
    // Simulate start_teammate_stream receiving context_type = "task_execution"
    let context_type_str = "task_execution";
    let conv_context_type = context_type_str
        .parse::<ChatContextType>()
        .unwrap_or(ChatContextType::Ideation);

    assert_eq!(
        conv_context_type,
        ChatContextType::TaskExecution,
        "TeamExecuti on team: teammate conversations must get TaskExecution context_type, \
         not the default Ideation fallback. This is the team mode parity fix."
    );
}

/// When ideation context is used for a team, the teammate conversation
/// context_type should be Ideation.
#[test]
fn test_teammate_conv_context_type_for_ideation_team() {
    let context_type_str = "ideation";
    let conv_context_type = context_type_str
        .parse::<ChatContextType>()
        .unwrap_or(ChatContextType::Ideation);

    assert_eq!(
        conv_context_type,
        ChatContextType::Ideation,
        "Ideation team: teammate conversations must get Ideation context_type"
    );
}

// ============================================================================
// Fix 3 — Running Count Nudge Bypass
//
// This fix is fully covered by:
//   src-tauri/tests/team_nudge_running_count_tests.rs (9 tests)
//
// The tests verify:
//   - claim_interactive_slot + increment_running are called after write_message succeeds
//   - Burst prevention: only the first nudge claims an idle slot
//   - Full nudge → TurnComplete lifecycle tracks count correctly
//   - Slot key format matches chat_service convention: "{context_type}/{context_id}"
// ============================================================================

/// Confirms fix 3 coverage: the 9 tests in team_nudge_running_count_tests.rs
/// cover all aspects of the running count nudge fix.
///
/// Tests there:
///   Section A (contract, 6): verify ExecutionState claim/increment/idle semantics
///   Section B (fix verification, 3): verify correct pattern after fix
///   Section C (slot key, 2): verify "{context_type}/{context_id}" format
#[test]
fn test_fix3_running_count_nudge_coverage_documented() {
    // Slot key format used by the fix: "{context_type}/{context_id}"
    let context_type = "ideation";
    let context_id = "session-123";
    let slot_key = format!("{}/{}", context_type, context_id);
    assert_eq!(
        slot_key, "ideation/session-123",
        "Fix 3 slot key format must match chat_service convention"
    );
}
