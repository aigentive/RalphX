use std::sync::Arc;

use crate::application::interactive_process_registry::{
    InteractiveProcessKey, InteractiveProcessRegistry,
};
use crate::application::prune_engine::{
    should_defer_pid_missing_prune, should_defer_terminal_settlement_prune,
};
use crate::application::{AppState, PruneEngine};
use crate::commands::execution_commands::ExecutionState;
use crate::domain::entities::{
    AgentRun, AgentRunId, AgentRunStatus, ChatContextType, ChatConversationId, InternalStatus,
    Project, Task,
};
use crate::domain::repositories::PRUNED_STALE_AGENT_RUN;
use crate::domain::services::{RunningAgentInfo, RunningAgentKey};
use crate::utils::path_safety::{require_under_root, validate_absolute_non_root_path};

// ─────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────

/// Build a PruneEngine wired to the given AppState's repos, with an optional IPR.
fn build_engine(app_state: &AppState, ipr: Option<Arc<InteractiveProcessRegistry>>) -> PruneEngine {
    PruneEngine::new(
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&app_state.agent_run_repo),
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.project_repo),
        ipr,
    )
}

/// Register a stale registry entry using a guaranteed non-existent PID.
async fn register_stale_entry(
    app_state: &AppState,
    key: &RunningAgentKey,
    run_id: &AgentRunId,
    worktree_path: Option<String>,
) {
    app_state
        .running_agent_registry
        .register(
            key.clone(),
            999_999, // guaranteed non-existent PID
            "conv-test".to_string(),
            run_id.as_str(),
            worktree_path,
            None,
        )
        .await;
}

/// Register an entry with PID 0 (in-flight, no agent_run_id yet).
async fn register_in_flight_entry(app_state: &AppState, key: &RunningAgentKey) {
    app_state
        .running_agent_registry
        .register(
            key.clone(),
            0,
            "conv-in-flight".to_string(),
            String::new(), // empty agent_run_id marks in-flight
            None,
            None,
        )
        .await;
}

/// Create an AgentRun in Running status and persist it.
async fn create_running_agent_run(app_state: &AppState) -> AgentRunId {
    let run = AgentRun::new(ChatConversationId::new());
    let id = run.id;
    app_state.agent_run_repo.create(run).await.unwrap();
    id
}

// ─────────────────────────────────────────────
// check_ipr_skip tests
// ─────────────────────────────────────────────

#[tokio::test]
async fn check_ipr_skip_no_ipr_registry_always_false() {
    let app_state = AppState::new_test();
    let engine = build_engine(&app_state, None);
    let key = RunningAgentKey::new("task_execution", "task-1");

    // With no IPR registry, check_ipr_skip always returns false.
    assert!(!engine.check_ipr_skip(&key, true).await);
    assert!(!engine.check_ipr_skip(&key, false).await);
}

#[tokio::test]
async fn check_ipr_skip_no_entry_in_ipr_returns_false() {
    let app_state = AppState::new_test();
    let ipr = Arc::new(InteractiveProcessRegistry::new());
    let engine = build_engine(&app_state, Some(Arc::clone(&ipr)));
    let key = RunningAgentKey::new("task_execution", "task-1");

    // IPR has no entry for this key — not interactive, don't skip.
    assert!(!engine.check_ipr_skip(&key, true).await);
}

#[tokio::test]
async fn check_ipr_skip_alive_pid_returns_true() {
    let app_state = AppState::new_test();
    let ipr = Arc::new(InteractiveProcessRegistry::new());

    // Spawn a real process to get a live stdin handle for the IPR.
    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("failed to spawn cat");
    let stdin = child.stdin.take().expect("no stdin");

    let key = RunningAgentKey::new("task_execution", "task-alive");
    let ipr_key = InteractiveProcessKey::new("task_execution", "task-alive");
    ipr.register(ipr_key.clone(), stdin).await;

    let engine = build_engine(&app_state, Some(Arc::clone(&ipr)));

    // IPR has entry + pid_alive=true → skip (returns true).
    assert!(engine.check_ipr_skip(&key, true).await);

    // IPR entry should still be there (alive = not removed).
    assert!(ipr.has_process(&ipr_key).await);

    let _ = child.kill().await;
}

