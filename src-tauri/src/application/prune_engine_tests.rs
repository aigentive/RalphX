use std::sync::Arc;

use crate::application::interactive_process_registry::{
    InteractiveProcessKey, InteractiveProcessRegistry,
};
use crate::application::{AppState, PruneEngine};
use crate::commands::execution_commands::ExecutionState;
use crate::domain::entities::{
    AgentRun, AgentRunId, AgentRunStatus, ChatConversationId, InternalStatus, Project, Task,
};
use crate::domain::services::RunningAgentKey;

// ─────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────

/// Build a PruneEngine wired to the given AppState's repos, with an optional IPR.
fn build_engine(
    app_state: &AppState,
    ipr: Option<Arc<InteractiveProcessRegistry>>,
) -> PruneEngine {
    PruneEngine::new(
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&app_state.agent_run_repo),
        Arc::clone(&app_state.task_repo),
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
async fn evaluate_and_prune_healthy_entry_not_pruned() {
    let app_state = AppState::new_test();

    let project = Project::new("P".to_string(), "/test".to_string());
    app_state.project_repo.create(project.clone()).await.unwrap();
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
async fn evaluate_and_prune_dead_pid_prunes_and_cancels_run() {
    let app_state = AppState::new_test();

    let project = Project::new("P".to_string(), "/test".to_string());
    app_state.project_repo.create(project.clone()).await.unwrap();
    let mut task = Task::new(project.id.clone(), "T".to_string());
    task.internal_status = InternalStatus::Executing;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let run_id = create_running_agent_run(&app_state).await;
    let key = RunningAgentKey::new("task_execution", task.id.as_str());
    register_stale_entry(&app_state, &key, &run_id, None).await;

    let engine = build_engine(&app_state, None);
    let entries = app_state.running_agent_registry.list_all().await;
    let (_, info) = entries.iter().find(|(k, _)| k == &key).unwrap();

    let pruned = engine.evaluate_and_prune(&key, info, false).await;

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
}

#[tokio::test]
async fn evaluate_and_prune_non_running_run_status_prunes() {
    let app_state = AppState::new_test();

    let project = Project::new("P".to_string(), "/test".to_string());
    app_state.project_repo.create(project.clone()).await.unwrap();
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
    app_state.project_repo.create(project.clone()).await.unwrap();

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
    app_state.project_repo.create(project.clone()).await.unwrap();
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

    let transition_service = Arc::new(TaskTransitionService::<tauri::Wry>::new(
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

    let reconciler: ReconciliationRunner<tauri::Wry> = ReconciliationRunner::new(
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
    app_state.project_repo.create(project.clone()).await.unwrap();
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

    let project = Project::new("P".to_string(), "/test".to_string());
    app_state.project_repo.create(project.clone()).await.unwrap();
    let mut task = Task::new(project.id.clone(), "T".to_string());
    task.internal_status = InternalStatus::Merging;
    app_state.task_repo.create(task.clone()).await.unwrap();

    // Create a real temp directory that represents the merge worktree.
    let tmp = tempfile::TempDir::new().expect("failed to create temp dir");
    let worktree_path = tmp.path().to_string_lossy().to_string();
    // Convert to PathBuf before PruneEngine removes it; keep the handle to avoid
    // double-remove panic if the drop runs after the dir is already gone.
    let worktree_path_owned = tmp.path().to_path_buf();
    // Leak the TempDir so its drop doesn't double-remove the (already-deleted) dir.
    std::mem::forget(tmp);

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
    app_state.project_repo.create(project.clone()).await.unwrap();
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
    app_state.project_repo.create(project.clone()).await.unwrap();
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
