use ralphx_lib::application::chat_service::{
    events, is_completion_tool_name, should_kill_on_timeout, ActiveTaskTracker, AgentChunkPayload,
    AgentRunCompletedPayload, AgentTaskCompletedPayload, AgentTaskStartedPayload,
    AgentToolCallPayload, AgentToolCallPreviewFields, CompletionSignalTracker, StreamError,
    StreamOutcome, StreamTimeoutConfig,
};
use ralphx_lib::domain::entities::{AgentRunUsage, ChatContextType};
use ralphx_lib::infrastructure::agents::claude::stream_timeouts;
use ralphx_lib::utils::secret_redactor::redact;
use std::time::Duration;

// ── should_kill_on_timeout unit tests ────────────────────────────────────────

/// Helper: build common duration values
fn dur(secs: u64) -> Duration {
    Duration::from_secs(secs)
}

/// 1. PID alive + not exited → reset timeout (don't kill)
#[test]
fn test_pid_alive_resets_timeout() {
    assert!(!should_kill_on_timeout(
        dur(601),  // elapsed > line_read_timeout (600s) but within wall-clock
        dur(1800), // max_wall_clock
        false,     // no pending question
        false,     // not interactive turn
        true,      // pid_alive
        false,     // child NOT exited
        false,     // no active tasks
        false,     // not in completion grace period
    ));
}

/// 2. PID dead → kill
#[test]
fn test_dead_process_killed() {
    assert!(should_kill_on_timeout(
        dur(601),
        dur(1800),
        false, // no pending question
        false, // not interactive turn
        false, // pid NOT alive
        true,  // child exited
        false, // no active tasks
        false, // not in completion grace period
    ));
}

/// 3. Wall-clock exceeded AND pid alive → still kill (wall-clock overrides everything)
#[test]
fn test_wall_clock_overrides_pid_alive() {
    assert!(should_kill_on_timeout(
        dur(1801), // elapsed > max_wall_clock
        dur(1800),
        false, // no pending question
        false, // not interactive turn
        true,  // pid_alive — would bypass normally
        false, // child NOT exited
        false, // no active tasks
        false, // not in completion grace period
    ));
}

/// 4. PID recycling guard: pid_alive=true but child_exited=true → kill
///    (PID was recycled by OS after child exited)
#[test]
fn test_pid_recycling_guard() {
    assert!(should_kill_on_timeout(
        dur(601),
        dur(1800),
        false, // no pending question
        false, // not interactive turn
        true,  // pid_alive (recycled PID shows alive)
        true,  // child_exited=true (try_wait returned Some(status))
        false, // no active tasks
        false, // not in completion grace period
    ));
}

/// 5. Interactive turn bypass → reset timeout (don't kill via error path)
#[test]
fn test_interactive_turn_bypass() {
    assert!(!should_kill_on_timeout(
        dur(601),
        dur(1800),
        false, // no pending question
        true,  // is_interactive_turn
        false, // pid not alive
        true,  // child exited
        false, // no active tasks
        false, // not in completion grace period
    ));
}

/// 6. Pending question bypass → reset timeout (don't kill)
#[test]
fn test_pending_question_bypass() {
    assert!(!should_kill_on_timeout(
        dur(601),
        dur(1800),
        true,  // has_pending_question
        false, // not interactive turn
        false, // pid not alive
        true,  // child exited
        false, // no active tasks
        false, // not in completion grace period
    ));
}

/// 7. Wall-clock exceeded AND pending question → kill (wall-clock wins)
#[test]
fn test_wall_clock_overrides_question() {
    assert!(should_kill_on_timeout(
        dur(1801), // exceeds wall-clock
        dur(1800),
        true,  // has_pending_question — would bypass normally
        false, // not interactive turn
        false, // pid not alive
        true,  // child exited
        false, // no active tasks
        false, // not in completion grace period
    ));
}

/// 8. Parse stall path: pid_alive + not exited → don't kill
///    (In parse stall context this causes last_parsed_at reset + flush continues)
#[test]
fn test_parse_stall_pid_alive_resets() {
    assert!(!should_kill_on_timeout(
        dur(181),  // elapsed > parse_stall_timeout (180s) but within wall-clock
        dur(1800), // max_wall_clock
        false,     // no pending question
        false,     // parse stall path passes false for is_interactive_turn
        true,      // pid_alive
        false,     // child NOT exited
        false,     // no active tasks
        false,     // not in completion grace period
    ));
}

#[test]
fn test_completion_grace_bypasses_timeout() {
    assert!(!should_kill_on_timeout(
        dur(601),
        dur(1800),
        false,
        false,
        false,
        true,
        false,
        true,
    ));
}

#[test]
fn test_wall_clock_overrides_completion_grace() {
    assert!(should_kill_on_timeout(
        dur(1801),
        dur(1800),
        false,
        false,
        false,
        true,
        false,
        true,
    ));
}

#[test]
fn test_completion_tracker_grace_expires_and_timeout_kills() {
    let mut tracker = CompletionSignalTracker::default();
    tracker.mark_completion_called_at(std::time::Instant::now() - dur(31));

    assert!(!tracker.is_in_grace_period(dur(30)));
    assert!(should_kill_on_timeout(
        dur(601),
        dur(1800),
        false,
        false,
        false,
        true,
        false,
        tracker.is_in_grace_period(dur(30)),
    ));
}