#[tokio::test]
async fn check_ipr_skip_dead_pid_removes_stale_entry_returns_false() {
    let app_state = AppState::new_test();
    let ipr = Arc::new(InteractiveProcessRegistry::new());

    // Spawn a process and immediately kill it to get a definitely-dead stdin.
    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("failed to spawn cat");
    let stdin = child.stdin.take().expect("no stdin");
    let _ = child.kill().await;

    let key = RunningAgentKey::new("task_execution", "task-dead");
    let ipr_key = InteractiveProcessKey::new("task_execution", "task-dead");
    ipr.register(ipr_key.clone(), stdin).await;

    let engine = build_engine(&app_state, Some(Arc::clone(&ipr)));

    // IPR has stale entry (PID dead) → remove it and return false.
    assert!(!engine.check_ipr_skip(&key, false).await);
    assert!(
        !ipr.has_process(&ipr_key).await,
        "stale IPR entry should have been removed"
    );
}

// ─────────────────────────────────────────────
// evaluate_and_prune tests
// ─────────────────────────────────────────────

#[tokio::test]
async fn evaluate_and_prune_in_flight_entry_skipped() {
    let app_state = AppState::new_test();
    let engine = build_engine(&app_state, None);
    let key = RunningAgentKey::new("task_execution", "task-1");

    register_in_flight_entry(&app_state, &key).await;

    let entries = app_state.running_agent_registry.list_all().await;
    let (_, info) = entries.iter().find(|(k, _)| k == &key).unwrap();

    // In-flight entries (empty agent_run_id) must never be pruned.
    let pruned = engine.evaluate_and_prune(&key, info, false).await;
    assert!(!pruned);
    assert!(
        app_state.running_agent_registry.is_running(&key).await,
        "in-flight entry should remain registered"
    );
}

#[tokio::test]
async fn evaluate_and_prune_keeps_fresh_owned_reservation_without_run_row() {
    let app_state = AppState::new_test();
    let engine = build_engine(&app_state, None);
    let key = RunningAgentKey::new("project", "slow-launch");
    app_state
        .running_agent_registry
        .try_register(key.clone(), "conversation".into(), "run-launch".into())
        .await
        .unwrap();

    let info = app_state.running_agent_registry.get(&key).await.unwrap();
    assert!(!engine.evaluate_and_prune(&key, &info, false).await);
    assert!(app_state.running_agent_registry.is_running(&key).await);
}

#[tokio::test]
async fn evaluate_and_prune_removes_expired_owned_reservation() {
    let app_state = AppState::new_test();
    let engine = build_engine(&app_state, None);
    let key = RunningAgentKey::new("project", "abandoned-launch");
    app_state
        .running_agent_registry
        .try_register(key.clone(), "conversation".into(), "run-launch".into())
        .await
        .unwrap();
    let lease = i64::try_from(
        crate::infrastructure::agents::claude::stream_timeouts().launch_reservation_lease_secs,
    )
    .unwrap();
    assert!(app_state
        .running_agent_registry
        .renew_reservation(
            &key,
            "run-launch",
            chrono::Utc::now() - chrono::Duration::seconds(lease + 1),
        )
        .await
        .unwrap());

    let info = app_state.running_agent_registry.get(&key).await.unwrap();
    assert!(engine.evaluate_and_prune(&key, &info, false).await);
    assert!(!app_state.running_agent_registry.is_running(&key).await);
}

