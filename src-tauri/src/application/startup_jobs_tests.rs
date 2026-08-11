use super::*;

use std::sync::Arc;
use std::time::Duration;

use crate::application::chat_service::MockChatService;
use crate::application::interactive_process_registry::{
    InteractiveProcessKey, InteractiveProcessMetadata,
};
use crate::application::{AppState, TaskTransitionService};
use crate::commands::execution_commands::{ActiveProjectState, ExecutionState};
use crate::domain::entities::app_state::ExecutionHaltMode;
use crate::domain::entities::ideation::{IdeationSessionFlow, IdeationSessionStatus};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentRun, AgentRunStatus,
    ChatContextType, ChatConversation, ChatConversationId, IdeationAnalysisBaseRefKind,
    IdeationSession, InternalStatus, Project, ProjectId, SessionOrigin, Task,
};
use crate::domain::services::{QueueKey, QueuedMessage, RunningAgentKey};
use crate::infrastructure::agents::claude::StreamTimeoutsConfig;
use crate::application::data_retention_service::CYCLE_TEST_SERIALIZER;
use crate::infrastructure::sqlite::DbConnection;
use crate::testing::SqliteTestDb;
use tokio::process::ChildStdin;

async fn create_test_stdin() -> (ChildStdin, tokio::process::Child) {
    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn stdin fixture");
    (child.stdin.take().expect("fixture stdin"), child)
}

// ======= Unit tests for should_auto_recover() =======

#[test]
fn notification_retention_prune_uses_runtime_config_values() {
    let config = StreamTimeoutsConfig {
        notification_retention_read_days: 7,
        notification_retention_max_rows: 42,
        ..StreamTimeoutsConfig::default()
    };
    let now = chrono::DateTime::parse_from_rfc3339("2026-07-11T12:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);

    let (read_before, max_rows) = notification_retention_prune_args(&config, now);

    assert_eq!(read_before, now - chrono::Duration::days(7));
    assert_eq!(max_rows, 42);
}

#[tokio::test]
async fn the_startup_retention_step_records_a_cycle_and_prunes_outside_the_window() {
    let _serialized = CYCLE_TEST_SERIALIZER.lock().await;
    let db = SqliteTestDb::new("startup-retention-runs");
    let stale = chrono::Utc::now() - chrono::Duration::days(200);
    seed_retention_payload(&db, stale);

    let app_state = AppState::new_test();
    let runner = build_runner_for_tests(&app_state)
        .with_data_retention_db(DbConnection::from_shared(db.shared_conn()));
    runner.run().await;

    assert!(
        wait_for(|| retention_last_run_at(&db).is_some()).await,
        "the detached retention step must complete a cycle"
    );
    assert_eq!(retention_payload_rows(&db), 0, "stale payloads are pruned");
}

#[tokio::test]
async fn a_disabled_retention_policy_prunes_nothing_at_startup() {
    let _serialized = CYCLE_TEST_SERIALIZER.lock().await;
    let db = SqliteTestDb::new("startup-retention-disabled");
    let stale = chrono::Utc::now() - chrono::Duration::days(200);
    seed_retention_payload(&db, stale);
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO data_retention_settings (id, payload_retention_enabled, payload_retention_days, payload_retention_archived_days, payload_retention_batch_rows, seeded_pristine, updated_at) VALUES (1, 0, 90, 7, 500, 0, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )
        .unwrap();
    });

    let app_state = AppState::new_test();
    let runner = build_runner_for_tests(&app_state)
        .with_data_retention_db(DbConnection::from_shared(db.shared_conn()));
    runner.run().await;

    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        retention_payload_rows(&db),
        1,
        "a disabled policy must leave payloads untouched"
    );
}

#[tokio::test]
async fn a_broken_retention_table_never_fails_the_startup_runner() {
    let _serialized = CYCLE_TEST_SERIALIZER.lock().await;
    let db = SqliteTestDb::new("startup-retention-repo-error");
    db.with_connection(|conn| {
        conn.execute_batch("DROP TABLE data_retention_settings")
            .unwrap();
    });

    let app_state = AppState::new_test();
    let runner = build_runner_for_tests(&app_state)
        .with_data_retention_db(DbConnection::from_shared(db.shared_conn()));

    // `run` returning at all is the assertion: retention errors are logged, never propagated.
    runner.run().await;
}

