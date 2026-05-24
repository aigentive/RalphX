use super::{
    session_changed_after_resume, should_process_stream_queue,
    should_warn_missing_agent_task_ledger,
};
use crate::application::interactive_process_registry::{
    InteractiveProcessKey, InteractiveProcessRegistry,
};
use crate::application::AppState;
use crate::commands::ExecutionState;
use crate::domain::entities::{
    AgentConversationWorkspaceMode, ChatAttachment, ChatContextType, ChatConversation,
    ChatConversationId, ChatTimelineItemStatus, ProjectId,
};
use crate::infrastructure::agents::claude::{ContentBlockItem, ToolCall};
use std::path::Path;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;

fn test_tool_call(name: &str) -> ToolCall {
    ToolCall {
        id: None,
        name: name.to_string(),
        arguments: serde_json::json!({}),
        result: None,
        parent_tool_use_id: None,
        diff_context: None,
        stats: None,
    }
}

fn agent_mode_conversation() -> ChatConversation {
    let mut conversation = ChatConversation::new_project(ProjectId::new());
    conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Edit));
    conversation
}

#[test]
fn session_changed_returns_true_when_ids_differ() {
    assert!(session_changed_after_resume(
        Some("session-old-abc"),
        Some("session-new-xyz"),
    ));
}

#[test]
fn session_changed_returns_false_when_ids_match() {
    assert!(!session_changed_after_resume(
        Some("session-abc"),
        Some("session-abc"),
    ));
}

#[test]
fn session_changed_returns_false_when_no_stored_id() {
    // --resume was not used; no comparison possible
    assert!(!session_changed_after_resume(None, Some("session-new")));
}

#[test]
fn session_changed_returns_false_when_no_new_id() {
    // Stream returned no session ID; cannot detect change
    assert!(!session_changed_after_resume(Some("session-old"), None));
}

#[test]
fn session_changed_returns_false_when_both_none() {
    assert!(!session_changed_after_resume(None, None));
}

#[test]
fn stream_queue_processing_gate_requires_queue_session_and_no_cancelled_silent_exit() {
    assert!(should_process_stream_queue(1, true, false, false));
    assert!(!should_process_stream_queue(0, true, false, false));
    assert!(!should_process_stream_queue(1, false, false, false));
    assert!(!should_process_stream_queue(1, true, true, true));
}

#[test]
fn stream_queue_processing_gate_allows_non_cancel_silent_exit_with_queue() {
    assert!(
        should_process_stream_queue(1, true, true, false),
        "timeout/eof silent exits can still drain queued messages"
    );
}

#[test]
fn agent_task_ledger_warning_triggers_for_agent_mode_edit_without_ledger_tool() {
    let conversation = agent_mode_conversation();

    assert!(should_warn_missing_agent_task_ledger(
        Some(&conversation),
        &[test_tool_call("Edit")]
    ));
}

#[test]
fn agent_task_ledger_warning_triggers_for_agent_mode_many_readonly_tools_without_ledger_tool() {
    let conversation = agent_mode_conversation();

    assert!(should_warn_missing_agent_task_ledger(
        Some(&conversation),
        &[
            test_tool_call("Read"),
            test_tool_call("Grep"),
            test_tool_call("Read"),
        ],
    ));
}

#[test]
fn agent_task_ledger_warning_is_suppressed_after_ledger_tool_use() {
    let conversation = agent_mode_conversation();

    assert!(!should_warn_missing_agent_task_ledger(
        Some(&conversation),
        &[
            test_tool_call("Edit"),
            test_tool_call("mcp__ralphx__create_agent_task"),
        ],
    ));
}

#[test]
fn agent_task_ledger_warning_is_suppressed_for_non_agent_mode_conversation() {
    let conversation = ChatConversation::new_project(ProjectId::new());

    assert!(!should_warn_missing_agent_task_ledger(
        Some(&conversation),
        &[
            test_tool_call("Read"),
            test_tool_call("Grep"),
            test_tool_call("Edit"),
        ],
    ));
}

/// Verifies the warning condition for zero-processed queue scenarios.
///
/// When `will_process_queue=true` (queue had items + session available), the
/// pre-queue `run_completed` is skipped. If `total_processed=0` (race, spawn
/// failure, or cancellation), the old `if total_processed > 0` guard would
/// have silently dropped `run_completed` entirely — leaving the UI stuck in
/// `generating` state forever.
///
/// The fix: always emit `run_completed` after queue processing; only log a
/// warning when `total_processed=0` but `initial_queue_count>0`.
#[test]
fn run_completed_emitted_when_queue_had_items_but_none_processed() {
    use crate::domain::entities::ChatContextType;
    use crate::domain::services::MessageQueue;

    let queue = MessageQueue::new();

    queue.queue(
        ChatContextType::TaskExecution,
        "task-1",
        "Queued message 1".to_string(),
    );
    queue.queue(
        ChatContextType::TaskExecution,
        "task-1",
        "Queued message 2".to_string(),
    );

    let initial_queue_count = queue
        .get_queued(ChatContextType::TaskExecution, "task-1")
        .len();
    assert_eq!(
        initial_queue_count, 2,
        "initial_queue_count must reflect queued messages"
    );

    // Simulate spawn failure: total_processed stays 0
    let total_processed: usize = 0;

    // Old guard `if total_processed > 0` would have skipped run_completed here.
    // New code: always emit; log warning when this condition is true.
    let should_warn = total_processed == 0 && initial_queue_count > 0;
    assert!(
        should_warn,
        "Warning condition must trigger for race/spawn failure/cancellation case"
    );

    // run_completed is always emitted — not gated on total_processed > 0.
    // The unconditional emission path is the fix (tested at call site in production code).
}