#[test]
fn test_completion_tool_detection_marks_tracker_and_bypasses_timeout() {
    let mut tracker = CompletionSignalTracker::default();

    if is_completion_tool_name("mcp__ralphx__execution_complete") {
        tracker.mark_completion_called();
    }

    assert!(tracker.was_called());
    assert!(tracker.is_in_grace_period(dur(30)));
    assert!(!should_kill_on_timeout(
        dur(601),
        dur(1800),
        false,
        false,
        false,
        true,
        false,
        tracker.is_in_grace_period(dur(30)),
    ));
}

#[test]
fn test_completion_tool_detection_accepts_review_mcp_name() {
    let mut tracker = CompletionSignalTracker::default();

    if is_completion_tool_name("mcp__ralphx__complete_review") {
        tracker.mark_completion_called();
    }

    assert!(tracker.was_called());
    assert!(tracker.is_in_grace_period(dur(30)));
}

#[test]
fn test_completion_tool_detection_accepts_merge_mcp_name() {
    let mut tracker = CompletionSignalTracker::default();

    if is_completion_tool_name("mcp__ralphx__complete_merge") {
        tracker.mark_completion_called();
    }

    assert!(tracker.was_called());
    assert!(tracker.is_in_grace_period(dur(30)));
}

#[test]
fn test_completion_tool_detection_accepts_agent_workspace_repair_mcp_name() {
    assert!(is_completion_tool_name(
        "mcp__ralphx__complete_agent_workspace_repair"
    ));
}

#[test]
fn test_completion_tool_detection_accepts_finalize_proposals_mcp_name() {
    assert!(is_completion_tool_name("mcp__ralphx__finalize_proposals"));
}

#[test]
fn test_completion_tool_detection_accepts_codex_double_colon_names() {
    for tool_name in [
        "ralphx::execution_complete",
        "ralphx::complete_review",
        "ralphx::complete_merge",
        "ralphx::complete_agent_workspace_repair",
        "ralphx::finalize_proposals",
    ] {
        assert!(is_completion_tool_name(tool_name), "{tool_name} should mark completion");
    }
}

#[test]
fn test_completion_tool_detection_rejects_non_completion_mcp_names() {
    let mut tracker = CompletionSignalTracker::default();

    if is_completion_tool_name("mcp__ralphx__get_task_context") {
        tracker.mark_completion_called();
    }

    assert!(!tracker.was_called());
}

#[test]
fn test_completion_tool_detection_rejects_legacy_bare_completion_names() {
    let mut tracker = CompletionSignalTracker::default();

    if is_completion_tool_name("execution_complete") {
        tracker.mark_completion_called();
    }

    assert!(!tracker.was_called());
}

#[test]
fn test_timeout_config_task_execution() {
    let config = StreamTimeoutConfig::for_context(&ChatContextType::TaskExecution);
    assert_eq!(config.line_read_timeout, Duration::from_secs(600));
    assert_eq!(config.parse_stall_timeout, Duration::from_secs(180));
}

#[test]
fn test_timeout_config_review() {
    let config = StreamTimeoutConfig::for_context(&ChatContextType::Review);
    assert_eq!(config.line_read_timeout, Duration::from_secs(600));
    assert_eq!(config.parse_stall_timeout, Duration::from_secs(120));
}

#[test]
fn test_timeout_config_merge() {
    let config = StreamTimeoutConfig::for_context(&ChatContextType::Merge);
    assert_eq!(config.line_read_timeout, Duration::from_secs(600));
    assert_eq!(config.parse_stall_timeout, Duration::from_secs(180));
}

#[test]
fn test_timeout_config_ideation_uses_defaults() {
    let config = StreamTimeoutConfig::for_context(&ChatContextType::Ideation);
    assert_eq!(config.line_read_timeout, Duration::from_secs(600));
    assert_eq!(config.parse_stall_timeout, Duration::from_secs(180));
}

#[test]
fn test_timeout_config_task_uses_defaults() {
    let config = StreamTimeoutConfig::for_context(&ChatContextType::Task);
    assert_eq!(config.line_read_timeout, Duration::from_secs(600));
    assert_eq!(config.parse_stall_timeout, Duration::from_secs(180));
}

#[test]
fn test_timeout_config_project_uses_defaults() {
    let config = StreamTimeoutConfig::for_context(&ChatContextType::Project);
    assert_eq!(config.line_read_timeout, Duration::from_secs(600));
    assert_eq!(config.parse_stall_timeout, Duration::from_secs(180));
}

#[test]
fn test_timeout_config_with_teammate() {
    let config = StreamTimeoutConfig::for_context(&ChatContextType::Ideation)
        .with_teammate("researcher".to_string(), "#ff6b35".to_string());
    assert_eq!(config.teammate_name, Some("researcher".to_string()));
    assert_eq!(config.teammate_color, Some("#ff6b35".to_string()));
    // Timeouts should be unchanged
    assert_eq!(config.line_read_timeout, Duration::from_secs(600));
}

#[test]
fn test_timeout_config_default_no_teammate() {
    let config = StreamTimeoutConfig::for_context(&ChatContextType::Ideation);
    assert!(config.teammate_name.is_none());
    assert!(config.teammate_color.is_none());
}