#[tokio::test]
async fn the_startup_retention_step_stays_detached_from_the_runner() {
    let _serialized = CYCLE_TEST_SERIALIZER.lock().await;
    let db = SqliteTestDb::new("startup-retention-detached");
    let blocker = db.shared_conn();
    // Hold the single shared connection well past the assertion window: an *inlined*
    // retention cycle would stall `run()` behind this lock.
    let holder = tokio::spawn(async move {
        let _guard = blocker.lock().await;
        tokio::time::sleep(Duration::from_secs(4)).await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let app_state = AppState::new_test();
    let runner = build_runner_for_tests(&app_state)
        .with_data_retention_db(DbConnection::from_shared(db.shared_conn()));

    let started = std::time::Instant::now();
    runner.run().await;
    let elapsed = started.elapsed();

    holder.abort();
    assert!(
        elapsed < Duration::from_secs(3),
        "startup jobs must not wait on the retention cycle (took {elapsed:?})"
    );
}

fn seed_retention_payload(db: &SqliteTestDb, created_at: chrono::DateTime<chrono::Utc>) {
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO chat_conversations (id, context_type, context_id, created_at, updated_at) VALUES ('conversation-1', 'project', 'project-1', ?1, ?1)",
            [created_at.to_rfc3339()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO chat_message_blocks (id, conversation_id, sequence, block_index, role, kind, status, created_at, updated_at) VALUES ('block-1', 'conversation-1', 1, 0, 'assistant', 'tool_use', 'finalized', ?1, ?1)",
            [created_at.to_rfc3339()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO chat_message_block_payloads (block_id, input_json, updated_at) VALUES ('block-1', '{\"retained\":true}', ?1)",
            [created_at.to_rfc3339()],
        )
        .unwrap();
    });
}

fn retention_payload_rows(db: &SqliteTestDb) -> i64 {
    db.with_connection(|conn| {
        conn.query_row(
            "SELECT COUNT(*) FROM chat_message_block_payloads",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap()
    })
}

fn retention_last_run_at(db: &SqliteTestDb) -> Option<String> {
    db.with_connection(|conn| {
        conn.query_row(
            "SELECT last_run_at FROM data_retention_settings WHERE id = 1",
            [],
            |row| row.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten()
    })
}

async fn wait_for(mut condition: impl FnMut() -> bool) -> bool {
    for _ in 0..100 {
        if condition() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

#[test]
fn accepted_plan_mode_handoff_recovery_accepts_only_exact_metadata_and_id() {
    let key = QueueKey::new(ChatContextType::Project, "conversation-1");
    let mut message = QueuedMessage::with_id(
        "plan-mode-handoff:request-1".to_string(),
        "continue in Plan mode".to_string(),
    );
    message.metadata_override = Some(
        serde_json::json!({
            "source": "accepted_plan_mode_proposal",
            "source_request_id": "request-1",
            "required_workspace_mode": "plan",
            "resume_in_place": true,
            "persist_hidden_marker": true,
        })
        .to_string(),
    );

    assert!(is_accepted_plan_mode_handoff_row(&key, &message));

    message.id = "unrelated-id".to_string();
    assert!(
        !is_accepted_plan_mode_handoff_row(&key, &message),
        "metadata alone must not authorize recovery without the stable handoff ID"
    );
}

#[test]
fn accepted_plan_mode_handoff_recovery_rejects_wrong_mode_and_malformed_metadata() {
    let key = QueueKey::new(ChatContextType::Project, "conversation-1");
    let mut wrong_mode = QueuedMessage::with_id(
        "plan-mode-handoff:request-1".to_string(),
        "continue in Plan mode".to_string(),
    );
    wrong_mode.metadata_override = Some(
        serde_json::json!({
            "source": "accepted_plan_mode_proposal",
            "source_request_id": "request-1",
            "required_workspace_mode": "edit",
            "resume_in_place": true,
            "persist_hidden_marker": true,
        })
        .to_string(),
    );
    assert!(!is_accepted_plan_mode_handoff_row(&key, &wrong_mode));

    let mut malformed = wrong_mode;
    malformed.metadata_override = Some("{not-json".to_string());
    assert!(!is_accepted_plan_mode_handoff_row(&key, &malformed));
}

fn startup_handoff_message(request_id: &str) -> QueuedMessage {
    let mut message = QueuedMessage::with_id(
        format!("plan-mode-handoff:{request_id}"),
        "Continue in Plan mode".to_string(),
    );
    message.metadata_override = Some(
        serde_json::json!({
            "source": "accepted_plan_mode_proposal",
            "source_request_id": request_id,
            "required_workspace_mode": "plan",
            "resume_in_place": true,
            "persist_hidden_marker": true,
        })
        .to_string(),
    );
    message
}

async fn setup_accepted_plan_mode_recovery_fixture(
    workspace_mode: AgentConversationWorkspaceMode,
    linked_session: Option<(IdeationSessionFlow, IdeationSessionStatus)>,
) -> (AppState, ChatConversationId) {
    let state = AppState::new_test();
    let project = state
        .project_repo
        .create(Project::new(
            "Plan handoff recovery".to_string(),
            "/tmp/plan-handoff-recovery".to_string(),
        ))
        .await
        .expect("project should persist");
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::Plan);
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation should persist");
    let conversation_id = conversation.id;

    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project.id.clone(),
        workspace_mode,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        None,
        None,
        "plan-handoff-recovery".to_string(),
        "/tmp/plan-handoff-recovery-worktree".to_string(),
    );
    if let Some((flow, status)) = linked_session {
        let mut session = IdeationSession::new(project.id);
        session.session_flow = flow;
        session.status = status;
        let session = state
            .ideation_session_repo
            .create(session)
            .await
            .expect("linked session should persist");
        workspace.linked_ideation_session_id = Some(session.id);
    }
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let message = startup_handoff_message("recover-1");
    let key = QueueKey::new(ChatContextType::Project, conversation_id.as_str());
    state
        .queued_message_repo
        .enqueue_back(&key, &message)
        .await
        .expect("durable handoff should persist");
    state.message_queue.queue_back_existing(
        ChatContextType::Project,
        conversation_id.as_str(),
        message,
    );

    (state, conversation_id)
}

#[tokio::test]
async fn accepted_plan_mode_handoff_recovery_kicks_exact_valid_row_once() {
    let (state, conversation_id) = setup_accepted_plan_mode_recovery_fixture(
        AgentConversationWorkspaceMode::Plan,
        Some((IdeationSessionFlow::Planning, IdeationSessionStatus::Active)),
    )
    .await;
    let chat_service = Arc::new(MockChatService::with_queue(Arc::clone(
        &state.message_queue,
    )));
    let runner = build_runner_for_tests(&state).with_chat_service(chat_service.clone());

    runner
        .recover_accepted_plan_mode_handoffs_for_state(&state)
        .await;
    runner
        .recover_accepted_plan_mode_handoffs_for_state(&state)
        .await;

    assert_eq!(chat_service.get_sent_messages().await.len(), 1);
    assert!(state
        .message_queue
        .get_queued(ChatContextType::Project, &conversation_id.as_str())
        .is_empty());
}

#[tokio::test]
async fn accepted_plan_mode_handoff_recovery_rejects_invalid_workspace_and_session_links() {
    let cases = [
        (
            AgentConversationWorkspaceMode::Edit,
            Some((IdeationSessionFlow::Planning, IdeationSessionStatus::Active)),
        ),
        (AgentConversationWorkspaceMode::Plan, None),
        (
            AgentConversationWorkspaceMode::Plan,
            Some((IdeationSessionFlow::Ideation, IdeationSessionStatus::Active)),
        ),
        (
            AgentConversationWorkspaceMode::Plan,
            Some((
                IdeationSessionFlow::Planning,
                IdeationSessionStatus::Accepted,
            )),
        ),
    ];

    for (workspace_mode, linked_session) in cases {
        let (state, conversation_id) =
            setup_accepted_plan_mode_recovery_fixture(workspace_mode, linked_session).await;
        let chat_service = Arc::new(MockChatService::with_queue(Arc::clone(
            &state.message_queue,
        )));
        let runner = build_runner_for_tests(&state).with_chat_service(chat_service.clone());

        runner
            .recover_accepted_plan_mode_handoffs_for_state(&state)
            .await;

        assert!(chat_service.get_sent_messages().await.is_empty());
        assert_eq!(
            state
                .message_queue
                .get_queued(ChatContextType::Project, &conversation_id.as_str())
                .len(),
            1,
            "rejected rows must stay durable for explicit remediation"
        );
    }
}

#[tokio::test]
async fn accepted_plan_mode_handoff_recovery_skips_exact_live_conversation_owner() {
    let (state, conversation_id) = setup_accepted_plan_mode_recovery_fixture(
        AgentConversationWorkspaceMode::Plan,
        Some((IdeationSessionFlow::Planning, IdeationSessionStatus::Active)),
    )
    .await;
    state
        .running_agent_registry
        .register(
            RunningAgentKey::new(
                ChatContextType::Project.to_string(),
                conversation_id.as_str(),
            ),
            0,
            conversation_id.as_str().to_string(),
            "live-conversation-run".to_string(),
            None,
            None,
        )
        .await;
    let chat_service = Arc::new(MockChatService::with_queue(Arc::clone(
        &state.message_queue,
    )));
    let runner = build_runner_for_tests(&state).with_chat_service(chat_service.clone());

    runner
        .recover_accepted_plan_mode_handoffs_for_state(&state)
        .await;

    assert!(chat_service.get_sent_messages().await.is_empty());
    assert_eq!(
        state
            .message_queue
            .get_queued(ChatContextType::Project, &conversation_id.as_str())
            .len(),
        1
    );
}

#[tokio::test]
async fn accepted_plan_mode_handoff_recovery_skips_exact_interactive_process_owner() {
    let (state, conversation_id) = setup_accepted_plan_mode_recovery_fixture(
        AgentConversationWorkspaceMode::Plan,
        Some((IdeationSessionFlow::Planning, IdeationSessionStatus::Active)),
    )
    .await;
    let key = InteractiveProcessKey::new("project", conversation_id.as_str());
    let (stdin, mut child) = create_test_stdin().await;
    state
        .interactive_process_registry
        .register_with_metadata(
            key.clone(),
            stdin,
            InteractiveProcessMetadata {
                agent_run_id: Some("live-interactive-run".to_string()),
                ..Default::default()
            },
        )
        .await;
    let chat_service = Arc::new(MockChatService::with_queue(Arc::clone(
        &state.message_queue,
    )));
    let runner = build_runner_for_tests(&state).with_chat_service(chat_service.clone());

    runner
        .recover_accepted_plan_mode_handoffs_for_state(&state)
        .await;

    assert!(chat_service.get_sent_messages().await.is_empty());
    assert_eq!(
        state
            .message_queue
            .get_queued(ChatContextType::Project, &conversation_id.as_str())
            .len(),
        1,
        "interactive ownership must preserve the in-memory handoff row"
    );
    assert_eq!(
        state
            .queued_message_repo
            .list(&QueueKey::new(
                ChatContextType::Project,
                conversation_id.as_str(),
            ))
            .await
            .expect("durable handoff should load")
            .len(),
        1,
        "interactive ownership must preserve the durable handoff row"
    );

    drop(state.interactive_process_registry.remove(&key).await);
    if child.try_wait().expect("inspect stdin fixture").is_none() {
        child.kill().await.expect("stop stdin fixture");
        child.wait().await.expect("reap stdin fixture");
    }
}

#[test]
fn test_auto_recover_with_shutdown_flag() {
    let meta = serde_json::json!({
        "shutdown_interrupted": true,
        "last_agent_error_context": "execution"
    });
    assert!(
        should_auto_recover(&meta),
        "shutdown_interrupted=true with 0 attempts should trigger auto-recovery"
    );
}

#[test]
fn test_auto_recover_with_crash_error_message() {
    let meta = serde_json::json!({
        "last_agent_error": "Agent completed without calling execution_complete",
        "last_agent_error_context": "review"
    });
    assert!(
        should_auto_recover(&meta),
        "last_agent_error containing 'completed without calling' should trigger auto-recovery"
    );
}

#[test]
fn test_no_auto_recover_no_flag_no_error() {
    let meta = serde_json::json!({
        "last_agent_error_context": "execution"
    });
    assert!(
        !should_auto_recover(&meta),
        "No shutdown_interrupted and no crash indicator → no auto-recovery"
    );
}

#[test]
fn test_no_auto_recover_when_attempts_is_one() {
    let meta = serde_json::json!({
        "shutdown_interrupted": true,
        "last_agent_error_context": "execution",
        "startup_recovery_attempts": 1
    });
    assert!(
        !should_auto_recover(&meta),
        "startup_recovery_attempts=1 should prevent further auto-recovery"
    );
}

#[test]
fn test_no_auto_recover_when_attempts_is_two() {
    let meta = serde_json::json!({
        "shutdown_interrupted": true,
        "last_agent_error_context": "execution",
        "startup_recovery_attempts": 2
    });
    assert!(
        !should_auto_recover(&meta),
        "startup_recovery_attempts=2 should prevent auto-recovery"
    );
}

#[test]
fn test_auto_recover_with_zero_attempts_explicit() {
    let meta = serde_json::json!({
        "shutdown_interrupted": true,
        "last_agent_error_context": "execution",
        "startup_recovery_attempts": 0
    });
    assert!(
        should_auto_recover(&meta),
        "Explicit startup_recovery_attempts=0 with flag set should trigger auto-recovery"
    );
}

#[test]
fn test_no_auto_recover_empty_metadata() {
    let meta = serde_json::json!({});
    assert!(
        !should_auto_recover(&meta),
        "Empty metadata → no auto-recovery"
    );
}

#[test]
fn test_auto_recover_with_both_shutdown_and_crash() {
    let meta = serde_json::json!({
        "shutdown_interrupted": true,
        "last_agent_error": "Agent completed without calling execution_complete",
        "last_agent_error_context": "merge"
    });
    assert!(
        should_auto_recover(&meta),
        "Both shutdown_interrupted and crash indicator should trigger auto-recovery"
    );
}

#[test]
fn test_no_auto_recover_crash_indicator_false_positive_string() {
    // Error message that does NOT contain the exact phrase
    let meta = serde_json::json!({
        "last_agent_error": "Some other error occurred",
        "last_agent_error_context": "execution"
    });
    assert!(
        !should_auto_recover(&meta),
        "Generic error message without crash indicator phrase → no auto-recovery"
    );
}

#[test]
fn test_no_auto_recover_shutdown_false() {
    let meta = serde_json::json!({
        "shutdown_interrupted": false,
        "last_agent_error_context": "execution"
    });
    assert!(
        !should_auto_recover(&meta),
        "shutdown_interrupted=false with no crash indicator → no auto-recovery"
    );
}

// ======= Integration tests for recover_crash_escalated_tasks() =======

fn build_runner_for_tests(app_state: &AppState) -> StartupJobRunner {
    let execution_state = Arc::new(ExecutionState::new());
    let transition_service = Arc::new(TaskTransitionService::new(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.agent_run_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&execution_state),
        None,
        Arc::clone(&app_state.memory_event_repo),
    ));
    StartupJobRunner::new(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.artifact_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&app_state.memory_event_repo),
        Arc::clone(&app_state.agent_run_repo),
        transition_service,
        execution_state,
        Arc::new(ActiveProjectState::new()),
        Arc::clone(&app_state.app_state_repo),
        Arc::clone(&app_state.execution_settings_repo),
        None,
    )
}

#[tokio::test]
async fn post_ready_safety_net_runs_deferred_dependency_checks() {
    let app_state = AppState::new_test();
    let project = Project::new("Safety Net Project".into(), "/tmp/safety-net".into());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut ready_task = Task::new(project.id.clone(), "Ready task".into());
    ready_task.internal_status = InternalStatus::Ready;
    let ready_task = app_state.task_repo.create(ready_task).await.unwrap();

    let runner = build_runner_for_tests(&app_state);
    runner.spawn_post_ready_safety_net(Duration::ZERO);

    tokio::time::sleep(Duration::from_millis(50)).await;

    let stored = app_state
        .task_repo
        .get_by_id(&ready_task.id)
        .await
        .unwrap()
        .expect("ready task should remain available");
    assert_eq!(stored.internal_status, InternalStatus::Ready);
}

#[tokio::test]
async fn startup_unblock_marks_blocked_task_ready_when_blockers_are_complete() {
    let app_state = AppState::new_test();
    let project = app_state
        .project_repo
        .create(Project::new(
            "Unblock Project".into(),
            "/tmp/unblock".into(),
        ))
        .await
        .unwrap();

    let mut blocker = Task::new(project.id.clone(), "Merged blocker".into());
    blocker.internal_status = InternalStatus::Merged;
    let blocker = app_state.task_repo.create(blocker).await.unwrap();

    let mut blocked = Task::new(project.id.clone(), "Blocked dependent".into());
    blocked.internal_status = InternalStatus::Blocked;
    blocked.blocked_reason = Some("Waiting for blocker".into());
    let blocked = app_state.task_repo.create(blocked).await.unwrap();

    app_state
        .task_dependency_repo
        .add_dependency(&blocked.id, &blocker.id)
        .await
        .unwrap();

    StartupJobRunner::unblock_ready_tasks_for(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.project_repo),
        None,
    )
    .await;

    let stored = app_state
        .task_repo
        .get_by_id(&blocked.id)
        .await
        .unwrap()
        .expect("blocked task should exist");
    assert_eq!(stored.internal_status, InternalStatus::Ready);
    assert_eq!(stored.blocked_reason, None);
}