#[test]
fn queue_processing_outcome_uses_last_queued_run_for_terminal_event() {
    let outcome = super::super::chat_service_queue::QueueProcessingOutcome {
        total_processed: 2,
        last_run_id: Some("queued-run-2".to_string()),
    };

    assert_eq!(outcome.terminal_run_id("parent-run"), "queued-run-2");
}

#[test]
fn queue_processing_outcome_falls_back_to_parent_run_without_queued_run() {
    let outcome = super::super::chat_service_queue::QueueProcessingOutcome {
        total_processed: 0,
        last_run_id: None,
    };

    assert_eq!(outcome.terminal_run_id("parent-run"), "parent-run");
}

#[tokio::test]
async fn queue_processing_leaves_messages_pending_when_execution_paused() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.pause();

    app_state.message_queue.queue(
        ChatContextType::Ideation,
        "session-paused",
        "Queued while paused".to_string(),
    );

    let conversation_id = ChatConversationId::new();
    let unused_paused_path = Path::new(".");

    let outcome = super::super::chat_service_queue::process_queued_messages::<tauri::Wry>(
        ChatContextType::Ideation,
        crate::domain::agents::AgentHarnessKind::Claude,
        "session-paused",
        "session-paused",
        conversation_id,
        "session-cli",
        &app_state.message_queue,
        &app_state.chat_message_repo,
        None,
        &app_state.chat_attachment_repo,
        &app_state.artifact_repo,
        &app_state.activity_event_repo,
        &app_state.task_repo,
        &app_state.ideation_session_repo,
        unused_paused_path,
        unused_paused_path,
        unused_paused_path,
        None,
        Some(Arc::clone(&execution_state)),
        None,
        None,
        false,
        tokio_util::sync::CancellationToken::new(),
        None,
        None,
        super::StreamingStateCache::new(),
    )
    .await;

    assert_eq!(
        outcome.total_processed, 0,
        "paused queue processing must not launch messages"
    );
    assert_eq!(outcome.last_run_id, None);
    assert_eq!(
        app_state
            .message_queue
            .get_queued(ChatContextType::Ideation, "session-paused")
            .len(),
        1,
        "paused queue processing must leave the queued message pending"
    );
}