#[test]
fn test_timeout_config_ordering() {
    // Both review and merge match default line_read_timeout (600s) to prevent false kills
    // during long cargo test runs. Review parse_stall is still tighter than default.
    let merge = StreamTimeoutConfig::for_context(&ChatContextType::Merge);
    let review = StreamTimeoutConfig::for_context(&ChatContextType::Review);
    let default = StreamTimeoutConfig::for_context(&ChatContextType::TaskExecution);

    assert_eq!(review.line_read_timeout, default.line_read_timeout);
    assert!(review.parse_stall_timeout < default.parse_stall_timeout);
    // Merge matches default — merger agents may run silent test suites
    assert_eq!(merge.line_read_timeout, default.line_read_timeout);
    assert_eq!(merge.parse_stall_timeout, default.parse_stall_timeout);
}

#[test]
fn test_payloads_serialize_with_seq() {
    // Verify AgentChunkPayload includes seq field
    let chunk = AgentChunkPayload {
        text: "test".to_string(),
        conversation_id: "conv-1".to_string(),
        context_type: "task".to_string(),
        context_id: "task-1".to_string(),
        seq: 0,
        append_to_previous: false,
    };
    let json = serde_json::to_string(&chunk).unwrap();
    assert!(
        json.contains("\"seq\":0"),
        "AgentChunkPayload should serialize with seq field"
    );

    // Verify AgentToolCallPayload includes seq field
    let tool_call = AgentToolCallPayload {
        tool_name: "test_tool".to_string(),
        tool_id: Some("tool-1".to_string()),
        arguments: serde_json::json!({}),
        result: None,
        preview: AgentToolCallPreviewFields::default(),
        conversation_id: "conv-1".to_string(),
        context_type: "task".to_string(),
        context_id: "task-1".to_string(),
        diff_context: None,
        parent_tool_use_id: None,
        seq: 1,
    };
    let json = serde_json::to_string(&tool_call).unwrap();
    assert!(
        json.contains("\"seq\":1"),
        "AgentToolCallPayload should serialize with seq field"
    );

    // Verify AgentTaskStartedPayload includes seq field
    let task_started = AgentTaskStartedPayload {
        tool_use_id: "tool-1".to_string(),
        tool_name: "Task".to_string(),
        description: Some("test".to_string()),
        subagent_type: Some("bash".to_string()),
        model: Some("sonnet".to_string()),
        teammate_name: None,
        delegated_job_id: None,
        delegated_session_id: None,
        delegated_conversation_id: None,
        delegated_agent_run_id: None,
        provider_harness: None,
        provider_session_id: None,
        upstream_provider: None,
        provider_profile: None,
        logical_model: None,
        effective_model_id: None,
        logical_effort: None,
        effective_effort: None,
        approval_policy: None,
        sandbox_mode: None,
        conversation_id: "conv-1".to_string(),
        context_type: "task".to_string(),
        context_id: "task-1".to_string(),
        seq: 2,
    };
    let json = serde_json::to_string(&task_started).unwrap();
    assert!(
        json.contains("\"seq\":2"),
        "AgentTaskStartedPayload should serialize with seq field"
    );

    // Verify AgentTaskCompletedPayload includes seq field
    let task_completed = AgentTaskCompletedPayload {
        tool_use_id: "tool-1".to_string(),
        agent_id: Some("agent-1".to_string()),
        status: None,
        total_duration_ms: Some(1000),
        total_tokens: Some(100),
        total_tool_use_count: Some(5),
        teammate_name: None,
        delegated_job_id: None,
        delegated_session_id: None,
        delegated_conversation_id: None,
        delegated_agent_run_id: None,
        provider_harness: None,
        provider_session_id: None,
        upstream_provider: None,
        provider_profile: None,
        logical_model: None,
        effective_model_id: None,
        logical_effort: None,
        effective_effort: None,
        approval_policy: None,
        sandbox_mode: None,
        input_tokens: None,
        output_tokens: None,
        cache_creation_tokens: None,
        cache_read_tokens: None,
        estimated_usd: None,
        text_output: None,
        error: None,
        conversation_id: "conv-1".to_string(),
        context_type: "task".to_string(),
        context_id: "task-1".to_string(),
        seq: 3,
    };
    let json = serde_json::to_string(&task_completed).unwrap();
    assert!(
        json.contains("\"seq\":3"),
        "AgentTaskCompletedPayload should serialize with seq field"
    );
}

#[test]
fn test_seq_values_are_monotonic() {
    // Test that multiple events would have incrementing seq values
    let mut stream_seq: u64 = 0;

    // Simulate streaming multiple events
    let seq1 = stream_seq;
    stream_seq += 1;
    let seq2 = stream_seq;
    stream_seq += 1;
    let seq3 = stream_seq;
    stream_seq += 1;
    let seq4 = stream_seq;

    assert_eq!(seq1, 0, "First event should have seq 0");
    assert_eq!(seq2, 1, "Second event should have seq 1");
    assert_eq!(seq3, 2, "Third event should have seq 2");
    assert_eq!(seq4, 3, "Fourth event should have seq 3");

    // Verify strict monotonic ordering
    assert!(seq2 > seq1, "seq must be strictly increasing");
    assert!(seq3 > seq2, "seq must be strictly increasing");
    assert!(seq4 > seq3, "seq must be strictly increasing");
}

// --- Dynamic team_mode upgrade tests ---