#[tokio::test]
async fn dependency_reconciliation_reblocks_ready_task_with_failed_blocker() {
    let app_state = AppState::new_test();
    let project = app_state
        .project_repo
        .create(Project::new(
            "Dependency Reconcile Project".into(),
            "/tmp/dependency-reconcile".into(),
        ))
        .await
        .unwrap();

    let mut blocker = Task::new(project.id.clone(), "Failed blocker".into());
    blocker.internal_status = InternalStatus::Failed;
    let blocker = app_state.task_repo.create(blocker).await.unwrap();

    let mut ready = Task::new(project.id.clone(), "Ready dependent".into());
    ready.internal_status = InternalStatus::Ready;
    let ready = app_state.task_repo.create(ready).await.unwrap();

    app_state
        .task_dependency_repo
        .add_dependency(&ready.id, &blocker.id)
        .await
        .unwrap();

    StartupJobRunner::reconcile_dependency_violations_for(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.project_repo),
        None,
    )
    .await;

    let stored = app_state
        .task_repo
        .get_by_id(&ready.id)
        .await
        .unwrap()
        .expect("ready task should exist");
    assert_eq!(stored.internal_status, InternalStatus::Blocked);
    assert_eq!(
        stored.blocked_reason.as_deref(),
        Some("Waiting for: \"Failed blocker\" (failed)")
    );
}

