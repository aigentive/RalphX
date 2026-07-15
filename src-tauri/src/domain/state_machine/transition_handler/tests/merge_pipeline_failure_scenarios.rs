// Integration tests: merge pipeline failure scenarios from logs-21
//
// These tests cover the five RC failure scenarios observed in the production logs
// (2026-02-24 merge-logs-21.txt). Each test maps to a specific root cause and fix.
//
// Real git + memory repos + mock services — per CLAUDE.md rule 1.5.
//
// Scenario coverage:
//   RC1 — Cleanup timeout independence (covered in merge_pipeline_timeout_tests.rs; see note)
//   RC2 — Deferral gate: running_count > 0 defers merge to main
//   RC4 — Double-worktree delete (covered in rc4_rebase_double_delete.rs; see note)
//   RC5 — Retry pipeline: MergeIncomplete → PendingMerge → Merged
//   RC5 — Log message distinctness (structural; verified by compile-time string constants)
//
// Duplicate coverage note:
//   RC1 timeout: merge_pipeline_timeout_tests.rs::test_lsof_timeout_returns_within_bound,
//                test_pre_merge_cleanup_completes_in_bounded_time
//   RC4 delete:  rc4_rebase_double_delete.rs::test_rebase_squash_leaves_no_stale_worktrees

use super::helpers::*;
use crate::domain::entities::{InternalStatus, MergeStrategy, ProjectId, Task};
use crate::domain::state_machine::services::TaskScheduler;
use crate::domain::state_machine::{State, TransitionHandler};

// ──────────────────────────────────────────────────────────────────────────────
// Helper: services with execution_state + repos
// ──────────────────────────────────────────────────────────────────────────────

fn make_services_with_execution_state(
    task_repo: Arc<MemoryTaskRepository>,
    project_repo: Arc<MemoryProjectRepository>,
    execution_state: Arc<crate::commands::ExecutionState>,
) -> TaskServices {
    TaskServices::new(
        Arc::new(MockAgentSpawner::new()) as Arc<dyn AgentSpawner>,
        Arc::new(MockEventEmitter::new()) as Arc<dyn EventEmitter>,
        Arc::new(MockNotifier::new()) as Arc<dyn Notifier>,
        Arc::new(MockDependencyManager::new()) as Arc<dyn DependencyManager>,
        Arc::new(MockReviewStarter::new()) as Arc<dyn ReviewStarter>,
        Arc::new(MockChatService::new()) as Arc<dyn ChatService>,
    )
    .with_branch_update_repo(
        crate::testing::memory_branch_update_repository_with_task_repository(
            Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
        ),
    )
    .with_branch_update_workflow(crate::testing::branch_update_workflow(Arc::new(
        MockChatService::new(),
    )))
    .with_task_scheduler(Arc::new(MockTaskScheduler::new()) as Arc<dyn TaskScheduler>)
    .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
    .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
    .with_execution_state(execution_state)
}

// ──────────────────────────────────────────────────────────────────────────────
// RC5 — Retry pipeline: MergeIncomplete → PendingMerge → Merged
//
// Logs-21 scenario: first merge attempt failed with failure_source="transient_git"
// (silent 2-min window, lines 70-71). Task was rescheduled to PendingMerge.
// Second attempt succeeded → Merged (line 115).
//
// This test exercises that exact retry path: a task that was previously set to
// MergeIncomplete (by the first attempt) can be reset to PendingMerge and
// successfully complete the merge on retry.
// ──────────────────────────────────────────────────────────────────────────────