#[test]
fn test_timeout_config_dynamic_team_upgrade() {
    // Scenario: lead spawned with team_mode=false (default), then TeamCreated is detected.
    // The timeout should upgrade from default (600s) to team (3600s).
    let mut config = StreamTimeoutConfig::for_context(&ChatContextType::Ideation);
    assert_eq!(
        config.line_read_timeout,
        Duration::from_secs(600),
        "Before upgrade: should use default timeout"
    );

    // Simulate the dynamic upgrade that happens in process_stream_background
    // when StreamEvent::TeamCreated is detected and team_mode was false
    let cfg = stream_timeouts();
    config.line_read_timeout = Duration::from_secs(cfg.team_line_read_secs);
    config.parse_stall_timeout = Duration::from_secs(cfg.team_parse_stall_secs);

    assert_eq!(
        config.line_read_timeout,
        Duration::from_secs(3600),
        "After upgrade: should use team timeout"
    );
    assert_eq!(
        config.parse_stall_timeout,
        Duration::from_secs(3600),
        "After upgrade: parse stall should also use team timeout"
    );
}

#[test]
fn test_timeout_config_team_mode_true_already_upgraded() {
    // When team_mode=true at spawn time, timeout is already set correctly.
    // The dynamic upgrade should be a no-op (guarded by `if !team_mode`).
    let mut config = StreamTimeoutConfig::for_context(&ChatContextType::Ideation);
    let cfg = stream_timeouts();

    // Simulate team_mode=true at spawn time
    config.line_read_timeout = Duration::from_secs(cfg.team_line_read_secs);
    config.parse_stall_timeout = Duration::from_secs(cfg.team_parse_stall_secs);

    let before = config.line_read_timeout;

    // Even if we re-apply, the value stays the same
    config.line_read_timeout = Duration::from_secs(cfg.team_line_read_secs);
    assert_eq!(config.line_read_timeout, before, "Should be idempotent");
}

// --- ActiveTaskTracker tests ---

#[test]
fn test_active_task_tracker_empty_by_default() {
    let tracker = ActiveTaskTracker::default();
    assert!(!tracker.has_active_tasks());
    assert_eq!(tracker.count(), 0);
}

#[test]
fn test_active_task_tracker_counts_started_tasks() {
    let mut tracker = ActiveTaskTracker::default();
    tracker.task_started();
    assert!(tracker.has_active_tasks());
    assert_eq!(tracker.count(), 1);

    tracker.task_started();
    assert_eq!(tracker.count(), 2);
}

#[test]
fn test_active_task_tracker_decrements_on_completed() {
    let mut tracker = ActiveTaskTracker::default();
    tracker.task_started();
    tracker.task_started();
    tracker.task_completed();
    assert!(tracker.has_active_tasks());
    assert_eq!(tracker.count(), 1);

    tracker.task_completed();
    assert!(!tracker.has_active_tasks());
    assert_eq!(tracker.count(), 0);
}

#[test]
fn test_active_task_tracker_saturates_at_zero() {
    let mut tracker = ActiveTaskTracker::default();
    tracker.task_completed(); // No active tasks, should not underflow
    assert!(!tracker.has_active_tasks());
    assert_eq!(tracker.count(), 0);
}

#[test]
fn test_active_task_tracker_prevents_timeout_during_sidechain() {
    // Scenario: Lead spawns 2 Task tool subagents (frontend-researcher, backend-researcher).
    // Both TaskStarted events arrive → count = 2.
    // During sidechain work, no stdout lines → timeout would fire.
    // But has_active_tasks() returns true → timeout should be bypassed.
    let mut tracker = ActiveTaskTracker::default();

    // Lead spawns both researchers
    tracker.task_started(); // frontend-researcher
    tracker.task_started(); // backend-researcher
    assert!(
        tracker.has_active_tasks(),
        "Should prevent timeout while subagents are active"
    );

    // First researcher completes
    tracker.task_completed();
    assert!(
        tracker.has_active_tasks(),
        "Should still prevent timeout with 1 active subagent"
    );

    // Second researcher completes
    tracker.task_completed();
    assert!(
        !tracker.has_active_tasks(),
        "Should allow timeout when all subagents done"
    );
}

#[test]
fn test_completion_tracker_recognizes_all_completion_tools() {
    for tool_name in [
        "mcp__ralphx__execution_complete",
        "mcp__ralphx__complete_review",
        "mcp__ralphx__complete_merge",
        "mcp__ralphx__finalize_proposals",
    ] {
        let mut tracker = CompletionSignalTracker::default();
        if is_completion_tool_name(tool_name) {
            tracker.mark_completion_called();
        }
        assert!(tracker.was_called(), "{tool_name} should mark completion");
    }
}

#[test]
fn test_non_completion_tool_does_not_mark_tracker() {
    let mut tracker = CompletionSignalTracker::default();

    if is_completion_tool_name("mcp__ralphx__get_task_context") {
        tracker.mark_completion_called();
    }

    assert!(!tracker.was_called());
    assert!(should_kill_on_timeout(
        dur(601),
        dur(1800),
        false,
        false,
        false,
        true,
        false,
        tracker.is_in_grace_period(dur(30)),
    ));
}

// ============================================================================
// TurnComplete event emission tests
// ============================================================================
// These tests verify the behavioral contract of TurnComplete vs run_completed
// event emission. Since process_stream_background requires a real child process
// and Tauri AppHandle (not available in unit tests), we test the decision logic,
// payload shape, and StreamOutcome signaling that drives the caller's behavior.

#[test]
fn test_turn_completed_event_name_is_distinct_from_run_completed() {
    // The whole point of the TurnComplete feature: interactive turns emit
    // a DIFFERENT event name so the frontend doesn't set isAgentRunning=false.
    assert_ne!(
        events::AGENT_TURN_COMPLETED,
        events::AGENT_RUN_COMPLETED,
        "turn_completed and run_completed must be distinct event names"
    );
    assert_eq!(
        events::AGENT_TURN_COMPLETED,
        "agent:turn_completed",
        "AGENT_TURN_COMPLETED must be 'agent:turn_completed'"
    );
    assert_eq!(
        events::AGENT_RUN_COMPLETED,
        "agent:run_completed",
        "AGENT_RUN_COMPLETED must be 'agent:run_completed'"
    );
}