#[tokio::test]
async fn test_previous_session_cutoff_cleanup_preserves_current_boot_agents_and_runs() {
    let app_state = AppState::new_test();
    let old_key = RunningAgentKey::new("project", "old-conversation");
    let current_key = RunningAgentKey::new("project", "current-conversation");
    let old_token = tokio_util::sync::CancellationToken::new();
    let current_token = tokio_util::sync::CancellationToken::new();

    let old_run = AgentRun::new(ChatConversationId::new());
    let old_run_id = old_run.id;
    app_state.agent_run_repo.create(old_run).await.unwrap();
    app_state
        .running_agent_registry
        .register(
            old_key.clone(),
            1,
            "conv-old".to_string(),
            old_run_id.as_str().to_string(),
            None,
            Some(old_token.clone()),
        )
        .await;

    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    let boot_cutoff = chrono::Utc::now();
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;

    let current_run = AgentRun::new(ChatConversationId::new());
    let current_run_id = current_run.id;
    app_state.agent_run_repo.create(current_run).await.unwrap();
    app_state
        .running_agent_registry
        .register(
            current_key.clone(),
            1,
            "conv-current".to_string(),
            current_run_id.as_str().to_string(),
            None,
            Some(current_token.clone()),
        )
        .await;

    let runner = build_runner_for_tests(&app_state).with_previous_session_cutoff(boot_cutoff);

    runner.run().await;

    assert!(!app_state.running_agent_registry.is_running(&old_key).await);
    assert!(
        app_state
            .running_agent_registry
            .is_running(&current_key)
            .await
    );
    assert!(old_token.is_cancelled());
    assert!(!current_token.is_cancelled());

    let old = app_state
        .agent_run_repo
        .get_by_id(&old_run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(old.status, AgentRunStatus::Cancelled);
    assert_eq!(
        old.error_message,
        Some(ORPHANED_AGENT_RUN_ON_APP_RESTART.to_string())
    );

    let current = app_state
        .agent_run_repo
        .get_by_id(&current_run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current.status, AgentRunStatus::Running);
    assert_eq!(current.error_message, None);
}

fn make_escalated_task(project_id: &ProjectId, metadata: serde_json::Value) -> Task {
    let mut task = Task::new(project_id.clone(), "test task".into());
    task.internal_status = InternalStatus::Escalated;
    task.metadata = Some(metadata.to_string());
    task
}

fn make_active_task(
    project_id: &ProjectId,
    status: InternalStatus,
    metadata: serde_json::Value,
) -> Task {
    let mut task = Task::new(project_id.clone(), "active test task".into());
    task.internal_status = status;
    task.metadata = Some(metadata.to_string());
    task
}

#[tokio::test]
async fn test_prepare_active_task_startup_resume_marks_execution_for_resume() {
    let app_state = AppState::new_test();
    let project = Project::new("Test Project".into(), "/tmp/test-project".into());
    let project_id = project.id.clone();
    app_state.project_repo.create(project).await.unwrap();

    let meta = serde_json::json!({
        "shutdown_interrupted": true,
        "last_agent_error_context": "execution",
        "startup_recovery_attempts": 0
    });
    let task = make_active_task(&project_id, InternalStatus::Executing, meta);
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    let runner = build_runner_for_tests(&app_state);
    let stored = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should exist");

    let interrupted_contexts = std::collections::HashSet::new();
    let resumed = runner
        .prepare_active_task_startup_resume(
            &stored,
            InternalStatus::Executing,
            &interrupted_contexts,
        )
        .await
        .expect("shutdown-interrupted execution task should be resumed");

    assert_eq!(resumed.internal_status, InternalStatus::Executing);
    let resumed_meta: serde_json::Value = resumed
        .metadata
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    assert_eq!(
        resumed_meta
            .get("startup_recovery_attempts")
            .and_then(|v| v.as_u64()),
        Some(1),
        "startup auto-resume must increment the attempt counter"
    );
    assert_eq!(
        resumed_meta
            .get("startup_recovery_source")
            .and_then(|v| v.as_str()),
        Some("shutdown_interrupted_metadata"),
        "startup auto-resume should record why the task was claimed before generic reconciliation"
    );
    assert!(
        resumed_meta.get("shutdown_interrupted").is_none(),
        "startup auto-resume must clear the shutdown_interrupted flag"
    );

    let persisted = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should still exist");
    assert_eq!(persisted.metadata, resumed.metadata);
}

#[tokio::test]
async fn test_prepare_active_task_startup_resume_rejects_mismatched_context() {
    let app_state = AppState::new_test();
    let project = Project::new("Test Project".into(), "/tmp/test-project".into());
    let project_id = project.id.clone();
    app_state.project_repo.create(project).await.unwrap();

    let meta = serde_json::json!({
        "shutdown_interrupted": true,
        "last_agent_error_context": "review",
        "startup_recovery_attempts": 0
    });
    let task = make_active_task(&project_id, InternalStatus::Executing, meta);
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    let runner = build_runner_for_tests(&app_state);
    let stored = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should exist");

    let interrupted_contexts = std::collections::HashSet::new();
    let resumed = runner
        .prepare_active_task_startup_resume(
            &stored,
            InternalStatus::Executing,
            &interrupted_contexts,
        )
        .await;
    assert!(
        resumed.is_none(),
        "review recovery metadata must not auto-resume an executing task"
    );

    let persisted = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should still exist");
    let persisted_meta: serde_json::Value = persisted
        .metadata
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    assert_eq!(
        persisted_meta
            .get("startup_recovery_attempts")
            .and_then(|v| v.as_u64()),
        Some(0),
        "non-resumable active tasks must not consume the startup retry budget"
    );
    assert_eq!(
        persisted_meta
            .get("shutdown_interrupted")
            .and_then(|v| v.as_bool()),
        Some(true),
        "non-resumable active tasks must keep the interruption marker unchanged"
    );
}

#[tokio::test]
async fn test_prepare_active_task_startup_resume_accepts_persisted_registry_claim() {
    let app_state = AppState::new_test();
    let project = Project::new("Test Project".into(), "/tmp/test-project".into());
    let project_id = project.id.clone();
    app_state.project_repo.create(project).await.unwrap();

    let task = make_active_task(
        &project_id,
        InternalStatus::Executing,
        serde_json::json!({ "startup_recovery_attempts": 0 }),
    );
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    let runner = build_runner_for_tests(&app_state);
    let stored = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should exist");

    let interrupted_contexts =
        std::collections::HashSet::from([RunningAgentKey::new("task_execution", task_id.as_str())]);
    let resumed = runner
        .prepare_active_task_startup_resume(
            &stored,
            InternalStatus::Executing,
            &interrupted_contexts,
        )
        .await
        .expect("persisted running-agent registry claim should be resumed");

    let resumed_meta: serde_json::Value = resumed
        .metadata
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    assert_eq!(
        resumed_meta
            .get("startup_recovery_source")
            .and_then(|v| v.as_str()),
        Some("persisted_running_agent"),
        "persisted registry claims should bypass generic stale-task reconciliation once"
    );
    assert_eq!(
        resumed_meta
            .get("startup_recovery_attempts")
            .and_then(|v| v.as_u64()),
        Some(1),
        "persisted registry claims should consume the one-shot startup recovery budget"
    );
}

#[tokio::test]
async fn test_recovery_shutdown_interrupted_review_transitions_to_pending_review() {
    let app_state = AppState::new_test();
    let project = Project::new("Test Project".into(), "/tmp/test-project".into());
    let project_id = project.id.clone();
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let meta = serde_json::json!({
        "shutdown_interrupted": true,
        "last_agent_error_context": "review"
    });
    let task = make_escalated_task(&project_id, meta);
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    let runner = build_runner_for_tests(&app_state);
    let recovered = runner.recover_crash_escalated_tasks(&[project]).await;

    assert_eq!(recovered, 1, "One task should have been recovered");

    let updated = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("Task should still exist");
    assert_eq!(
        updated.internal_status,
        InternalStatus::PendingReview,
        "Shutdown-interrupted review task should transition to PendingReview"
    );
}

#[tokio::test]
async fn test_recovery_crash_execution_transitions_to_ready() {
    let app_state = AppState::new_test();
    let project = Project::new("Test Project".into(), "/tmp/test-project".into());
    let project_id = project.id.clone();
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let meta = serde_json::json!({
        "last_agent_error": "Agent completed without calling execution_complete",
        "last_agent_error_context": "execution"
    });
    let task = make_escalated_task(&project_id, meta);
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    let runner = build_runner_for_tests(&app_state);
    let recovered = runner.recover_crash_escalated_tasks(&[project]).await;

    assert_eq!(recovered, 1, "One task should have been recovered");

    let updated = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("Task should still exist");
    assert_eq!(
        updated.internal_status,
        InternalStatus::Ready,
        "Crash execution task should transition to Ready"
    );
}

#[tokio::test]
async fn test_recovery_shutdown_interrupted_merge_transitions_to_pending_merge() {
    let app_state = AppState::new_test();
    let project = Project::new("Test Project".into(), "/tmp/test-project".into());
    let project_id = project.id.clone();
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let meta = serde_json::json!({
        "shutdown_interrupted": true,
        "last_agent_error_context": "merge"
    });
    let task = make_escalated_task(&project_id, meta);
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    let runner = build_runner_for_tests(&app_state);
    let recovered = runner.recover_crash_escalated_tasks(&[project]).await;

    assert_eq!(recovered, 1, "One task should have been recovered");

    let updated = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("Task should still exist");
    assert_eq!(
        updated.internal_status,
        InternalStatus::PendingMerge,
        "Shutdown-interrupted merge task should transition to PendingMerge"
    );
}

#[tokio::test]
async fn test_no_recovery_genuine_escalation_stays_escalated() {
    let app_state = AppState::new_test();
    let project = Project::new("Test Project".into(), "/tmp/test-project".into());
    let project_id = project.id.clone();
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // No shutdown_interrupted, no crash indicator → genuine escalation
    let meta = serde_json::json!({
        "escalation_reason": "Human review required",
        "last_agent_error_context": "review"
    });
    let task = make_escalated_task(&project_id, meta);
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    let runner = build_runner_for_tests(&app_state);
    let recovered = runner.recover_crash_escalated_tasks(&[project]).await;

    assert_eq!(recovered, 0, "Genuine escalation should not be recovered");

    let updated = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("Task should still exist");
    assert_eq!(
        updated.internal_status,
        InternalStatus::Escalated,
        "Genuinely escalated task should remain Escalated"
    );
}

#[tokio::test]
async fn test_no_recovery_retry_limit_stays_escalated() {
    let app_state = AppState::new_test();
    let project = Project::new("Test Project".into(), "/tmp/test-project".into());
    let project_id = project.id.clone();
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let meta = serde_json::json!({
        "shutdown_interrupted": true,
        "last_agent_error_context": "execution",
        "startup_recovery_attempts": 1
    });
    let task = make_escalated_task(&project_id, meta);
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    let runner = build_runner_for_tests(&app_state);
    let recovered = runner.recover_crash_escalated_tasks(&[project]).await;

    assert_eq!(
        recovered, 0,
        "Task with startup_recovery_attempts=1 should not be recovered again"
    );

    let updated = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("Task should still exist");
    assert_eq!(
        updated.internal_status,
        InternalStatus::Escalated,
        "Task past retry limit should remain Escalated"
    );
}

#[tokio::test]
async fn test_recovery_increments_startup_recovery_attempts() {
    let app_state = AppState::new_test();
    let project = Project::new("Test Project".into(), "/tmp/test-project".into());
    let project_id = project.id.clone();
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let meta = serde_json::json!({
        "shutdown_interrupted": true,
        "last_agent_error_context": "execution",
        "startup_recovery_attempts": 0
    });
    let task = make_escalated_task(&project_id, meta);
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    let runner = build_runner_for_tests(&app_state);
    runner.recover_crash_escalated_tasks(&[project]).await;

    let updated = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("Task should still exist");

    let updated_meta: serde_json::Value = updated
        .metadata
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();

    let attempts = updated_meta
        .get("startup_recovery_attempts")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    assert_eq!(
        attempts, 1,
        "startup_recovery_attempts should be incremented to 1 after recovery"
    );
}

#[tokio::test]
async fn test_recovery_clears_shutdown_interrupted_flag() {
    let app_state = AppState::new_test();
    let project = Project::new("Test Project".into(), "/tmp/test-project".into());
    let project_id = project.id.clone();
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let meta = serde_json::json!({
        "shutdown_interrupted": true,
        "last_agent_error_context": "review"
    });
    let task = make_escalated_task(&project_id, meta);
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    let runner = build_runner_for_tests(&app_state);
    runner.recover_crash_escalated_tasks(&[project]).await;

    let updated = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("Task should still exist");

    let updated_meta: serde_json::Value = updated
        .metadata
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();

    assert!(
        updated_meta.get("shutdown_interrupted").is_none(),
        "shutdown_interrupted flag should be removed from metadata after recovery"
    );
}

#[tokio::test]
async fn test_no_recovery_missing_error_context_stays_escalated() {
    let app_state = AppState::new_test();
    let project = Project::new("Test Project".into(), "/tmp/test-project".into());
    let project_id = project.id.clone();
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Has the crash indicator but NO last_agent_error_context → can't determine target state
    let meta = serde_json::json!({
        "shutdown_interrupted": true
        // No last_agent_error_context
    });
    let task = make_escalated_task(&project_id, meta);
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    let runner = build_runner_for_tests(&app_state);
    let recovered = runner.recover_crash_escalated_tasks(&[project]).await;

    assert_eq!(
        recovered, 0,
        "Missing last_agent_error_context should prevent recovery (unknown target state)"
    );

    let updated = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("Task should still exist");
    assert_eq!(
        updated.internal_status,
        InternalStatus::Escalated,
        "Task without error context should remain Escalated"
    );
}

#[tokio::test]
async fn test_recovery_multiple_tasks_counts_correctly() {
    let app_state = AppState::new_test();
    let project = Project::new("Test Project".into(), "/tmp/test-project".into());
    let project_id = project.id.clone();
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Task 1: recoverable (shutdown interrupted)
    let meta1 = serde_json::json!({
        "shutdown_interrupted": true,
        "last_agent_error_context": "execution"
    });
    let task1 = make_escalated_task(&project_id, meta1);
    app_state.task_repo.create(task1).await.unwrap();

    // Task 2: recoverable (crash indicator)
    let meta2 = serde_json::json!({
        "last_agent_error": "Agent completed without calling execution_complete",
        "last_agent_error_context": "review"
    });
    let task2 = make_escalated_task(&project_id, meta2);
    app_state.task_repo.create(task2).await.unwrap();

    // Task 3: NOT recoverable (genuine escalation)
    let meta3 = serde_json::json!({
        "last_agent_error_context": "execution"
    });
    let task3 = make_escalated_task(&project_id, meta3);
    app_state.task_repo.create(task3).await.unwrap();

    let runner = build_runner_for_tests(&app_state);
    let recovered = runner.recover_crash_escalated_tasks(&[project]).await;

    assert_eq!(recovered, 2, "Exactly 2 of the 3 tasks should be recovered");
}

#[tokio::test]
async fn test_no_recovery_archived_task_skipped() {
    let app_state = AppState::new_test();
    let project = Project::new("Test Project".into(), "/tmp/test-project".into());
    let project_id = project.id.clone();
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let meta = serde_json::json!({
        "shutdown_interrupted": true,
        "last_agent_error_context": "execution"
    });
    let mut task = make_escalated_task(&project_id, meta);
    // Mark as archived
    task.archived_at = Some(chrono::Utc::now());
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    let runner = build_runner_for_tests(&app_state);
    let recovered = runner.recover_crash_escalated_tasks(&[project]).await;

    assert_eq!(recovered, 0, "Archived tasks should be skipped");

    let updated = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("Task should still exist");
    assert_eq!(
        updated.internal_status,
        InternalStatus::Escalated,
        "Archived task should remain Escalated"
    );
}

// ======= Unit tests for recover_ideation_session() =======

/// Helper: build a runner wired with a MockChatService.
fn build_runner_with_chat_service(
    app_state: &AppState,
    chat_service: Arc<dyn ChatService>,
) -> StartupJobRunner {
    let execution_state = Arc::new(ExecutionState::new());
    let transition_service = Arc::new(TaskTransitionService::new(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.agent_run_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&execution_state),
        None,
        Arc::clone(&app_state.memory_event_repo),
    ));
    StartupJobRunner::new(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.artifact_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&app_state.memory_event_repo),
        Arc::clone(&app_state.agent_run_repo),
        transition_service,
        execution_state,
        Arc::new(ActiveProjectState::new()),
        Arc::clone(&app_state.app_state_repo),
        Arc::clone(&app_state.execution_settings_repo),
        None,
    )
    .with_chat_service(chat_service)
}

