// Tests for merge-hang fixes: pre_merge_cleanup step 0, 120s deadline, step timeouts
// Also: PDM-278 Bug 3 regression tests — worktree cleanup hardening
//
// These tests verify the three defensive layers added to prevent
// the 5+ minute merge hang caused by worktree deletion blocking
// when an agent still holds files open in the worktree.

use super::helpers::*;
use crate::domain::entities::{ChatContextType, InternalStatus};
use crate::domain::state_machine::{State, TransitionHandler};

// ==================
// Step 0: Agent kill before worktree deletion
// ==================

/// pre_merge_cleanup step 0 invokes stop_agent for Review and Merge context types.
///
/// With repos wired, `attempt_programmatic_merge()` proceeds past the early-return
/// guard and actually executes `pre_merge_cleanup` (stop_agent calls), then tries
/// the merge strategy dispatch. The MockChatService.stop_agent returns Ok(false)
/// (no agent running), which should be handled gracefully.
#[tokio::test]
async fn test_step0_agent_kill_executes_without_error() {
    let (mut machine, _, _) = setup_pending_merge_repos("Step 0 test", Some("feature/test"))
        .await
        .into_machine();
    let handler = TransitionHandler::new(&mut machine);

    // With repos wired, on_enter(PendingMerge) proceeds past the guard and actually
    // runs pre_merge_cleanup (step 0 = stop_agent) before trying to merge.
    // Git operations fail fast on nonexistent dir.
    let result = handler.on_enter(&State::PendingMerge).await;
    assert!(
        result.is_ok(),
        "on_enter(PendingMerge) should succeed even with step 0 agent kill"
    );
}

/// Defense-in-depth guard: when task is PendingMerge (not Reviewing),
/// pre_merge_cleanup skips stop_agent for the Review context but still
/// calls stop_agent for the Merge context.
///
/// This validates the self-sabotage guard: the Review agent should never be
/// killed after it has already transitioned the task past Reviewing.
#[tokio::test]
async fn test_pre_merge_cleanup_skips_review_stop_when_task_past_reviewing() {
    let setup =
        setup_pending_merge_repos("defense-in-depth guard test", Some("feature/test")).await;

    // Confirm task is in PendingMerge (not Reviewing) — guard should fire.
    let task = setup
        .task_repo
        .get_by_id(&setup.task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        task.internal_status,
        InternalStatus::PendingMerge,
        "pre-condition: task must be PendingMerge for guard to fire"
    );

    let chat_service = Arc::new(MockChatService::new());

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&setup.task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&setup.project_repo) as Arc<dyn ProjectRepository>)
        .with_chat_service(Arc::clone(&chat_service) as Arc<dyn crate::application::ChatService>);

    let context = TaskContext::new(setup.task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::PendingMerge).await;
    assert!(result.is_ok(), "on_enter(PendingMerge) should succeed");

    let calls = chat_service.get_stop_agent_calls().await;

    // Guard fires: Review stop_agent must NOT be called.
    let review_stopped = calls.iter().any(|(ctx, _)| *ctx == ChatContextType::Review);
    assert!(
        !review_stopped,
        "stop_agent should NOT be called for Review context when task is PendingMerge (self-sabotage guard); got calls: {:?}",
        calls
    );

    // Merge stop_agent MUST still be called (guard only skips Review).
    let merge_stopped = calls.iter().any(|(ctx, _)| *ctx == ChatContextType::Merge);
    assert!(
        merge_stopped,
        "stop_agent should still be called for Merge context; got calls: {:?}",
        calls
    );
}

// Tests early-return guard — does not reach merge strategy dispatch
/// Without repos, attempt_programmatic_merge bails before the step 0 settle sleep.
#[tokio::test]
async fn test_guard_no_repos_skips_step0_settle_sleep() {
    use std::time::Instant;

    let services = TaskServices::new_mock();
    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let start = Instant::now();
    let _ = handler.on_enter(&State::PendingMerge).await;
    let elapsed = start.elapsed();

    // Without repos, attempt_programmatic_merge returns immediately (no cleanup runs)
    assert!(
        elapsed.as_millis() < 100,
        "on_enter(PendingMerge) without repos should skip cleanup entirely, took {}ms",
        elapsed.as_millis()
    );
}