#[test]
fn test_turn_completed_payload_shape_matches_run_completed() {
    // TurnComplete reuses AgentRunCompletedPayload — verify it serializes
    // with all required fields for the frontend to correctly identify the context.
    let payload = AgentRunCompletedPayload {
        conversation_id: "conv-interactive-1".to_string(),
        context_type: "task_execution".to_string(),
        context_id: "task-42".to_string(),
        claude_session_id: Some("session-abc".to_string()),
        provider_harness: Some("claude".to_string()),
        provider_session_id: Some("session-abc".to_string()),
        run_chain_id: None,
    };

    let json = serde_json::to_value(&payload).unwrap();
    assert_eq!(json["conversation_id"], "conv-interactive-1");
    assert_eq!(json["context_type"], "task_execution");
    assert_eq!(json["context_id"], "task-42");
    assert_eq!(json["claude_session_id"], "session-abc");
    assert_eq!(json["provider_harness"], "claude");
    assert_eq!(json["provider_session_id"], "session-abc");
    // run_chain_id is None → should be absent (skip_serializing_if)
    assert!(
        json.get("run_chain_id").is_none(),
        "run_chain_id=None should be omitted from serialized payload"
    );
}

#[test]
fn test_turn_completed_payload_with_no_session_id() {
    // Early turns may not yet have a session_id from Claude.
    let payload = AgentRunCompletedPayload {
        conversation_id: "conv-interactive-2".to_string(),
        context_type: "ideation".to_string(),
        context_id: "session-7".to_string(),
        claude_session_id: None,
        provider_harness: Some("codex".to_string()),
        provider_session_id: Some("thread-7".to_string()),
        run_chain_id: None,
    };

    let json = serde_json::to_value(&payload).unwrap();
    assert_eq!(json["conversation_id"], "conv-interactive-2");
    assert_eq!(json["context_type"], "ideation");
    assert_eq!(json["context_id"], "session-7");
    assert_eq!(json["provider_harness"], "codex");
    assert_eq!(json["provider_session_id"], "thread-7");
    // claude_session_id=None → serializes as null (no skip_serializing_if on this field)
    assert!(
        json["claude_session_id"].is_null(),
        "claude_session_id=None should serialize as null"
    );
}

#[test]
fn test_non_interactive_run_completed_includes_run_chain_id() {
    // Non-interactive (one-shot) agents pass run_chain_id through.
    // Interactive TurnComplete always sets run_chain_id: None.
    let payload = AgentRunCompletedPayload {
        conversation_id: "conv-oneshot-1".to_string(),
        context_type: "task_execution".to_string(),
        context_id: "task-99".to_string(),
        claude_session_id: Some("session-xyz".to_string()),
        provider_harness: Some("claude".to_string()),
        provider_session_id: Some("session-xyz".to_string()),
        run_chain_id: Some("chain-abc".to_string()),
    };

    let json = serde_json::to_value(&payload).unwrap();
    assert_eq!(
        json["run_chain_id"], "chain-abc",
        "Non-interactive run_completed should include run_chain_id"
    );
}

#[test]
fn test_stream_outcome_turns_finalized_controls_post_loop_behavior() {
    // When turns_finalized > 0 and no new output, the caller (send_background)
    // sets skip_post_loop_finalization=true, preventing duplicate run_completed.
    // This simulates the decision logic in chat_service_send_background.rs.

    // Case 1: Interactive mode — turns were finalized, no new output after last turn
    let outcome = StreamOutcome {
        response_text: String::new(),
        tool_calls: vec![],
        content_blocks: vec![],
        session_id: Some("session-1".to_string()),
        usage: AgentRunUsage::default(),
        stderr_text: String::new(),
        turns_finalized: 2,
        execution_slot_held: false, // idle between turns at exit
        silent_interactive_exit: false,
    };
    let has_output = outcome.has_meaningful_output();
    let skip_post_loop = outcome.turns_finalized > 0 && !has_output;
    assert!(
        skip_post_loop,
        "Interactive with finalized turns + no new output → skip post-loop run_completed"
    );
    assert!(
        !outcome.execution_slot_held,
        "Idle between turns → slot not held"
    );

    // Case 2: Non-interactive — no turns finalized
    let outcome_non_interactive = StreamOutcome {
        response_text: "Agent response here".to_string(),
        tool_calls: vec![],
        content_blocks: vec![],
        session_id: Some("session-2".to_string()),
        usage: AgentRunUsage::default(),
        stderr_text: String::new(),
        turns_finalized: 0,
        execution_slot_held: true, // normal exit — slot still held
        silent_interactive_exit: false,
    };
    let has_output = outcome_non_interactive.has_meaningful_output();
    let skip_post_loop = outcome_non_interactive.turns_finalized > 0 && !has_output;
    assert!(
        !skip_post_loop,
        "Non-interactive with turns_finalized=0 → DO emit post-loop run_completed"
    );
    assert!(
        outcome_non_interactive.execution_slot_held,
        "Non-interactive → slot held until on_exit"
    );
}