/// Helper: create and persist an IdeationSession with the given status.
async fn create_session(
    app_state: &AppState,
    project_id: &ProjectId,
    status: IdeationSessionStatus,
) -> IdeationSession {
    let mut session = IdeationSession::new(project_id.clone());
    session.status = status;
    app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap()
}

#[tokio::test]
async fn test_recover_ideation_session_active_calls_send_message() {
    let app_state = AppState::new_test();
    let project = Project::new("Test Project".into(), "/tmp/test-project".into());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create an active ideation session.
    let session = create_session(&app_state, &project.id, IdeationSessionStatus::Active).await;
    let session_id = session.id.as_str().to_string();

    let mock = Arc::new(MockChatService::new());
    let _runner =
        build_runner_with_chat_service(&app_state, Arc::clone(&mock) as Arc<dyn ChatService>);

    // Call recover_ideation_session directly (tests the helper without spinning up the full runner).
    let item = crate::application::recovery_queue::RecoveryItem {
        context_type: "ideation".to_string(),
        context_id: session_id.clone(),
        conversation_id: "conv-1".to_string(),
        priority: crate::application::recovery_queue::RecoveryPriority::Ideation,
        started_at: chrono::Utc::now(),
    };

    let result = StartupJobRunner::recover_ideation_session(
        item,
        mock.as_ref(),
        app_state.ideation_session_repo.as_ref(),
        None,
    )
    .await;

    assert!(result.is_ok(), "Recovery should succeed for active session");
    assert_eq!(mock.call_count(), 1, "send_message should be called once");
}