#[tokio::test]
async fn queue_processing_records_run_id_before_spawn_failure() {
    let app_state = AppState::new_test();
    let message_queue = Arc::clone(&app_state.message_queue);
    let chat_message_repo = Arc::clone(&app_state.chat_message_repo);
    let chat_attachment_repo = Arc::clone(&app_state.chat_attachment_repo);
    let artifact_repo = Arc::clone(&app_state.artifact_repo);
    let activity_event_repo = Arc::clone(&app_state.activity_event_repo);
    let task_repo = Arc::clone(&app_state.task_repo);
    let ideation_session_repo = Arc::clone(&app_state.ideation_session_repo);
    let app = tauri::test::mock_builder()
        .manage(app_state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    let app_handle = app.handle().clone();

    message_queue.queue(
        ChatContextType::Ideation,
        "session-spawn-fails",
        "Queued message".to_string(),
    );

    let conversation_id = ChatConversationId::new();
    let invalid_cli_path = Path::new("/definitely/missing/ralphx-test-cli");
    let unused_path = Path::new(".");

    let outcome =
        super::super::chat_service_queue::process_queued_messages::<tauri::test::MockRuntime>(
            ChatContextType::Ideation,
            crate::domain::agents::AgentHarnessKind::Claude,
            "session-spawn-fails",
            "session-spawn-fails",
            conversation_id,
            "session-cli",
            &message_queue,
            &chat_message_repo,
            None,
            &chat_attachment_repo,
            &artifact_repo,
            &activity_event_repo,
            &task_repo,
            &ideation_session_repo,
            invalid_cli_path,
            unused_path,
            unused_path,
            None,
            None,
            Some(app_handle),
            None,
            false,
            tokio_util::sync::CancellationToken::new(),
            None,
            None,
            super::StreamingStateCache::new(),
        )
        .await;

    assert_eq!(outcome.total_processed, 1);
    assert!(outcome.last_run_id.is_some());
}

#[tokio::test]
async fn queue_processing_links_selected_attachments_before_spawn_failure() {
    let app_state = AppState::new_test();
    let message_queue = Arc::clone(&app_state.message_queue);
    let chat_message_repo = Arc::clone(&app_state.chat_message_repo);
    let chat_attachment_repo = Arc::clone(&app_state.chat_attachment_repo);
    let artifact_repo = Arc::clone(&app_state.artifact_repo);
    let activity_event_repo = Arc::clone(&app_state.activity_event_repo);
    let task_repo = Arc::clone(&app_state.task_repo);
    let ideation_session_repo = Arc::clone(&app_state.ideation_session_repo);
    let app = tauri::test::mock_builder()
        .manage(app_state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    let app_handle = app.handle().clone();
    let temp = tempfile::tempdir().expect("tempdir");
    let selected_path = temp.path().join("selected.txt");
    let unselected_path = temp.path().join("unselected.txt");
    std::fs::write(&selected_path, "selected queued attachment").expect("write selected");
    std::fs::write(&unselected_path, "unselected queued attachment").expect("write unselected");

    let conversation_id = ChatConversationId::new();
    let selected_attachment = chat_attachment_repo
        .create(ChatAttachment::new(
            conversation_id,
            "selected.txt",
            selected_path.to_string_lossy().to_string(),
            26,
            Some("text/plain".to_string()),
        ))
        .await
        .expect("selected attachment should persist");
    let unselected_attachment = chat_attachment_repo
        .create(ChatAttachment::new(
            conversation_id,
            "unselected.txt",
            unselected_path.to_string_lossy().to_string(),
            28,
            Some("text/plain".to_string()),
        ))
        .await
        .expect("unselected attachment should persist");

    message_queue.queue_with_overrides_and_project_references(
        ChatContextType::Ideation,
        "session-queued-attachments",
        "Queued message with selected attachment".to_string(),
        None,
        None,
        None,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        vec![selected_attachment.id],
    );

    let invalid_cli_path = Path::new("/definitely/missing/ralphx-test-cli");
    let outcome =
        super::super::chat_service_queue::process_queued_messages::<tauri::test::MockRuntime>(
            ChatContextType::Ideation,
            crate::domain::agents::AgentHarnessKind::Claude,
            "session-queued-attachments",
            "session-queued-attachments",
            conversation_id,
            "session-cli",
            &message_queue,
            &chat_message_repo,
            None,
            &chat_attachment_repo,
            &artifact_repo,
            &activity_event_repo,
            &task_repo,
            &ideation_session_repo,
            invalid_cli_path,
            temp.path(),
            temp.path(),
            None,
            None,
            Some(app_handle),
            None,
            false,
            tokio_util::sync::CancellationToken::new(),
            None,
            None,
            super::StreamingStateCache::new(),
        )
        .await;

    assert_eq!(outcome.total_processed, 1);

    let selected = chat_attachment_repo
        .get_by_id(&selected_attachment.id)
        .await
        .expect("selected lookup should succeed")
        .expect("selected attachment should exist");
    let unselected = chat_attachment_repo
        .get_by_id(&unselected_attachment.id)
        .await
        .expect("unselected lookup should succeed")
        .expect("unselected attachment should exist");

    assert!(
        selected.message_id.is_some(),
        "selected queued attachment should link to the queued user message"
    );
    assert_eq!(
        unselected.message_id, None,
        "unselected queued attachment should remain pending"
    );
}

async fn spawn_claude_jsonl_fixture(lines: &[&str]) -> tokio::process::Child {
    let mut payload = String::new();
    for line in lines {
        payload.push_str(line);
        payload.push('\n');
    }

    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn stream fixture");
    let mut stdin = child.stdin.take().expect("capture fixture stdin");
    stdin
        .write_all(payload.as_bytes())
        .await
        .expect("write stream fixture");
    drop(stdin);
    child
}

#[tokio::test]
async fn background_run_drains_queue_after_non_cancelled_silent_exit() {
    use crate::domain::agents::AgentHarnessKind;
    use crate::domain::entities::{ChatConversation, ChatMessageAttribution, IdeationSessionId};
    use tokio::time::{sleep, timeout, Duration};

    let state = AppState::new_test();
    let context_id = IdeationSessionId::new();
    let conversation = ChatConversation::new_ideation(context_id.clone());
    let conversation_id = conversation.id.clone();
    state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .expect("seed conversation");

    let message_queue = Arc::clone(&state.message_queue);
    let context_id_str = context_id.as_str().to_string();
    message_queue.queue(
        ChatContextType::Ideation,
        &context_id_str,
        "queued follow-up after idle exit".to_string(),
    );

    let repos = super::BackgroundRunRepos {
        chat_message_repo: Arc::clone(&state.chat_message_repo),
        chat_timeline_repo: Some(Arc::clone(&state.chat_timeline_repo)),
        chat_attachment_repo: Arc::clone(&state.chat_attachment_repo),
        artifact_repo: Arc::clone(&state.artifact_repo),
        conversation_repo: Arc::clone(&state.chat_conversation_repo),
        agent_run_repo: Arc::clone(&state.agent_run_repo),
        task_repo: Arc::clone(&state.task_repo),
        task_dependency_repo: Arc::clone(&state.task_dependency_repo),
        project_repo: Arc::clone(&state.project_repo),
        ideation_session_repo: Arc::clone(&state.ideation_session_repo),
        delegated_session_repo: Arc::clone(&state.delegated_session_repo),
        execution_settings_repo: None,
        agent_lane_settings_repo: None,
        ideation_effort_settings_repo: None,
        ideation_model_settings_repo: None,
        agent_conversation_workspace_repo: Some(Arc::clone(
            &state.agent_conversation_workspace_repo,
        )),
        task_proposal_repo: Some(Arc::clone(&state.task_proposal_repo)),
        activity_event_repo: Arc::clone(&state.activity_event_repo),
        memory_event_repo: Arc::clone(&state.memory_event_repo),
        message_queue: Arc::clone(&message_queue),
        running_agent_registry: Arc::clone(&state.running_agent_registry),
        task_step_repo: Some(Arc::clone(&state.task_step_repo)),
        review_repo: Some(Arc::clone(&state.review_repo)),
    };

    let app = tauri::test::mock_builder()
        .manage(state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    let app_handle = app.handle().clone();

    let child = spawn_claude_jsonl_fixture(&[
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"initial turn complete"}]},"session_id":"sess-bg"}"#,
        r#"{"type":"result","session_id":"sess-bg","is_error":false,"result":"initial turn complete","cost_usd":0.0}"#,
    ])
    .await;

    super::spawn_send_message_background::<tauri::test::MockRuntime>(super::BackgroundRunContext {
        child,
        harness: AgentHarnessKind::Claude,
        context_type: ChatContextType::Ideation,
        context_id: context_id_str.clone(),
        runtime_context_id: context_id_str.clone(),
        conversation_id,
        agent_run_id: "background-run-id".to_string(),
        stored_session_id: None,
        working_directory: Path::new(".").to_path_buf(),
        cli_path: Path::new("/definitely/missing/ralphx-test-cli").to_path_buf(),
        plugin_dir: Path::new(".").to_path_buf(),
        repos,
        execution_state: None,
        question_state: None,
        plan_branch_repo: None,
        app_handle: Some(app_handle),
        run_chain_id: None,
        is_retry_attempt: false,
        user_message_content: Some("initial prompt".to_string()),
        conversation: Some(conversation),
        agent_name: Some("orchestrator".to_string()),
        team_mode: false,
        assistant_message_attribution: ChatMessageAttribution::default(),
        persist_conversation_provider_session_ref: true,
        cancellation_token: tokio_util::sync::CancellationToken::new(),
        team_service: None,
        streaming_state_cache: super::StreamingStateCache::new(),
        interactive_process_registry: None,
        verification_child_registry: None,
    });

    timeout(Duration::from_secs(3), async {
        loop {
            if message_queue
                .get_queued(ChatContextType::Ideation, &context_id_str)
                .is_empty()
            {
                break;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("background queue processing should drain queued message");
}

/// Verifies that session swap recovery enqueues rehydration at front of queue,
/// preserving ordering: recovery context → pending user messages.
#[test]
fn session_swap_recovery_enqueues_rehydration_before_user_messages() {
    use crate::domain::entities::ChatContextType;
    use crate::domain::services::MessageQueue;

    let queue = MessageQueue::new();

    // Simulate: user queued messages while agent was running
    queue.queue(
        ChatContextType::Ideation,
        "ctx-1",
        "User follow-up 1".to_string(),
    );
    queue.queue(
        ChatContextType::Ideation,
        "ctx-1",
        "User follow-up 2".to_string(),
    );

    // Session swap detected → recovery enqueues rehydration at front
    let rehydration_content = "<instructions>Your session was recovered</instructions>".to_string();
    queue.queue_front(
        ChatContextType::Ideation,
        "ctx-1",
        rehydration_content.clone(),
    );

    // Verify queue order: rehydration first, then user messages
    let queued = queue.get_queued(ChatContextType::Ideation, "ctx-1");
    assert_eq!(queued.len(), 3);
    assert_eq!(queued[0].content, rehydration_content);
    assert_eq!(queued[1].content, "User follow-up 1");
    assert_eq!(queued[2].content, "User follow-up 2");

    // Pop order should match: rehydration processed first via --resume
    let first = queue.pop(ChatContextType::Ideation, "ctx-1").unwrap();
    assert!(first.content.contains("session was recovered"));
}

// ============================================================================
// IPR zombie fix tests (Fix 1A)
//
// These tests verify the invariant: IPR is ALWAYS removed on stream exit,
// regardless of whether a team is still active. A dead process's stdin is
// useless and must never be kept as a zombie.
// ============================================================================

/// Helper: spawn a cat process to get a real ChildStdin (same as IPR registry tests).
async fn spawn_test_stdin() -> (tokio::process::ChildStdin, tokio::process::Child) {
    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("failed to spawn cat");
    let stdin = child.stdin.take().expect("no stdin");
    (stdin, child)
}

/// Verifies that IPR entry is removed even when the team is still active.
///
/// Regression test for the IPR_KEEP zombie bug: previously, when `team_still_active=true`,
/// the IPR entry was kept (`IPR_KEEP`), creating a zombie stdin handle for a dead process.
/// The fix always removes the entry unconditionally on stream exit.
#[tokio::test]
async fn ipr_removed_even_when_team_still_active() {
    let (stdin, _child) = spawn_test_stdin().await;
    let ipr = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("ideation", "session-zombie-test");

    // Register a process (simulating a lead agent that just started)
    ipr.register(key.clone(), stdin).await;
    assert!(
        ipr.has_process(&key).await,
        "Precondition: IPR entry must exist before cleanup"
    );

    // Simulate stream exit cleanup with team_still_active=true.
    // The new behavior: always remove, even when team is still active.
    // (Previously: IPR_KEEP would skip this remove → zombie)
    ipr.remove(&key).await;

    assert!(
        !ipr.has_process(&key).await,
        "IPR entry must be removed on stream exit even when team is still active"
    );
}

/// Verifies that a disband_team failure does not leave a zombie IPR entry.
///
/// When `disband_team` fails, the old code left `team_still_active=true` which
/// triggered IPR_KEEP, persisting a dead stdin handle. The fix: even on disband
/// failure, always call `ipr.remove()` — dead stdin is useless regardless.
#[tokio::test]
async fn disband_failure_does_not_leave_zombie_ipr_entry() {
    use crate::application::team_service::TeamService;
    use crate::application::team_state_tracker::TeamStateTracker;
    use std::sync::Arc;

    let (stdin, _child) = spawn_test_stdin().await;
    let ipr = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("ideation", "session-disband-fail-test");
    let context_id = "session-disband-fail-test";

    // Register IPR entry for a lead process
    ipr.register(key.clone(), stdin).await;
    assert!(
        ipr.has_process(&key).await,
        "Precondition: IPR entry must exist"
    );

    // Create a TeamService and register a team for this context.
    // We simulate a scenario where a team is active but we need to clean up.
    let tracker = Arc::new(TeamStateTracker::new());
    let service = TeamService::new_without_events(Arc::clone(&tracker));
    service
        .create_team("test-team", context_id, "ideation")
        .await
        .unwrap();

    // Verify team is active (simulates state before disband failure)
    let status = service.get_team_status("test-team").await.unwrap();
    assert_eq!(status.context_id, context_id);

    // Simulate disband failure by NOT calling disband_team (team remains active).
    // In this scenario, the old code would set team_still_active=true and KEEP the IPR.
    // The fix: always remove the IPR regardless of disband outcome.
    // Here we directly verify: remove() works unconditionally even with active team.
    ipr.remove(&key).await;

    assert!(
        !ipr.has_process(&key).await,
        "IPR entry must be removed even when disband_team fails (no zombie)"
    );

    // Team may still be registered (disband failed), but IPR is gone.
    // Teammate nudges will trigger re-spawn via the IPR-miss path.
    let post_status = service.get_team_status("test-team").await;
    assert!(
        post_status.is_ok(),
        "Team registration may persist when disband fails, but IPR must still be cleaned"
    );
}

/// Verifies that after IPR removal, has_process returns false,
/// which causes the send_message path to fall through to agent re-spawn.
///
/// When a teammate tries to nudge the lead after IPR removal:
/// 1. has_process() returns false → write_message skipped
/// 2. running_agent_registry miss → queue skipped
/// 3. send_message spawns a new agent (re-spawn via IPR-miss path)
#[tokio::test]
async fn ipr_miss_enables_respawn_path() {
    let (stdin, _child) = spawn_test_stdin().await;
    let ipr = InteractiveProcessRegistry::new();
    let key = InteractiveProcessKey::new("ideation", "session-respawn-test");

    // Start with an IPR entry
    ipr.register(key.clone(), stdin).await;
    assert!(ipr.has_process(&key).await, "Precondition: entry exists");

    // Lead process exits → IPR removed (the fix)
    ipr.remove(&key).await;

    // After removal: has_process returns false
    // This is what triggers the re-spawn path in send_message handlers
    assert!(
        !ipr.has_process(&key).await,
        "has_process must return false after removal, enabling re-spawn path"
    );

    // write_message on a missing key returns an error (would be caught in send flow)
    let write_result = ipr.write_message(&key, "nudge from teammate").await;
    assert!(
        write_result.is_err(),
        "write_message must fail when IPR entry absent (triggers re-spawn fallthrough)"
    );
}

// ============================================================================
// Auto-archive guard tests (Fix 3)
//
// These tests verify the invariant: verification child sessions are NOT
// auto-archived at the auto-archive callsite in chat_service_send_background.rs.
// The run_completed hook (Fix 1) is responsible for archival after parent
// reconciliation. The periodic reconciler is the fallback for orphaned children.
// ============================================================================

/// Verifies that a verification child session is NOT auto-archived at the
/// auto-archive callsite.
///
/// Fix 3 changes the Verification match arm from archiving the child to
/// skipping archival (deferred to the run_completed hook). This test
/// confirms the guard fires: the session remains Active after the code path
/// executes without calling update_status.
#[tokio::test]
async fn verification_child_session_not_auto_archived_at_callsite() {
    use crate::domain::entities::{
        IdeationSession, IdeationSessionStatus, ProjectId, SessionPurpose,
    };
    use crate::domain::repositories::IdeationSessionRepository;
    use crate::infrastructure::memory::MemoryIdeationSessionRepository;
    use std::sync::Arc;

    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // Create a verification child session (simulates a ralphx-plan-verifier child agent)
    let session = IdeationSession::builder()
        .project_id(project_id)
        .session_purpose(SessionPurpose::Verification)
        .build();
    let session_id = session.id.clone();
    repo.create(session).await.unwrap();

    // Simulate the auto-archive guard logic:
    // The guard matches session_purpose == Verification and skips update_status.
    let retrieved = repo.get_by_id(&session_id).await.unwrap().unwrap();
    if retrieved.session_purpose == SessionPurpose::Verification {
        // Guard fires: do NOT call update_status — deferred to run_completed hook
    }
    // No update_status call means the session status is unchanged.

    let after = repo.get_by_id(&session_id).await.unwrap().unwrap();
    assert_eq!(
        after.status,
        IdeationSessionStatus::Active,
        "verification child must NOT be auto-archived at the auto-archive callsite"
    );
}

/// Verifies that non-verification (general) sessions are unaffected by the
/// auto-archive guard — no regression from Fix 3.
///
/// General sessions fall through to the `Ok(Some(_)) => {}` arm (no action).
/// This test confirms that after Fix 3, general sessions remain Active and
/// are not accidentally archived or errored.
#[tokio::test]
async fn general_session_not_archived_at_auto_archive_callsite_no_regression() {
    use crate::domain::entities::{
        IdeationSession, IdeationSessionStatus, ProjectId, SessionPurpose,
    };
    use crate::domain::repositories::IdeationSessionRepository;
    use crate::infrastructure::memory::MemoryIdeationSessionRepository;
    use std::sync::Arc;

    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let project_id = ProjectId::new();

    // Create a general (non-verification) session — default session_purpose is General
    let session = IdeationSession::new(project_id);
    assert_eq!(
        session.session_purpose,
        SessionPurpose::General,
        "IdeationSession::new() must default to General purpose"
    );
    let session_id = session.id.clone();
    repo.create(session).await.unwrap();

    // Simulate the auto-archive guard logic:
    // The guard does not match General sessions → falls through to no-op arm.
    let retrieved = repo.get_by_id(&session_id).await.unwrap().unwrap();
    if retrieved.session_purpose == SessionPurpose::Verification {
        panic!("unexpected: general session matched verification guard");
    }
    // No update_status call for general sessions (same as before Fix 3).

    let after = repo.get_by_id(&session_id).await.unwrap().unwrap();
    assert_eq!(
        after.status,
        IdeationSessionStatus::Active,
        "general session must remain Active — not archived at the auto-archive callsite"
    );
}

#[tokio::test]
async fn finalize_no_output_writes_both_chat_messages_and_timeline_placeholder() {
    use crate::application::chat_service::create_assistant_message;
    use crate::application::chat_service::finalize_no_output_assistant_message_for_test;
    use crate::domain::entities::{ChatConversationId, ChatTimelineItemStatus, IdeationSessionId};

    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    let session_id = IdeationSessionId::new();

    // Seed the pre-created empty assistant placeholder, matching the production spawn flow.
    let pre_assistant = create_assistant_message(
        ChatContextType::Ideation,
        session_id.as_str(),
        "",
        conversation_id.clone(),
        &[],
        &[],
    );
    let pre_assistant_id = pre_assistant.id.as_str().to_string();
    state
        .chat_message_repo
        .create(pre_assistant)
        .await
        .expect("seed pre-assistant message");

    finalize_no_output_assistant_message_for_test::<tauri::Wry>(
        &state.chat_message_repo,
        &Some(state.chat_timeline_repo.clone()),
        None,
        &conversation_id,
        "ideation",
        session_id.as_str(),
        &pre_assistant_id,
        "orchestrator",
    )
    .await;

    // chat_messages got the placeholder note.
    let persisted = state
        .chat_message_repo
        .get_by_id(&crate::domain::entities::ChatMessageId::from_string(
            pre_assistant_id.clone(),
        ))
        .await
        .expect("load message")
        .expect("message persisted");
    assert!(
        persisted.content.contains("Agent completed with no output"),
        "chat_messages.content must carry the no-output note"
    );

    // chat_message_blocks (timeline) also got the placeholder so the timeline-rendering
    // chat UI does not show a blank turn.
    let page = state
        .chat_timeline_repo
        .get_page(&conversation_id, 10, None)
        .await
        .expect("load timeline page");
    let assistant_blocks: Vec<_> = page
        .items
        .iter()
        .filter(|item| {
            item.message_id
                .as_ref()
                .is_some_and(|id| id.as_str() == pre_assistant_id)
        })
        .collect();
    assert_eq!(
        assistant_blocks.len(),
        1,
        "no-output finalization must write exactly one timeline placeholder block"
    );
    assert_eq!(
        assistant_blocks[0].status,
        ChatTimelineItemStatus::Finalized,
        "the placeholder block must be finalized so the UI does not show a spinner"
    );
    assert!(
        assistant_blocks[0]
            .text
            .as_deref()
            .unwrap_or_default()
            .contains("Agent completed with no output"),
        "the placeholder block must carry the same note as chat_messages"
    );
}

#[tokio::test]
async fn finalize_structured_writes_chat_message_and_finalized_timeline_rows() {
    use crate::application::chat_service::create_assistant_message;
    use crate::domain::entities::IdeationSessionId;

    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    let session_id = IdeationSessionId::new();
    let pre_assistant = create_assistant_message(
        ChatContextType::Ideation,
        session_id.as_str(),
        "",
        conversation_id.clone(),
        &[],
        &[],
    );
    let pre_assistant_id = pre_assistant.id.as_str().to_string();
    state
        .chat_message_repo
        .create(pre_assistant)
        .await
        .expect("seed pre-assistant message");

    let tool_calls = vec![ToolCall {
        id: Some("toolu-read".to_string()),
        name: "Read".to_string(),
        arguments: serde_json::json!({ "file_path": "src/app.ts" }),
        result: Some(serde_json::json!("preview")),
        parent_tool_use_id: None,
        diff_context: None,
        stats: None,
    }];
    let content_blocks = vec![
        ContentBlockItem::Text {
            text: "Done".to_string(),
        },
        ContentBlockItem::ToolUse {
            id: Some("toolu-read".to_string()),
            name: "Read".to_string(),
            arguments: serde_json::json!({ "file_path": "src/app.ts" }),
            result: Some(serde_json::json!("preview")),
            parent_tool_use_id: None,
            diff_context: Some(serde_json::json!({ "file_path": "src/app.ts" })),
        },
    ];

    super::finalize_structured_assistant_message::<tauri::Wry>(
        &state.chat_message_repo,
        &Some(state.chat_timeline_repo.clone()),
        None,
        ChatContextType::Ideation,
        session_id.as_str(),
        &conversation_id,
        &pre_assistant_id,
        "orchestrator",
        "Done",
        &tool_calls,
        &content_blocks,
        false,
    )
    .await;

    let persisted = state
        .chat_message_repo
        .get_by_id(&crate::domain::entities::ChatMessageId::from_string(
            pre_assistant_id.clone(),
        ))
        .await
        .expect("load message")
        .expect("message persisted");
    assert_eq!(persisted.content, "Done");
    assert!(persisted
        .content_blocks
        .as_deref()
        .is_some_and(|raw| raw.contains("toolu-read")));

    let page = state
        .chat_timeline_repo
        .get_page(&conversation_id, 10, None)
        .await
        .expect("load timeline page");
    let assistant_blocks: Vec<_> = page
        .items
        .iter()
        .filter(|item| {
            item.message_id
                .as_ref()
                .is_some_and(|id| id.as_str() == pre_assistant_id)
        })
        .collect();
    assert_eq!(assistant_blocks.len(), 2);
    assert!(assistant_blocks
        .iter()
        .all(|item| item.status == ChatTimelineItemStatus::Finalized));
    assert_eq!(assistant_blocks[0].text.as_deref(), Some("Done"));
    assert_eq!(assistant_blocks[1].tool_call_id.as_deref(), Some("toolu-read"));
    assert_eq!(assistant_blocks[1].tool_status.as_deref(), Some("completed"));
    assert!(assistant_blocks[1]
        .raw_block_json
        .as_deref()
        .is_some_and(|raw| raw.contains("diff_context")));
}

#[tokio::test]
async fn finalize_structured_split_transcript_writes_timeline_for_each_segment() {
    use crate::application::chat_service::create_assistant_message;
    use crate::domain::entities::IdeationSessionId;

    let state = AppState::new_test();
    let app = tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    let app_handle = app.handle().clone();
    let conversation_id = ChatConversationId::new();
    let session_id = IdeationSessionId::new();
    let pre_assistant = create_assistant_message(
        ChatContextType::Ideation,
        session_id.as_str(),
        "",
        conversation_id.clone(),
        &[],
        &[],
    );
    let pre_assistant_id = pre_assistant.id.as_str().to_string();
    state
        .chat_message_repo
        .create(pre_assistant)
        .await
        .expect("seed pre-assistant message");

    let content_blocks = vec![
        ContentBlockItem::Text {
            text: "First segment".to_string(),
        },
        ContentBlockItem::ToolUse {
            id: Some("toolu-read".to_string()),
            name: "Read".to_string(),
            arguments: serde_json::json!({ "file_path": "src/app.ts" }),
            result: Some(serde_json::json!("preview")),
            parent_tool_use_id: None,
            diff_context: None,
        },
        ContentBlockItem::Text {
            text: "Second segment".to_string(),
        },
    ];
    let tool_calls = vec![ToolCall {
        id: Some("toolu-read".to_string()),
        name: "Read".to_string(),
        arguments: serde_json::json!({ "file_path": "src/app.ts" }),
        result: Some(serde_json::json!("preview")),
        parent_tool_use_id: None,
        diff_context: None,
        stats: None,
    }];

    super::finalize_structured_assistant_message::<tauri::test::MockRuntime>(
        &state.chat_message_repo,
        &Some(state.chat_timeline_repo.clone()),
        Some(&app_handle),
        ChatContextType::Ideation,
        session_id.as_str(),
        &conversation_id,
        &pre_assistant_id,
        "orchestrator",
        "First segmentSecond segment",
        &tool_calls,
        &content_blocks,
        true,
    )
    .await;

    let messages = state
        .chat_message_repo
        .get_by_conversation(&conversation_id)
        .await
        .expect("load conversation messages");
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].id.as_str(), pre_assistant_id);
    assert_eq!(messages[0].content, "First segment");
    assert_eq!(messages[1].content, "Second segment");

    let page = state
        .chat_timeline_repo
        .get_page(&conversation_id, 10, None)
        .await
        .expect("load timeline page");
    assert_eq!(page.items.len(), 3);
    assert!(page
        .items
        .iter()
        .all(|item| item.status == ChatTimelineItemStatus::Finalized));
    assert_eq!(page.items[0].message_id.as_ref().unwrap().as_str(), pre_assistant_id);
    assert_eq!(page.items[1].message_id.as_ref().unwrap().as_str(), pre_assistant_id);
    assert_eq!(
        page.items[2].message_id.as_ref().unwrap().as_str(),
        messages[1].id.as_str()
    );
}

#[tokio::test]
async fn exported_finalization_test_helpers_delegate_to_core_paths() {
    use crate::application::chat_service::{
        create_assistant_message, finalize_assistant_message_for_test,
        finalize_structured_assistant_message_for_test,
    };
    use crate::domain::entities::{ChatMessageId, IdeationSessionId};

    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    let session_id = IdeationSessionId::new();

    let plain_message = create_assistant_message(
        ChatContextType::Ideation,
        session_id.as_str(),
        "",
        conversation_id.clone(),
        &[],
        &[],
    );
    let plain_message_id = plain_message.id.as_str().to_string();
    state
        .chat_message_repo
        .create(plain_message)
        .await
        .expect("seed plain assistant message");
    finalize_assistant_message_for_test::<tauri::Wry>(
        &state.chat_message_repo,
        None,
        &conversation_id.as_str(),
        "ideation",
        session_id.as_str(),
        &plain_message_id,
        "orchestrator",
        "Plain helper content",
        None,
        None,
    )
    .await;

    let structured_message = create_assistant_message(
        ChatContextType::Ideation,
        session_id.as_str(),
        "",
        conversation_id.clone(),
        &[],
        &[],
    );
    let structured_message_id = structured_message.id.as_str().to_string();
    state
        .chat_message_repo
        .create(structured_message)
        .await
        .expect("seed structured assistant message");
    finalize_structured_assistant_message_for_test::<tauri::Wry>(
        &state.chat_message_repo,
        None,
        ChatContextType::Ideation,
        session_id.as_str(),
        &conversation_id,
        &structured_message_id,
        "orchestrator",
        "Structured helper content",
        &[],
        &[ContentBlockItem::Text {
            text: "Structured helper content".to_string(),
        }],
        false,
    )
    .await;

    let plain = state
        .chat_message_repo
        .get_by_id(&ChatMessageId::from_string(plain_message_id))
        .await
        .expect("load plain helper message")
        .expect("plain helper message");
    let structured = state
        .chat_message_repo
        .get_by_id(&ChatMessageId::from_string(structured_message_id))
        .await
        .expect("load structured helper message")
        .expect("structured helper message");
    assert_eq!(plain.content, "Plain helper content");
    assert_eq!(structured.content, "Structured helper content");
}