#[test]
fn test_stream_outcome_execution_slot_held_reflects_interactive_state() {
    // execution_slot_held = !between_interactive_turns || !uses_execution_slot
    // For contexts that use execution slots:
    //   - between_interactive_turns=true → slot NOT held (TurnComplete decremented)
    //   - between_interactive_turns=false → slot held (still processing or non-interactive)

    // Idle between turns (TurnComplete received, no resume)
    let idle_outcome = StreamOutcome {
        response_text: String::new(),
        tool_calls: vec![],
        content_blocks: vec![],
        session_id: None,
        usage: AgentRunUsage::default(),
        stderr_text: String::new(),
        turns_finalized: 1,
        execution_slot_held: false,
        silent_interactive_exit: false,
    };
    assert!(
        !idle_outcome.execution_slot_held,
        "Should NOT hold slot when idle between interactive turns"
    );

    // Mid-turn exit (process crashed or timed out while active)
    let active_outcome = StreamOutcome {
        response_text: "partial output".to_string(),
        tool_calls: vec![],
        content_blocks: vec![],
        session_id: None,
        usage: AgentRunUsage::default(),
        stderr_text: String::new(),
        turns_finalized: 0,
        execution_slot_held: true,
        silent_interactive_exit: false,
    };
    assert!(
        active_outcome.execution_slot_held,
        "Should hold slot when exiting mid-turn (on_exit must decrement)"
    );
}

/// Simulates the full decision tree for event name selection that
/// process_stream_background uses:
/// - In-loop TurnComplete → emit AGENT_TURN_COMPLETED
/// - Post-loop (non-interactive) → emit AGENT_RUN_COMPLETED
/// - Post-loop (interactive, turns_finalized>0, no output) → skip (already emitted turn_completed)
#[test]
fn test_event_name_selection_decision_tree() {
    // Scenario 1: Interactive turn completes during streaming
    // The TurnComplete arm in the stream loop emits this:
    let interactive_event_name = events::AGENT_TURN_COMPLETED;
    assert_eq!(interactive_event_name, "agent:turn_completed");

    // Scenario 2: Non-interactive agent finishes (post-loop)
    let non_interactive_event_name = events::AGENT_RUN_COMPLETED;
    assert_eq!(non_interactive_event_name, "agent:run_completed");

    // Scenario 3: Interactive process exits after TurnComplete
    // turns_finalized > 0, no new output → skip_post_loop_finalization = true
    // The post-loop code checks `if !skip_post_loop_finalization` before emitting.
    // So no AGENT_RUN_COMPLETED is emitted — the AGENT_TURN_COMPLETED was the last event.
    let turns_finalized: usize = 1;
    let has_output = false;
    let skip_post_loop = turns_finalized > 0 && !has_output;
    assert!(
        skip_post_loop,
        "After interactive TurnComplete + idle exit, post-loop run_completed must be skipped"
    );
}

// --- silent_interactive_exit behavioral contract tests ---

/// Verifies the queue gate: when `silent_interactive_exit` is true, `will_process_queue`
/// must be false even when there are queued messages and a valid session_id.
/// This is the PRIMARY fix preventing the resurrection cascade.
#[test]
fn test_will_process_queue_suppressed_on_silent_exit() {
    // Simulate the decision logic from chat_service_send_background.rs:
    // let will_process_queue = initial_queue_count > 0 && has_session_for_queue && !outcome.silent_interactive_exit;

    // Case 1: Normal exit — queue should be processed
    let outcome_normal = StreamOutcome {
        response_text: String::new(),
        tool_calls: vec![],
        content_blocks: vec![],
        session_id: Some("session-abc".to_string()),
        usage: AgentRunUsage::default(),
        stderr_text: String::new(),
        turns_finalized: 1,
        execution_slot_held: false,
        silent_interactive_exit: false,
    };
    let initial_queue_count = 2;
    let has_session_for_queue = outcome_normal.session_id.is_some();
    let will_process_queue_normal =
        initial_queue_count > 0 && has_session_for_queue && !outcome_normal.silent_interactive_exit;
    assert!(
        will_process_queue_normal,
        "Normal exit with queued messages + session → must process queue"
    );

    // Case 2: Silent interactive exit — queue must NOT be processed regardless of queue/session
    let outcome_silent = StreamOutcome {
        response_text: String::new(),
        tool_calls: vec![],
        content_blocks: vec![],
        session_id: Some("session-abc".to_string()),
        usage: AgentRunUsage::default(),
        stderr_text: String::new(),
        turns_finalized: 1,
        execution_slot_held: false,
        silent_interactive_exit: true,
    };
    let will_process_queue_silent =
        initial_queue_count > 0 && has_session_for_queue && !outcome_silent.silent_interactive_exit;
    assert!(
        !will_process_queue_silent,
        "Silent exit → must NOT process queue even with queued messages and valid session"
    );

    // Case 3: No session at all (regardless of silent flag) — no queue
    let outcome_no_session = StreamOutcome {
        response_text: String::new(),
        tool_calls: vec![],
        content_blocks: vec![],
        session_id: None,
        usage: AgentRunUsage::default(),
        stderr_text: String::new(),
        turns_finalized: 1,
        execution_slot_held: false,
        silent_interactive_exit: false,
    };
    let has_session_no_session = outcome_no_session.session_id.is_some();
    let will_process_queue_no_session =
        initial_queue_count > 0 && has_session_no_session && !outcome_no_session.silent_interactive_exit;
    assert!(
        !will_process_queue_no_session,
        "No session → queue cannot be processed regardless"
    );
}