#[tokio::test]
async fn test_recover_ideation_session_skips_when_not_found() {
    let app_state = AppState::new_test();

    let mock = Arc::new(MockChatService::new());

    let item = crate::application::recovery_queue::RecoveryItem {
        context_type: "ideation".to_string(),
        context_id: "nonexistent-session-id".to_string(),
        conversation_id: "conv-x".to_string(),
        priority: crate::application::recovery_queue::RecoveryPriority::Ideation,
        started_at: chrono::Utc::now(),
    };

    let result = StartupJobRunner::recover_ideation_session(
        item,
        mock.as_ref(),
        app_state.ideation_session_repo.as_ref(),
        None,
    )
    .await;

    // Should return Ok (intentional skip), not an error.
    assert!(
        result.is_ok(),
        "Not-found session should be silently skipped"
    );
    assert_eq!(mock.call_count(), 0, "send_message should NOT be called");
}

#[tokio::test]
async fn test_recover_ideation_session_skips_when_archived() {
    let app_state = AppState::new_test();
    let project = Project::new("Test Project".into(), "/tmp/test-project".into());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create an archived (non-active) session.
    let session = create_session(&app_state, &project.id, IdeationSessionStatus::Archived).await;

    let mock = Arc::new(MockChatService::new());

    let item = crate::application::recovery_queue::RecoveryItem {
        context_type: "ideation".to_string(),
        context_id: session.id.as_str().to_string(),
        conversation_id: "conv-2".to_string(),
        priority: crate::application::recovery_queue::RecoveryPriority::Ideation,
        started_at: chrono::Utc::now(),
    };

    let result = StartupJobRunner::recover_ideation_session(
        item,
        mock.as_ref(),
        app_state.ideation_session_repo.as_ref(),
        None,
    )
    .await;

    assert!(
        result.is_ok(),
        "Archived session should be silently skipped, not an error"
    );
    assert_eq!(
        mock.call_count(),
        0,
        "send_message should NOT be called for archived session"
    );
}