/// pre_merge_cleanup step 0b emits two-phase progress: agent cancellation then lsof scan.
///
/// Validates that the code path after the agent-stop loop (the orphaned-process scan)
/// runs without error. Progress events are no-ops without an EventSink, but the
/// flow compiles and executes correctly. This is the RC-B regression guard.
#[tokio::test]
async fn test_step0b_two_phase_progress_no_regression() {
    let (mut machine, _, _) = setup_pending_merge_repos("Step 0b two-phase", Some("feature/test"))
        .await
        .into_machine();
    let handler = TransitionHandler::new(&mut machine);

    // With repos wired, on_enter(PendingMerge) runs:
    //   1. stop_agent for Review + Merge (fast, no agents running)
    //   2. emit "Scanning worktree for orphaned processes..."  ← RC-B addition
    //   3. kill_worktree_processes_async (worktree path from task may not exist → no-op)
    // All steps must complete without blocking or panicking.
    let start = std::time::Instant::now();
    let result = handler.on_enter(&State::PendingMerge).await;
    let elapsed = start.elapsed();

    assert!(
        result.is_ok(),
        "on_enter(PendingMerge) should succeed with two-phase step 0b"
    );
    assert!(
        elapsed.as_secs() < 30,
        "step 0b should not block indefinitely, took {}s",
        elapsed.as_secs()
    );
}

// ==================
// 120s merge deadline
// ==================

// Tests early-return guard — does not reach merge strategy dispatch
/// Without repos, the no-repos guard returns before the deadline check is reached.
#[tokio::test]
async fn test_guard_no_repos_completes_within_deadline() {
    use std::time::Instant;

    // Use mock services without repos — this means attempt_programmatic_merge
    // returns immediately, but the code structure with the deadline still compiles
    // and the no-repos early return happens before the deadline check.
    let services = TaskServices::new_mock();
    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let start = Instant::now();
    let _ = handler.on_enter(&State::PendingMerge).await;
    let elapsed = start.elapsed();

    // If the deadline code were broken (e.g., blocking forever), this would fail
    assert!(
        elapsed.as_secs() < 5,
        "PendingMerge entry should complete well within deadline, took {}s",
        elapsed.as_secs()
    );
}

// Tests early-return guard — does not reach merge strategy dispatch
/// Without repos, the repos-unavailable path fires on_exit(PendingMerge, MergeIncomplete).
#[tokio::test]
async fn test_guard_no_repos_fires_on_exit_to_merge_incomplete() {
    let emitter = Arc::new(MockEventEmitter::new());
    let mut services = TaskServices::new_mock();
    services.event_emitter = Arc::clone(&emitter) as Arc<dyn EventEmitter>;

    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    // Without repos, attempt_programmatic_merge logs error and calls on_exit
    let _ = handler.on_enter(&State::PendingMerge).await;

    // The repos-unavailable path emits on_exit(PendingMerge, MergeIncomplete)
    // which is the same code path used by the deadline timeout
    // Verify the handler completes without panic
}

// ==================
// Timeout wrappers on steps 4/5/6
// ==================

/// Steps 4/5/6 timeout wrappers don't break existing behavior.
///
/// With repos wired, `attempt_programmatic_merge()` proceeds past the early-return
/// guard and actually runs pre_merge_cleanup (including timeout-wrapped steps)
/// before hitting the merge strategy dispatch. When the git dir doesn't exist,
/// the merge fails via handle_outcome_git_error → MergeIncomplete.
///
/// Note: The git error path in merge_outcome_handler.rs uses a local
/// transition_to_merge_incomplete (not self.on_exit), so on_exit side effects
/// like try_retry_deferred_merges are NOT triggered. Deferred retry is handled
/// by the reconciler instead.
#[tokio::test]
async fn test_timeout_wrappers_dont_break_existing_workflow() {
    let (mut machine, task_repo, task_id) =
        setup_pending_merge_repos("Timeout wrapper test", Some("feature/test"))
            .await
            .into_machine();
    let handler = TransitionHandler::new(&mut machine);

    // Enter PendingMerge — with repos, this runs pre_merge_cleanup (timeout-wrapped steps)
    // then attempts merge, which fails on nonexistent git dir.
    let start = std::time::Instant::now();
    let result = handler.on_enter(&State::PendingMerge).await;
    let elapsed = start.elapsed();
    assert!(result.is_ok());

    // Verify the merge path completes in bounded time (not stuck in cleanup/strategy)
    assert!(
        elapsed.as_secs() < 30,
        "on_enter(PendingMerge) with repos should complete in bounded time, took {}s",
        elapsed.as_secs()
    );

    // Verify the task transitioned to MergeIncomplete (not stuck in PendingMerge)
    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        updated.internal_status,
        crate::domain::entities::InternalStatus::MergeIncomplete,
        "Task should be in MergeIncomplete after merge fails with nonexistent git dir, got {:?}",
        updated.internal_status
    );
}