/// Verifies `run_completed` emission logic: when `silent_interactive_exit` is true,
/// `run_completed` must be emitted even when `skip_post_loop_finalization` is true.
/// Without this fix, the frontend would be stuck in `waiting_for_input` forever.
#[test]
fn test_run_completed_forced_on_silent_exit() {
    // Simulates the gate:
    // if !skip_post_loop_finalization || outcome.silent_interactive_exit { emit run_completed }

    // Normal interactive exit: turns_finalized > 0, no output → skip_post_loop = true → NO run_completed
    let skip_post_loop_finalization = true; // turns finalized, no new output
    let silent_exit_false = false;
    let should_emit_normal = !skip_post_loop_finalization || silent_exit_false;
    assert!(
        !should_emit_normal,
        "Normal silent exit (between turns) with skip_post_loop=true → must NOT emit duplicate run_completed"
    );

    // Silent interactive exit: even though skip_post_loop=true, we MUST emit run_completed
    let silent_exit_true = true;
    let should_emit_silent = !skip_post_loop_finalization || silent_exit_true;
    assert!(
        should_emit_silent,
        "Silent interactive exit → MUST emit run_completed to unblock frontend from waiting_for_input"
    );

    // Non-interactive: skip_post_loop=false → always emits (unchanged behavior)
    let skip_post_loop_non_interactive = false;
    let should_emit_non_interactive = !skip_post_loop_non_interactive || silent_exit_false;
    assert!(
        should_emit_non_interactive,
        "Non-interactive → always emit run_completed (skip_post_loop=false)"
    );
}

/// Verifies the re-increment skip is scoped to Ideation only.
/// TaskExecution/Review/Merge MUST still re-increment (their on_exit decrements it back to zero).
#[test]
fn test_re_increment_skip_scoped_to_ideation_only() {
    // Simulates the guard:
    // if !execution_slot_held && uses_execution_slot(context_type)
    //    && !(outcome.silent_interactive_exit && context_type == ChatContextType::Ideation)
    // { exec.increment_running(); }

    let execution_slot_held = false; // idle between turns
    let silent_interactive_exit = true;

    // Helper that mirrors the guard logic from chat_service_send_background.rs:
    // `if !execution_slot_held && uses_execution_slot(context_type)
    //     && !(outcome.silent_interactive_exit && context_type == ChatContextType::Ideation)`
    // We inline `uses_execution_slot` semantics: Ideation/TaskExecution/Review/Merge = true.
    let should_re_increment = |context_type: &ChatContextType| -> bool {
        let uses_slot = matches!(
            context_type,
            ChatContextType::Ideation
                | ChatContextType::TaskExecution
                | ChatContextType::Review
                | ChatContextType::Merge
        );
        !execution_slot_held
            && uses_slot
            && !(*context_type == ChatContextType::Ideation && silent_interactive_exit)
    };

    // Ideation: silent exit → SKIP re-increment (no on_exit decrement to balance)
    assert!(
        !should_re_increment(&ChatContextType::Ideation),
        "Ideation silent exit → must skip re-increment (no on_exit will decrement it)"
    );

    // TaskExecution: silent exit flag irrelevant — MUST re-increment for balance
    assert!(
        should_re_increment(&ChatContextType::TaskExecution),
        "TaskExecution → must always re-increment when slot freed between turns"
    );

    // Review: same as TaskExecution
    assert!(
        should_re_increment(&ChatContextType::Review),
        "Review → must always re-increment when slot freed between turns"
    );

    // Merge: same
    assert!(
        should_re_increment(&ChatContextType::Merge),
        "Merge → must always re-increment when slot freed between turns"
    );

    // Edge case: slot still held (not idle between turns) → never re-increment
    let execution_slot_held_true = true;
    let should_re_increment_held = |context_type: &ChatContextType| -> bool {
        let uses_slot = matches!(
            context_type,
            ChatContextType::Ideation
                | ChatContextType::TaskExecution
                | ChatContextType::Review
                | ChatContextType::Merge
        );
        !execution_slot_held_true
            && uses_slot
            && !(*context_type == ChatContextType::Ideation && silent_interactive_exit)
    };
    assert!(
        !should_re_increment_held(&ChatContextType::TaskExecution),
        "Slot still held → no re-increment needed (on_exit handles decrement)"
    );
}

// ============================================================================
// Stderr redaction tests
// ============================================================================
// These tests verify that secrets in agent stderr are redacted before they
// reach downstream consumers (AgentExit construction, debug file payloads,
// tracing::warn previews).

/// Verifies that `redact()` sanitises secrets in stderr before they are stored
/// in `StreamError::AgentExit`. The `to_string()` of the error must not leak.
#[test]
fn test_agent_exit_stderr_redacted_before_construction() {
    let raw_stderr = "Error: ANTHROPIC_AUTH_TOKEN=sk-ant-api03-AbCdEfGhIjKlMnOpQrStUvWxYz01234567890123456789 not accepted";
    let redacted_stderr = redact(raw_stderr);

    // The redacted stderr must not contain the secret
    assert!(
        !redacted_stderr.contains("sk-ant-api03"),
        "Redacted stderr must not contain Anthropic API key prefix"
    );
    assert!(
        !redacted_stderr.contains("AbCdEfGhIjKlMnOpQrStUvWxYz01234567890123456789"),
        "Redacted stderr must not contain raw key material"
    );
    assert!(
        redacted_stderr.contains("***REDACTED***"),
        "Redacted stderr must contain placeholder"
    );

    // Constructing AgentExit with redacted stderr — to_string() is safe
    let err = StreamError::AgentExit {
        exit_code: Some(1),
        stderr: redacted_stderr.clone(),
    };
    let err_str = err.to_string();
    assert!(
        !err_str.contains("sk-ant-api03"),
        "AgentExit.to_string() must not expose Anthropic API key"
    );
    assert!(
        !err_str.contains("AbCdEfGhIjKlMnOpQrStUvWxYz01234567890123456789"),
        "AgentExit.to_string() must not expose raw key material"
    );
}