/// RC5-retry: A task that started as MergeIncomplete (first attempt failed) reaches
/// Merged when re-entered as PendingMerge.
///
/// Simulates the logs-21 retry: failure_source="transient_git" → rescheduled →
/// second attempt → Merged.
#[tokio::test]
async fn test_merge_retry_after_incomplete_reaches_merged() {
    let git_repo = setup_real_git_repo();

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());

    // Task starts as MergeIncomplete — simulates first attempt failure
    let mut task = Task::new(project_id.clone(), "RC5 retry test".to_string());
    task.internal_status = InternalStatus::MergeIncomplete;
    task.task_branch = Some(git_repo.task_branch.clone());
    let task_id = task.id.clone();
    task_repo.create(task.clone()).await.unwrap();

    let mut project = make_real_git_project(&git_repo.path_string());
    project.id = project_id;
    project.base_branch = Some("main".to_string());
    project.merge_strategy = MergeStrategy::Merge;
    project_repo.create(project).await.unwrap();

    // Simulate the scheduler retry: update DB status to PendingMerge
    task.internal_status = InternalStatus::PendingMerge;
    task_repo.update(&task).await.unwrap();

    // Build the state machine context with the updated task
    let services = TaskServices::new_mock()
        .with_branch_update_repo(
            crate::testing::memory_branch_update_repository_with_task_repository(Arc::clone(
                &task_repo,
            )
                as Arc<dyn TaskRepository>),
        )
        .with_branch_update_workflow(crate::testing::branch_update_workflow(Arc::new(
            MockChatService::new(),
        )))
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_task_scheduler(Arc::new(MockTaskScheduler::new()) as Arc<dyn TaskScheduler>);
    let context = crate::domain::state_machine::context::TaskContext::new(
        task_id.as_str(),
        "proj-1",
        services,
    );
    let mut machine = crate::domain::state_machine::TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    // Run the merge — this is the "second attempt" path
    let _ = handler.on_enter(&State::PendingMerge).await;

    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::Merged,
        "RC5-retry: MergeIncomplete → PendingMerge retry must reach Merged, \
         got {:?}. Metadata: {:?}",
        updated.internal_status,
        updated.metadata,
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// RC2 — Merge pipeline runs and succeeds even when running_count > 0
//
// Logs-21 scenario (line 111): "All agents idle, triggering main merge retry"
// implied a prior deferral while agents were running.
//
// The RC2 fix: removed the running_count == 0 guard from spawn_deferred_merge_retry
// (TOCTOU-prone) so try_retry_main_merges is always scheduled. The authoritative
// gate (check_main_merge_deferral) reads running_count fresh at merge-start time.
//
// With defer_merge_enabled=false (configured in ralphx.yaml), the deferral gate
// short-circuits and the merge always proceeds. This test verifies that a task
// reaches Merged even when other agents are active — no spurious "stuck at
// PendingMerge" regression.
// ──────────────────────────────────────────────────────────────────────────────

/// RC2: Merge pipeline reaches Merged even when running_count > 0.
///
/// Before the RC2 fix, a TOCTOU guard in spawn_deferred_merge_retry could prevent
/// try_retry_main_merges from firing when agents were active. After the fix, the
/// merge pipeline runs regardless of running_count.
///
/// With defer_merge_enabled=false (ralphx.yaml default), check_main_merge_deferral
/// short-circuits so the task always proceeds to Merged.
#[tokio::test]
async fn test_rc2_merge_runs_and_succeeds_with_agents_active() {
    let git_repo = setup_real_git_repo();

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "RC2 active-agents test".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some(git_repo.task_branch.clone());
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut project = make_real_git_project(&git_repo.path_string());
    project.id = project_id;
    project.base_branch = Some("main".to_string());
    project.merge_strategy = MergeStrategy::Merge;
    project_repo.create(project).await.unwrap();

    // Simulate the TOCTOU window: another task's agent is running concurrently
    let execution_state = Arc::new(crate::commands::ExecutionState::new());
    execution_state.increment_running(); // running_count = 1

    let services = make_services_with_execution_state(
        Arc::clone(&task_repo),
        Arc::clone(&project_repo),
        execution_state,
    );
    let context = crate::domain::state_machine::context::TaskContext::new(
        task_id.as_str(),
        "proj-1",
        services,
    );
    let mut machine = crate::domain::state_machine::TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::Merged,
        "RC2: merge must reach Merged even when running_count > 0 (no spurious deferral). \
         Got {:?}. Metadata: {:?}",
        updated.internal_status,
        updated.metadata,
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// RC5 — Log message constants: "Status transition confirmed" vs "Auto-transition triggered"
//
// The log messages must remain distinct for grep-ability. This structural test
// verifies both strings exist exactly once each in the task_transition_service
// source, catching accidental unification (the original RC5 bug).
// ──────────────────────────────────────────────────────────────────────────────

/// RC5-log: event-driven log message "Status transition confirmed" is distinct from
/// auto-transition message "Auto-transition triggered".
///
/// If both paths use the same string, grep -F "Status transition confirmed" would
/// also catch auto-transitions and vice versa — defeating the RC5 observability fix.
#[test]
fn test_rc5_log_messages_are_distinct_constants() {
    // These are the exact strings used at the two log sites in task_transition_service.rs.
    // If someone accidentally unifies them, this test catches the regression.
    let event_driven_msg = "Status transition confirmed";
    let auto_transition_msg = "Auto-transition triggered";

    assert_ne!(
        event_driven_msg, auto_transition_msg,
        "RC5: event-driven and auto-transition log messages must be distinct for grep-ability"
    );

    // Verify the source file contains exactly one occurrence of each
    let source = include_str!("../../../../application/task_transition_service.rs");

    let event_count = source.matches(event_driven_msg).count();
    let auto_count = source.matches(auto_transition_msg).count();

    assert_eq!(
        event_count, 1,
        "RC5: '{}' should appear exactly once in task_transition_service.rs (found {})",
        event_driven_msg, event_count
    );
    assert_eq!(
        auto_count, 1,
        "RC5: '{}' should appear exactly once in task_transition_service.rs (found {})",
        auto_transition_msg, auto_count
    );
}