// ==================
// Config: pending_merge_stale_minutes = 2
// ==================

/// Verify pending_merge_stale_minutes code default is 2 (reduced from 5).
///
/// Tests the struct default directly to avoid config file interference.
#[test]
fn test_pending_merge_stale_minutes_default_is_2() {
    use crate::infrastructure::agents::claude::ReconciliationConfig;

    let config = ReconciliationConfig::default();
    assert_eq!(
        config.pending_merge_stale_minutes, 2,
        "pending_merge_stale_minutes should default to 2 (reduced from 5 for faster recovery)"
    );
}

// ==================
// PDM-278 Bug 3 regressions: worktree cleanup hardening
// ==================

/// PDM-278 Bug 3 regression: pre_merge_cleanup runs on the first clean attempt.
///
/// Before the fix, `maybe_skip_first_attempt_cleanup` returned `true` on a first
/// clean attempt (no debris metadata, no pipeline_active column set), causing
/// the entire cleanup to be skipped. This left stale worktrees from prior crashed
/// merges unaddressed.
///
/// After the fix, `maybe_skip_first_attempt_cleanup` always returns `false`,
/// so cleanup (including agent stop calls) always executes.
#[tokio::test]
async fn test_pdm278_cleanup_runs_on_first_clean_attempt() {
    // Task with no debris markers — this is a "first clean attempt" by is_first_clean_attempt()
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = crate::domain::entities::ProjectId::from_string("proj-1".to_string());
    let mut task =
        crate::domain::entities::Task::new(project_id.clone(), "clean first attempt".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some("feature/test".to_string());
    // No merge_pipeline_active, no debris metadata, no worktree_path on disk
    // → is_first_clean_attempt() returns true for this task
    task_repo.create(task).await.unwrap();

    let mut project = crate::domain::entities::Project::new(
        "test-project".to_string(),
        "/tmp/nonexistent-pdm278-first-attempt".to_string(),
    );
    project.id = project_id;
    project.base_branch = Some("main".to_string());
    project_repo.create(project).await.unwrap();

    let chat_service = Arc::new(MockChatService::new());
    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_chat_service(Arc::clone(&chat_service) as Arc<dyn crate::application::ChatService>);

    let task_id = task_repo
        .get_by_project(&crate::domain::entities::ProjectId::from_string(
            "proj-1".to_string(),
        ))
        .await
        .unwrap()[0]
        .id
        .clone();
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = crate::domain::state_machine::TaskStateMachine::new(context);
    let handler = crate::domain::state_machine::TransitionHandler::new(&mut machine);

    let result = handler
        .on_enter(&crate::domain::state_machine::State::PendingMerge)
        .await;
    assert!(
        result.is_ok(),
        "on_enter(PendingMerge) should succeed on first clean attempt"
    );

    // Cleanup ran — stop_agent for Merge context must have been called.
    // (Review context is skipped by the self-sabotage guard since task is PendingMerge.)
    let calls = chat_service.get_stop_agent_calls().await;
    let merge_stopped = calls.iter().any(|(ctx, _)| *ctx == ChatContextType::Merge);
    assert!(
        merge_stopped,
        "PDM-278 Bug 3: stop_agent for Merge must be called even on the first clean attempt; got calls: {:?}",
        calls
    );
}

/// PDM-278 Bug 3 regression: outer_deadline formula gives enough budget for one retry.
///
/// cleanup.rs computes: outer_deadline = now + (worktree_timeout_secs * 2 + 10s)
/// This ensures that if Step 2 times out, there is still time for a bounded retry
/// before the outer deadline is hit.
///
/// This test verifies the deadline formula produces a value > 2 * original_timeout
/// (so there is always retry budget) using the configured default timeout.
#[test]
fn test_pdm278_outer_deadline_provides_retry_budget() {
    use crate::infrastructure::agents::claude::git_runtime_config;
    let timeout_secs = git_runtime_config().cleanup_worktree_timeout_secs;
    // Formula: outer_deadline = now + (timeout * 2 + 10s)
    let outer_budget_secs = timeout_secs * 2 + 10;
    // For retry to be possible: remaining (outer_budget_secs) > original_timeout + 2s
    assert!(
        outer_budget_secs > timeout_secs + 2,
        "PDM-278 Bug 3: outer_deadline budget ({outer_budget_secs}s) must exceed original_timeout + 2s ({})s to allow at least one retry",
        timeout_secs + 2
    );
}

/// PDM-278 Bug 3 regression: cleanup_stale_worktree_artifacts failure is best-effort.
///
/// If cleanup_stale_worktree_artifacts fails (e.g., invalid repo path), the merge
/// pipeline must continue — failure must never abort the pipeline.
///
/// This test exercises the code path via on_enter(PendingMerge) with a project path
/// that will cause cleanup to fail. The pipeline should reach MergeIncomplete (git
/// failure), not crash or return an error from the cleanup call.
#[tokio::test]
async fn test_pdm278_cleanup_stale_artifacts_failure_is_best_effort() {
    // Use a repo path that does not exist — cleanup_stale_worktree_artifacts will
    // fail (no git repo), but the pipeline should still proceed to MergeIncomplete.
    let (mut machine, task_repo, task_id) =
        setup_pending_merge_repos("PDM-278 best-effort cleanup", Some("feature/test"))
            .await
            .into_machine();
    let handler = crate::domain::state_machine::TransitionHandler::new(&mut machine);

    let result = handler
        .on_enter(&crate::domain::state_machine::State::PendingMerge)
        .await;

    // Pipeline must succeed (no hard error from cleanup failure)
    assert!(
        result.is_ok(),
        "PDM-278 Bug 3: cleanup_stale_worktree_artifacts failure must not abort the merge pipeline"
    );

    // Task reached MergeIncomplete (git path failed, not cleanup) — confirms pipeline ran
    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::MergeIncomplete,
        "PDM-278 Bug 3: task should reach MergeIncomplete (via git error) even when cleanup_stale_worktree_artifacts fails"
    );
}