#[tokio::test]
async fn test_recover_ideation_session_skips_when_accepted() {
    let app_state = AppState::new_test();
    let project = Project::new("Test Project".into(), "/tmp/test-project".into());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let session = create_session(&app_state, &project.id, IdeationSessionStatus::Accepted).await;

    let mock = Arc::new(MockChatService::new());

    let item = crate::application::recovery_queue::RecoveryItem {
        context_type: "ideation".to_string(),
        context_id: session.id.as_str().to_string(),
        conversation_id: "conv-3".to_string(),
        priority: crate::application::recovery_queue::RecoveryPriority::Ideation,
        started_at: chrono::Utc::now(),
    };

    let result = StartupJobRunner::recover_ideation_session(
        item,
        mock.as_ref(),
        app_state.ideation_session_repo.as_ref(),
        None,
    )
    .await;

    assert!(
        result.is_ok(),
        "Accepted session should be silently skipped, not an error"
    );
    assert_eq!(
        mock.call_count(),
        0,
        "send_message should NOT be called for accepted session"
    );
}

#[tokio::test]
async fn test_run_skips_ideation_recovery_when_persisted_stop_barrier_is_set() {
    let app_state = AppState::new_test();
    let project = Project::new("Test Project".into(), "/tmp/test-project".into());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let session = create_session(&app_state, &project.id, IdeationSessionStatus::Active).await;

    app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("ideation", session.id.as_str()),
            123,
            "conv-stopped".to_string(),
            "run-stopped".to_string(),
            None,
            None,
        )
        .await;

    app_state
        .app_state_repo
        .set_execution_halt_mode(ExecutionHaltMode::Stopped)
        .await
        .unwrap();

    let mock = Arc::new(MockChatService::new());
    let runner =
        build_runner_with_chat_service(&app_state, Arc::clone(&mock) as Arc<dyn ChatService>);

    let claimed = runner.run().await;

    assert_eq!(
        mock.call_count(),
        0,
        "Persisted stop barrier must suppress Phase N+1 ideation recovery"
    );
    assert!(
        claimed.is_empty(),
        "stop barrier should not claim any ideation sessions for Phase N+1 recovery"
    );
}