#[tokio::test]
async fn evaluate_and_prune_healthy_entry_not_pruned() {
    let app_state = AppState::new_test();

    let project = Project::new("P".to_string(), "/test".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    let mut task = Task::new(project.id.clone(), "T".to_string());
    task.internal_status = InternalStatus::Executing;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let run_id = create_running_agent_run(&app_state).await;
    let key = RunningAgentKey::new("task_execution", task.id.as_str());
    app_state
        .running_agent_registry
        .register(
            key.clone(),
            std::process::id(), // current process — definitely alive
            "conv-healthy".to_string(),
            run_id.as_str(),
            None,
            None,
        )
        .await;

    let engine = build_engine(&app_state, None);
    let entries = app_state.running_agent_registry.list_all().await;
    let (_, info) = entries.iter().find(|(k, _)| k == &key).unwrap();

    // pid alive + run Running + task Executing → healthy, not pruned.
    let pruned = engine.evaluate_and_prune(&key, info, true).await;
    assert!(!pruned);
    assert!(app_state.running_agent_registry.is_running(&key).await);
}

#[tokio::test]
async fn evaluate_and_prune_dead_pid_with_fresh_heartbeat_waits_for_completion() {
    let app_state = AppState::new_test();
    let run_id = create_running_agent_run(&app_state).await;
    let key = RunningAgentKey::new("project", "fresh-stream-drain");
    register_stale_entry(&app_state, &key, &run_id, None).await;

    let engine = build_engine(&app_state, None);
    let info = app_state.running_agent_registry.get(&key).await.unwrap();

    let pruned = engine.evaluate_and_prune(&key, &info, false).await;

    assert!(
        !pruned,
        "fresh dead-PID entry should receive completion grace"
    );
    assert!(app_state.running_agent_registry.is_running(&key).await);
    let run = app_state
        .agent_run_repo
        .get_by_id(&run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.status, AgentRunStatus::Running);
    assert!(run.error_message.is_none());
}

#[tokio::test]
async fn evaluate_and_prune_dead_pid_after_grace_marks_and_cancels_run() {
    let app_state = AppState::new_test();

    let project = Project::new("P".to_string(), "/test".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    let mut task = Task::new(project.id.clone(), "T".to_string());
    task.internal_status = InternalStatus::Executing;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let run_id = create_running_agent_run(&app_state).await;
    let key = RunningAgentKey::new("task_execution", task.id.as_str());
    register_stale_entry(&app_state, &key, &run_id, None).await;
    let grace_secs = i64::try_from(
        crate::infrastructure::agents::claude::stream_timeouts().completion_grace_secs,
    )
    .unwrap();
    assert!(app_state
        .running_agent_registry
        .update_heartbeat(
            &key,
            &run_id.as_str(),
            chrono::Utc::now() - chrono::Duration::seconds(grace_secs + 1),
        )
        .await
        .unwrap());

    let engine = build_engine(&app_state, None);
    let info = app_state.running_agent_registry.get(&key).await.unwrap();

    let pruned = engine.evaluate_and_prune(&key, &info, false).await;

    assert!(pruned, "dead-PID entry should be pruned");
    assert!(
        !app_state.running_agent_registry.is_running(&key).await,
        "registry entry should be removed"
    );

    let run = app_state
        .agent_run_repo
        .get_by_id(&run_id)
        .await
        .unwrap()
        .expect("run should still exist");
    assert_eq!(
        run.status,
        AgentRunStatus::Cancelled,
        "running agent_run should be cancelled after prune"
    );
    assert_eq!(
        run.error_message.as_deref(),
        Some(PRUNED_STALE_AGENT_RUN),
        "dead-PID prune should remain attributable to the system pruner"
    );
}

#[tokio::test]
async fn evaluate_and_prune_non_running_run_status_prunes() {
    let app_state = AppState::new_test();

    let project = Project::new("P".to_string(), "/test".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    let mut task = Task::new(project.id.clone(), "T".to_string());
    task.internal_status = InternalStatus::Executing;
    app_state.task_repo.create(task.clone()).await.unwrap();

    // Create a run that is already Cancelled (not Running).
    let mut run = AgentRun::new(ChatConversationId::new());
    run.cancel();
    let run_id = run.id;
    app_state.agent_run_repo.create(run).await.unwrap();

    let key = RunningAgentKey::new("task_execution", task.id.as_str());
    register_stale_entry(&app_state, &key, &run_id, None).await;

    let engine = build_engine(&app_state, None);
    let entries = app_state.running_agent_registry.list_all().await;
    let (_, info) = entries.iter().find(|(k, _)| k == &key).unwrap();

    // pid dead + run Cancelled → reason: pid_missing, run_not_running → prune
    let pruned = engine.evaluate_and_prune(&key, info, false).await;
    assert!(pruned);
    assert!(!app_state.running_agent_registry.is_running(&key).await);
}

#[tokio::test]
async fn evaluate_and_prune_run_missing_prunes() {
    let app_state = AppState::new_test();

    // Use a fake run_id that points to a non-existent run in the repo.
    let fake_run_id = AgentRunId::from_string("00000000-0000-0000-0000-000000000042");
    let key = RunningAgentKey::new("task_execution", "task-no-run");
    app_state
        .running_agent_registry
        .register(
            key.clone(),
            999_999,
            "conv".to_string(),
            fake_run_id.as_str(),
            None,
            None,
        )
        .await;

    let engine = build_engine(&app_state, None);
    let entries = app_state.running_agent_registry.list_all().await;
    let (_, info) = entries.iter().find(|(k, _)| k == &key).unwrap();

    // run_missing + pid_missing → prune
    let pruned = engine.evaluate_and_prune(&key, info, false).await;
    assert!(pruned);
    assert!(!app_state.running_agent_registry.is_running(&key).await);
}

#[tokio::test]
async fn evaluate_and_prune_task_status_mismatch_prunes() {
    let app_state = AppState::new_test();

    let project = Project::new("P".to_string(), "/test".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Task is in terminal state (Merged), not Executing.
    let mut task = Task::new(project.id.clone(), "T".to_string());
    task.internal_status = InternalStatus::Merged;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let run_id = create_running_agent_run(&app_state).await;
    let key = RunningAgentKey::new("task_execution", task.id.as_str());
    // Use a non-existent PID; task_status_mismatch is also a reason alongside pid_missing.
    register_stale_entry(&app_state, &key, &run_id, None).await;

    let engine = build_engine(&app_state, None);
    let entries = app_state.running_agent_registry.list_all().await;
    let (_, info) = entries.iter().find(|(k, _)| k == &key).unwrap();

    // task in Merged ≠ Executing → task_status_mismatch (+ pid_missing) → prune
    let pruned = engine.evaluate_and_prune(&key, info, false).await;
    assert!(pruned, "task status mismatch should trigger prune");
    assert!(!app_state.running_agent_registry.is_running(&key).await);
}

#[tokio::test]
async fn evaluate_and_prune_live_task_status_mismatch_cancel_stays_unmarked() {
    let app_state = AppState::new_test();

    let project = Project::new("P".to_string(), "/test".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    let mut task = Task::new(project.id.clone(), "T".to_string());
    task.internal_status = InternalStatus::Merged;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let run_id = create_running_agent_run(&app_state).await;
    let key = RunningAgentKey::new("task_execution", task.id.as_str());
    register_stale_entry(&app_state, &key, &run_id, None).await;

    let engine = build_engine(&app_state, None);
    let info = app_state.running_agent_registry.get(&key).await.unwrap();
    assert!(engine.evaluate_and_prune(&key, &info, true).await);

    let run = app_state
        .agent_run_repo
        .get_by_id(&run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.status, AgentRunStatus::Cancelled);
    assert!(
        run.error_message.is_none(),
        "deliberate live-process stop must not become repairable"
    );
}

#[tokio::test]
async fn evaluate_and_prune_recent_merge_status_mismatch_waits_for_settlement() {
    let app_state = AppState::new_test();

    let project = Project::new("P".to_string(), "/test".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "T".to_string());
    task.internal_status = InternalStatus::Merged;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let run_id = create_running_agent_run(&app_state).await;
    let key = RunningAgentKey::new("merge", task.id.as_str());
    register_stale_entry(&app_state, &key, &run_id, None).await;
    let _ = app_state
        .running_agent_registry
        .update_heartbeat(&key, &run_id.as_str(), chrono::Utc::now())
        .await;

    let engine = build_engine(&app_state, None);
    let entries = app_state.running_agent_registry.list_all().await;
    let (_, info) = entries.iter().find(|(k, _)| k == &key).unwrap();

    let pruned = engine.evaluate_and_prune(&key, info, true).await;
    assert!(
        !pruned,
        "recent live merge status mismatch should wait for terminal-tool settlement"
    );
    assert!(app_state.running_agent_registry.is_running(&key).await);
}

#[tokio::test]
async fn evaluate_and_prune_stale_merge_status_mismatch_prunes_after_settlement_grace() {
    let app_state = AppState::new_test();

    let project = Project::new("P".to_string(), "/test".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "T".to_string());
    task.internal_status = InternalStatus::Merged;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let run_id = create_running_agent_run(&app_state).await;
    let key = RunningAgentKey::new("merge", task.id.as_str());
    register_stale_entry(&app_state, &key, &run_id, None).await;
    let grace_secs = i64::try_from(
        crate::infrastructure::agents::claude::stream_timeouts().completion_grace_secs,
    )
    .unwrap_or(i64::MAX - 1);
    let _ = app_state
        .running_agent_registry
        .update_heartbeat(
            &key,
            &run_id.as_str(),
            chrono::Utc::now() - chrono::Duration::seconds(grace_secs + 1),
        )
        .await;

    let engine = build_engine(&app_state, None);
    let entries = app_state.running_agent_registry.list_all().await;
    let (_, info) = entries.iter().find(|(k, _)| k == &key).unwrap();

    let pruned = engine.evaluate_and_prune(&key, info, true).await;
    assert!(
        pruned,
        "stale merge status mismatch should prune after settlement grace"
    );
    assert!(!app_state.running_agent_registry.is_running(&key).await);
}

#[tokio::test]
async fn evaluate_and_prune_task_missing_prunes() {
    let app_state = AppState::new_test();

    let run_id = create_running_agent_run(&app_state).await;
    // Key points to a task that does not exist in the repo.
    let key = RunningAgentKey::new("task_execution", "00000000-0000-0000-0000-000000000000");
    register_stale_entry(&app_state, &key, &run_id, None).await;

    let engine = build_engine(&app_state, None);
    let entries = app_state.running_agent_registry.list_all().await;
    let (_, info) = entries.iter().find(|(k, _)| k == &key).unwrap();

    // task_missing + pid_missing → prune
    let pruned = engine.evaluate_and_prune(&key, info, false).await;
    assert!(pruned);
    assert!(!app_state.running_agent_registry.is_running(&key).await);
}

#[tokio::test]
async fn evaluate_and_prune_already_completed_run_not_re_cancelled() {
    let app_state = AppState::new_test();

    let project = Project::new("P".to_string(), "/test".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    let mut task = Task::new(project.id.clone(), "T".to_string());
    task.internal_status = InternalStatus::Executing;
    app_state.task_repo.create(task.clone()).await.unwrap();

    // Run already Completed (not Running) — PruneEngine should not call cancel again.
    let mut run = AgentRun::new(ChatConversationId::new());
    run.complete();
    let run_id = run.id;
    app_state.agent_run_repo.create(run).await.unwrap();

    let key = RunningAgentKey::new("task_execution", task.id.as_str());
    register_stale_entry(&app_state, &key, &run_id, None).await;

    let engine = build_engine(&app_state, None);
    let entries = app_state.running_agent_registry.list_all().await;
    let (_, info) = entries.iter().find(|(k, _)| k == &key).unwrap();

    // Prune succeeds (dead PID + run_not_running) but run stays Completed (not re-cancelled).
    let pruned = engine.evaluate_and_prune(&key, info, false).await;
    assert!(pruned);

    let run_after = app_state
        .agent_run_repo
        .get_by_id(&run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        run_after.status,
        AgentRunStatus::Completed,
        "completed run should not be re-cancelled"
    );
}

// ─────────────────────────────────────────────
// Slot counter correction test
// ─────────────────────────────────────────────

#[tokio::test]
async fn slot_counter_corrected_after_prune_via_reconciler() {
    use crate::application::reconciliation::ReconciliationRunner;
    use crate::application::TaskTransitionService;

    let app_state = AppState::new_test();
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

    let reconciler: ReconciliationRunner = ReconciliationRunner::new(
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
        Arc::clone(&execution_state),
        None,
    );

    let project = Project::new("P".to_string(), "/test".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    let mut task = Task::new(project.id.clone(), "T".to_string());
    task.internal_status = InternalStatus::Merged; // terminal — triggers task_status_mismatch
    app_state.task_repo.create(task.clone()).await.unwrap();

    let run = AgentRun::new(ChatConversationId::new());
    let run_id = run.id;
    app_state.agent_run_repo.create(run).await.unwrap();

    // Manually bump the slot counter to simulate a running task.
    execution_state.increment_running();
    assert_eq!(execution_state.running_count(), 1);

    let key = RunningAgentKey::new("task_execution", task.id.as_str());
    register_stale_entry(&app_state, &key, &run_id, None).await;

    // Reconciler prunes the stale entry and recalculates the running count.
    reconciler.reconcile_stuck_tasks().await;

    assert!(
        !app_state.running_agent_registry.is_running(&key).await,
        "stale entry should be pruned"
    );
    assert_eq!(
        execution_state.running_count(),
        0,
        "running_count should be decremented to 0 after prune"
    );
}

// ─────────────────────────────────────────────
// Worktree cleanup test (Bug 5)
// ─────────────────────────────────────────────

#[tokio::test]
async fn evaluate_and_prune_merge_context_removes_worktree_dir() {
    let app_state = AppState::new_test();

    let worktree_parent = tempfile::TempDir::new().expect("failed to create worktree parent");
    let worktree_parent_path = worktree_parent
        .path()
        .canonicalize()
        .expect("failed to canonicalize worktree parent");
    let mut project = Project::new("P".to_string(), "/test".to_string());
    project.worktree_parent_directory = Some(worktree_parent_path.to_string_lossy().to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    let mut task = Task::new(project.id.clone(), "T".to_string());
    task.internal_status = InternalStatus::Merging;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let worktree_path_owned = validate_absolute_non_root_path(
        &project.task_worktree_path(task.id.as_str()),
        "prune engine merge worktree test",
    )
    .expect("merge worktree test path should be safe");
    require_under_root(
        &worktree_path_owned,
        &worktree_parent_path,
        "prune engine merge worktree test",
    )
    .expect("merge worktree test path should stay under its temp root");
    // codeql[rust/path-injection]
    std::fs::create_dir_all(&worktree_path_owned).expect("failed to create merge worktree");
    let worktree_path = worktree_path_owned.to_string_lossy().to_string();

    let run_id = create_running_agent_run(&app_state).await;
    let key = RunningAgentKey::new("merge", task.id.as_str());
    app_state
        .running_agent_registry
        .register(
            key.clone(),
            999_999, // dead PID
            "conv".to_string(),
            run_id.as_str(),
            Some(worktree_path.clone()),
            None,
        )
        .await;
    let grace_secs = i64::try_from(
        crate::infrastructure::agents::claude::stream_timeouts().completion_grace_secs,
    )
    .unwrap();
    assert!(app_state
        .running_agent_registry
        .update_heartbeat(
            &key,
            &run_id.as_str(),
            chrono::Utc::now() - chrono::Duration::seconds(grace_secs + 1),
        )
        .await
        .unwrap());

    let engine = build_engine(&app_state, None);
    let entries = app_state.running_agent_registry.list_all().await;
    let (_, info) = entries.iter().find(|(k, _)| k == &key).unwrap();

    // PruneEngine should prune AND remove the merge worktree directory.
    let pruned = engine.evaluate_and_prune(&key, info, false).await;
    assert!(pruned, "Merging entry should be pruned");

    // Directory should have been removed.
    assert!(
        !worktree_path_owned.exists(),
        "merge worktree directory should be removed after prune"
    );
    let run = app_state
        .agent_run_repo
        .get_by_id(&run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.status, AgentRunStatus::Cancelled);
    assert!(
        run.error_message.is_none(),
        "merge cleanup cancellation must not become repairable"
    );
}

// ─────────────────────────────────────────────
// Context-specific tests
// ─────────────────────────────────────────────

#[tokio::test]
async fn evaluate_and_prune_ideation_context_skips_task_lookup() {
    // Ideation entries use session IDs, not task IDs — task lookup must be skipped
    // to avoid routing session IDs through the task repository.
    let app_state = AppState::new_test();

    let run_id = create_running_agent_run(&app_state).await;
    let key = RunningAgentKey::new("ideation", "session-abc-123");
    app_state
        .running_agent_registry
        .register(
            key.clone(),
            std::process::id(), // alive PID
            "conv".to_string(),
            run_id.as_str(),
            None,
            None,
        )
        .await;

    let engine = build_engine(&app_state, None);
    let entries = app_state.running_agent_registry.list_all().await;
    let (_, info) = entries.iter().find(|(k, _)| k == &key).unwrap();

    // Alive PID + running run + ideation (no task lookup) → healthy, not pruned.
    let pruned = engine.evaluate_and_prune(&key, info, true).await;
    assert!(!pruned, "healthy ideation entry should not be pruned");
    assert!(app_state.running_agent_registry.is_running(&key).await);
}

#[tokio::test]
async fn evaluate_and_prune_review_context_healthy_not_pruned() {
    let app_state = AppState::new_test();

    let project = Project::new("P".to_string(), "/test".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    let mut task = Task::new(project.id.clone(), "T".to_string());
    task.internal_status = InternalStatus::Reviewing;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let run_id = create_running_agent_run(&app_state).await;
    let key = RunningAgentKey::new("review", task.id.as_str());
    app_state
        .running_agent_registry
        .register(
            key.clone(),
            std::process::id(),
            "conv".to_string(),
            run_id.as_str(),
            None,
            None,
        )
        .await;

    let engine = build_engine(&app_state, None);
    let entries = app_state.running_agent_registry.list_all().await;
    let (_, info) = entries.iter().find(|(k, _)| k == &key).unwrap();

    // Reviewing task + review context + alive PID + running run → healthy.
    let pruned = engine.evaluate_and_prune(&key, info, true).await;
    assert!(!pruned, "healthy review entry should not be pruned");
}

#[tokio::test]
async fn evaluate_and_prune_merge_context_healthy_no_worktree_cleanup() {
    // Healthy Merging context with no worktree path: should not be pruned (nothing to clean up).
    let app_state = AppState::new_test();

    let project = Project::new("P".to_string(), "/test".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    let mut task = Task::new(project.id.clone(), "T".to_string());
    task.internal_status = InternalStatus::Merging;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let run_id = create_running_agent_run(&app_state).await;
    let key = RunningAgentKey::new("merge", task.id.as_str());
    app_state
        .running_agent_registry
        .register(
            key.clone(),
            std::process::id(),
            "conv".to_string(),
            run_id.as_str(),
            None, // no worktree path
            None,
        )
        .await;

    let engine = build_engine(&app_state, None);
    let entries = app_state.running_agent_registry.list_all().await;
    let (_, info) = entries.iter().find(|(k, _)| k == &key).unwrap();

    // Merging task + alive PID + running run → healthy, no prune.
    let pruned = engine.evaluate_and_prune(&key, info, true).await;
    assert!(!pruned, "healthy merge entry should not be pruned");
}

#[tokio::test]
async fn coverage_regression_expired_running_reservation_is_pruned_and_cancelled() {
    let app_state = AppState::new_test();
    let run_id = create_running_agent_run(&app_state).await;
    let key = RunningAgentKey::new("project", "expired-running-launch");
    app_state
        .running_agent_registry
        .try_register(key.clone(), "conversation".into(), run_id.as_str())
        .await
        .unwrap();
    let lease = i64::try_from(
        crate::infrastructure::agents::claude::stream_timeouts().launch_reservation_lease_secs,
    )
    .unwrap();
    assert!(app_state
        .running_agent_registry
        .renew_reservation(
            &key,
            &run_id.as_str(),
            chrono::Utc::now() - chrono::Duration::seconds(lease + 1),
        )
        .await
        .unwrap());

    let info = app_state.running_agent_registry.get(&key).await.unwrap();
    assert!(
        build_engine(&app_state, None)
            .evaluate_and_prune(&key, &info, false)
            .await
    );
    assert!(!app_state.running_agent_registry.is_running(&key).await);
    assert_eq!(
        app_state
            .agent_run_repo
            .get_by_id(&run_id)
            .await
            .unwrap()
            .unwrap()
            .error_message
            .as_deref(),
        Some(PRUNED_STALE_AGENT_RUN)
    );
}

#[tokio::test]
async fn coverage_regression_terminal_run_invalidates_a_fresh_reservation() {
    let app_state = AppState::new_test();
    let mut run = AgentRun::new(ChatConversationId::new());
    run.complete();
    let run_id = run.id;
    app_state.agent_run_repo.create(run).await.unwrap();
    let key = RunningAgentKey::new("project", "terminal-launch");
    app_state
        .running_agent_registry
        .try_register(key.clone(), "conversation".into(), run_id.as_str())
        .await
        .unwrap();

    let info = app_state.running_agent_registry.get(&key).await.unwrap();
    assert!(
        build_engine(&app_state, None)
            .evaluate_and_prune(&key, &info, false)
            .await
    );
    assert!(!app_state.running_agent_registry.is_running(&key).await);
    assert_eq!(
        app_state
            .agent_run_repo
            .get_by_id(&run_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AgentRunStatus::Completed
    );
}

#[tokio::test]
async fn coverage_regression_live_non_merge_stale_owner_is_stopped_and_cancelled() {
    let app_state = AppState::new_test();
    let project = Project::new("P".to_string(), "/test".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    let mut task = Task::new(project.id, "T".to_string());
    task.internal_status = InternalStatus::Merged;
    app_state.task_repo.create(task.clone()).await.unwrap();
    let run_id = create_running_agent_run(&app_state).await;
    let key = RunningAgentKey::new("task_execution", task.id.as_str());
    register_stale_entry(&app_state, &key, &run_id, None).await;

    let info = app_state.running_agent_registry.get(&key).await.unwrap();
    assert!(
        build_engine(&app_state, None)
            .evaluate_and_prune(&key, &info, true)
            .await
    );
    assert!(!app_state.running_agent_registry.is_running(&key).await);
    assert_eq!(
        app_state
            .agent_run_repo
            .get_by_id(&run_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AgentRunStatus::Cancelled
    );
}

#[tokio::test]
async fn coverage_regression_merge_prune_never_removes_untrusted_worktree_paths() {
    let app_state = AppState::new_test();
    let worktree_parent = tempfile::TempDir::new().unwrap();
    let unrelated = tempfile::TempDir::new().unwrap();
    let missing_project_candidate = tempfile::TempDir::new().unwrap();
    let unsafe_expected_candidate = tempfile::TempDir::new().unwrap();
    let mut project = Project::new("P".to_string(), "/test".to_string());
    project.worktree_parent_directory = Some(
        worktree_parent
            .path()
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    let mut unsafe_expected_project = Project::new("Unsafe".to_string(), "/test".to_string());
    unsafe_expected_project.worktree_parent_directory = Some("relative-root".to_string());
    app_state
        .project_repo
        .create(unsafe_expected_project.clone())
        .await
        .unwrap();

    let candidates = [
        (project.id.clone(), "relative/worktree".to_string()),
        (
            project.id,
            unrelated
                .path()
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .to_string(),
        ),
        (
            crate::domain::entities::ProjectId::from_string("missing-project".to_string()),
            missing_project_candidate
                .path()
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .to_string(),
        ),
        (
            unsafe_expected_project.id,
            unsafe_expected_candidate
                .path()
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .to_string(),
        ),
    ];

    for (index, (project_id, candidate)) in candidates.into_iter().enumerate() {
        let mut task = Task::new(project_id, format!("T{index}"));
        task.internal_status = InternalStatus::Merged;
        app_state.task_repo.create(task.clone()).await.unwrap();
        let run_id = create_running_agent_run(&app_state).await;
        let key = RunningAgentKey::new("merge", task.id.as_str());
        register_stale_entry(&app_state, &key, &run_id, Some(candidate)).await;
        let info = app_state.running_agent_registry.get(&key).await.unwrap();

        assert!(
            build_engine(&app_state, None)
                .evaluate_and_prune(&key, &info, false)
                .await
        );
        assert!(!app_state.running_agent_registry.is_running(&key).await);
    }

    assert!(unrelated.path().exists());
    assert!(missing_project_candidate.path().exists());
    assert!(unsafe_expected_candidate.path().exists());
}
#[cfg(test)]
mod terminal_settlement_prune_tests {
    use super::*;
    use crate::domain::entities::ChatConversationId;

    fn running_info(last_active_at: Option<chrono::DateTime<chrono::Utc>>) -> RunningAgentInfo {
        RunningAgentInfo {
            pid: 12_345,
            conversation_id: "conversation-test".to_string(),
            agent_run_id: "run-test".to_string(),
            started_at: chrono::Utc::now(),
            worktree_path: None,
            cancellation_token: None,
            last_active_at,
            model: None,
        }
    }

    fn running_run() -> AgentRun {
        AgentRun::new(ChatConversationId::new())
    }

    #[test]
    fn terminal_settlement_prune_defers_recent_live_merge_status_mismatch() {
        let info = running_info(Some(chrono::Utc::now()));
        let run = running_run();

        assert!(should_defer_terminal_settlement_prune(
            Some(ChatContextType::Merge),
            &["task_status_mismatch"],
            &info,
            Some(&run),
            true,
        ));
    }

    #[test]
    fn terminal_settlement_prune_rejects_non_settlement_shapes() {
        let info = running_info(Some(chrono::Utc::now()));
        let missing_heartbeat = running_info(None);
        let run = running_run();

        assert!(!should_defer_terminal_settlement_prune(
            Some(ChatContextType::Merge),
            &["task_status_mismatch"],
            &info,
            Some(&run),
            false,
        ));
        assert!(!should_defer_terminal_settlement_prune(
            Some(ChatContextType::Merge),
            &["task_status_mismatch", "pid_missing"],
            &info,
            Some(&run),
            true,
        ));
        assert!(!should_defer_terminal_settlement_prune(
            Some(ChatContextType::TaskExecution),
            &["task_status_mismatch"],
            &info,
            Some(&run),
            true,
        ));
        assert!(!should_defer_terminal_settlement_prune(
            Some(ChatContextType::Review),
            &["task_status_mismatch"],
            &info,
            None,
            true,
        ));
        assert!(!should_defer_terminal_settlement_prune(
            Some(ChatContextType::Review),
            &["task_status_mismatch"],
            &missing_heartbeat,
            Some(&run),
            true,
        ));
    }

    #[test]
    fn pid_missing_prune_defers_only_the_fresh_running_shape() {
        let info = running_info(Some(chrono::Utc::now()));
        let run = running_run();

        assert!(should_defer_pid_missing_prune(
            &["pid_missing"],
            &info,
            Some(&run),
        ));
        assert!(!should_defer_pid_missing_prune(
            &["pid_missing", "task_status_mismatch"],
            &info,
            Some(&run),
        ));

        let mut cancelled = running_run();
        cancelled.cancel();
        assert!(!should_defer_pid_missing_prune(
            &["pid_missing"],
            &info,
            Some(&cancelled),
        ));
    }
}