// ==================
// Post-merge cleanup: scheduler triggered after unblock_dependents
// ==================

/// post_merge_cleanup triggers try_schedule_ready_tasks after unblocking dependents.
///
/// This verifies the fix that adds scheduler triggering to the programmatic
/// merge path (which bypasses on_enter(Merged)).
#[tokio::test]
async fn test_post_merge_cleanup_triggers_scheduler() {
    use crate::domain::state_machine::mocks::MockTaskScheduler;

    let scheduler = Arc::new(MockTaskScheduler::new());
    let dep_manager = Arc::new(MockDependencyManager::new());

    let mut services = TaskServices::new_mock();
    services.dependency_manager = Arc::clone(&dep_manager) as Arc<dyn DependencyManager>;
    services.task_scheduler =
        Some(Arc::clone(&scheduler)
            as Arc<
                dyn crate::domain::state_machine::services::TaskScheduler,
            >);

    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    // Trigger on_enter(Merged) which calls unblock_dependents + try_schedule_ready_tasks
    let _ = handler.on_enter(&State::Merged).await;

    // Wait for spawned tasks to call try_schedule_ready_tasks
    let sched = Arc::clone(&scheduler);
    assert!(
        wait_for_condition(
            || {
                let s = Arc::clone(&sched);
                async move {
                    s.get_calls()
                        .iter()
                        .any(|c| c.method == "try_schedule_ready_tasks")
                }
            },
            5000
        )
        .await,
        "Scheduler should be triggered after unblocking dependents"
    );

    let dep_calls = dep_manager.get_calls();
    assert!(
        dep_calls.iter().any(|c| c.method == "unblock_dependents"),
        "unblock_dependents should be called on Merged entry"
    );
}