#[tokio::test]
async fn test_run_refreshes_phase_n1_ideation_sessions_before_recovery() {
    let app_state = AppState::new_test();
    let project = Project::new("Test Project".into(), "/tmp/test-project".into());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut session = IdeationSession::new(project.id.clone());
    session.status = IdeationSessionStatus::Active;
    session.origin = SessionOrigin::External;
    session.external_activity_phase = Some("created".to_string());
    session.updated_at = chrono::Utc::now() - chrono::Duration::hours(5);
    let session_id = session.id.clone();
    let old_updated_at = session.updated_at;
    app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("ideation", session_id.as_str()),
            123,
            "conv-refresh".to_string(),
            "run-refresh".to_string(),
            None,
            None,
        )
        .await;

    app_state
        .app_state_repo
        .set_active_project(Some(&project.id))
        .await
        .unwrap();

    let mock = Arc::new(MockChatService::new());
    let runner =
        build_runner_with_chat_service(&app_state, Arc::clone(&mock) as Arc<dyn ChatService>);

    let claimed = runner.run().await;

    let refreshed = app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        refreshed.updated_at > old_updated_at,
        "Phase N+1 ideation sessions should be touched before startup recovery so cold-boot archival does not sweep them"
    );
    assert!(
        claimed.contains(session_id.as_str()),
        "startup runner should report ideation sessions claimed for Phase N+1 recovery"
    );
}