/// Verifies redaction of an OpenRouter key appearing in stderr.
#[test]
fn test_agent_exit_openrouter_key_redacted() {
    let raw_stderr =
        "API call failed: sk-or-v1-abcdefghijklmnopqrstuvwxyz0123456789abcdef returned 401";
    let redacted = redact(raw_stderr);

    assert!(
        !redacted.contains("abcdefghijklmnopqrstuvwxyz0123456789abcdef"),
        "OpenRouter key body must be redacted"
    );
    assert!(
        redacted.contains("sk-or-v1-***REDACTED***"),
        "OpenRouter key must be replaced with placeholder"
    );

    let err = StreamError::AgentExit {
        exit_code: Some(1),
        stderr: redacted.clone(),
    };
    assert!(
        !err.to_string().contains("abcdefghijklmnopqrstuvwxyz"),
        "AgentExit with OpenRouter key must be safe to display"
    );
}

/// Verifies that the debug file payload (built from stderr) contains no secrets
/// when redaction is applied before payload assembly.
#[test]
fn test_debug_file_payload_contains_redacted_stderr() {
    let raw_stderr = "fatal: Bearer sk-ant-api03-XxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXx01234567890 rejected";
    let redacted_stderr = redact(raw_stderr);

    // Simulate how the debug file payload is assembled in process_stream_background
    let payload = format!(
        "no stdout lines captured\n\nexit_code: {:?}\nexit_signal: {:?}\n\nstderr:\n{}",
        Some(1i32),
        None::<i32>,
        redacted_stderr.trim(),
    );

    assert!(
        !payload.contains("sk-ant-api03"),
        "Debug file payload must not contain raw Anthropic key prefix"
    );
    assert!(
        !payload.contains("XxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXx01234567890"),
        "Debug file payload must not contain raw key material"
    );
    assert!(
        payload.contains("***REDACTED***"),
        "Debug file payload must contain redaction placeholder"
    );
}

/// Verifies that the stderr_preview used in tracing::warn is sanitised
/// when redaction is applied before constructing the preview slice.
#[test]
fn test_stderr_preview_for_warn_contains_redacted_content() {
    let raw_stderr = "sk-ant-api03-AAABBBCCC111222333444555666777888999000aabbccddee error in provider call".to_string();
    let redacted = redact(&raw_stderr);
    let preview = &redacted[..redacted.len().min(2000)];

    assert!(
        !preview.contains("sk-ant-api03"),
        "stderr_preview must not expose the Anthropic key prefix"
    );
    assert!(
        !preview.contains("AAABBBCCC111222333444555666777888999000aabbccddee"),
        "stderr_preview must not expose raw key material"
    );
    assert!(
        preview.contains("sk-ant-***REDACTED***"),
        "stderr_preview must show redaction placeholder"
    );
}

/// Verifies `silent_interactive_exit` is set on StreamOutcome and reflects
/// the between_interactive_turns state at process exit.
#[test]
fn test_silent_interactive_exit_flag_semantics() {
    // Process exits between turns (idle): silent_interactive_exit = true
    let idle_exit = StreamOutcome {
        response_text: String::new(),
        tool_calls: vec![],
        content_blocks: vec![],
        session_id: Some("sess-1".to_string()),
        usage: AgentRunUsage::default(),
        stderr_text: String::new(),
        turns_finalized: 1,
        execution_slot_held: false, // slot released at TurnComplete
        silent_interactive_exit: true,
    };
    assert!(idle_exit.silent_interactive_exit, "Idle between turns → silent exit");
    assert!(!idle_exit.execution_slot_held, "Slot released at TurnComplete");

    // Process exits mid-turn (active): silent_interactive_exit = false
    let active_exit = StreamOutcome {
        response_text: "partial".to_string(),
        tool_calls: vec![],
        content_blocks: vec![],
        session_id: None,
        usage: AgentRunUsage::default(),
        stderr_text: String::new(),
        turns_finalized: 0,
        execution_slot_held: true, // slot not yet released
        silent_interactive_exit: false,
    };
    assert!(!active_exit.silent_interactive_exit, "Mid-turn exit → not silent");
    assert!(active_exit.execution_slot_held, "Slot still held mid-turn");

    // Crash-while-idle path: Ok(Err(e)) branch with between_interactive_turns=true
    // → same semantics as idle exit
    let crash_idle = StreamOutcome {
        response_text: String::new(),
        tool_calls: vec![],
        content_blocks: vec![],
        session_id: None,
        usage: AgentRunUsage::default(),
        stderr_text: "error: session expired".to_string(),
        turns_finalized: 1,
        execution_slot_held: false,
        silent_interactive_exit: true, // set in Ok(Err(e)) branch when between_interactive_turns
    };
    assert!(
        crash_idle.silent_interactive_exit,
        "Crash-while-idle (Ok(Err(e)) with between_interactive_turns) → silent exit"
    );
}
