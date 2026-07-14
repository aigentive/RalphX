use async_trait::async_trait;
use chrono::{Duration, Utc};
use ralphx_lib::application::{AppState, ReadyWatchdog, TaskSchedulerService};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{
    ArtifactId, ExecutionPlan, ExecutionPlanId, GitMode, IdeationSession, IdeationSessionId,
    InternalStatus, PlanBranch, PlanBranchStatus, Project, ProjectId, Task,
};
use ralphx_lib::domain::execution::ExecutionSettings;
use ralphx_lib::domain::services::RunningAgentKey;
use ralphx_lib::domain::state_machine::services::TaskScheduler;
use ralphx_lib::domain::state_machine::transition_handler::DEFERRED_MERGE_TIMEOUT_SECONDS;
use ralphx_lib::infrastructure::agents::claude::scheduler_config;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tempfile::TempDir;

/// Helper to create test state
async fn setup_test_state() -> (Arc<ExecutionState>, AppState) {
    let execution_state = Arc::new(ExecutionState::new());
    let app_state = AppState::new_test();
    (execution_state, app_state)
}

/// Helper to build a TaskSchedulerService from test state
fn build_scheduler(
    app_state: &AppState,
    execution_state: &Arc<ExecutionState>,
) -> TaskSchedulerService {
    TaskSchedulerService::new(
        Arc::clone(execution_state),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.artifact_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.agent_run_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&app_state.memory_event_repo),
        None,
    )
    .with_execution_settings_repo(Arc::clone(&app_state.execution_settings_repo))
    .with_execution_plan_repo(Arc::clone(&app_state.execution_plan_repo))
}

#[tokio::test]
async fn test_no_schedule_when_paused() {
    let (execution_state, app_state) = setup_test_state().await;

    // Create a project with a Ready task
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Ready Task".to_string());
    task.internal_status = InternalStatus::Ready;
    app_state.task_repo.create(task.clone()).await.unwrap();

    // Pause execution
    execution_state.pause();

    let scheduler = build_scheduler(&app_state, &execution_state);

    // Should not schedule (paused)
    scheduler.try_schedule_ready_tasks().await;

    // Task should still be Ready
    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.internal_status, InternalStatus::Ready);
}

#[tokio::test]
async fn test_no_schedule_when_at_capacity() {
    let (execution_state, app_state) = setup_test_state().await;

    // Set global concurrent to 1 and fill the slot
    execution_state.set_global_max_concurrent(1);
    execution_state.increment_running();

    // Create a project with a Ready task
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Ready Task".to_string());
    task.internal_status = InternalStatus::Ready;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let scheduler = build_scheduler(&app_state, &execution_state);

    // Should not schedule (at capacity)
    scheduler.try_schedule_ready_tasks().await;

    // Task should still be Ready
    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.internal_status, InternalStatus::Ready);
}

#[tokio::test]
async fn test_no_schedule_when_no_ready_tasks() {
    let (execution_state, app_state) = setup_test_state().await;

    // Set high max concurrent
    execution_state.set_max_concurrent(10);

    let scheduler = build_scheduler(&app_state, &execution_state);

    // Should complete without panic (no tasks to schedule)
    scheduler.try_schedule_ready_tasks().await;

    // Running count should still be 0
    assert_eq!(execution_state.running_count(), 0);
}

#[tokio::test]
async fn test_schedules_oldest_ready_task() {
    let (execution_state, app_state) = setup_test_state().await;

    // Set high max concurrent
    execution_state.set_max_concurrent(10);

    // Create a project
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create older task first
    let mut older_task = Task::new(project.id.clone(), "Older Task".to_string());
    older_task.internal_status = InternalStatus::Ready;
    app_state
        .task_repo
        .create(older_task.clone())
        .await
        .unwrap();
    let older_task_id = older_task.id.clone();

    // Small delay to ensure different created_at timestamps
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Create newer task
    let mut newer_task = Task::new(project.id.clone(), "Newer Task".to_string());
    newer_task.internal_status = InternalStatus::Ready;
    app_state
        .task_repo
        .create(newer_task.clone())
        .await
        .unwrap();
    let newer_task_id = newer_task.id.clone();

    let scheduler = build_scheduler(&app_state, &execution_state);

    // Schedule - should pick the older task
    scheduler.try_schedule_ready_tasks().await;

    // Older task should be Executing (transitioned)
    let updated_older = app_state
        .task_repo
        .get_by_id(&older_task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_older.internal_status,
        InternalStatus::Failed,
        "Older task should be Failed after ExecutionBlocked"
    );

    // Newer task should also be Failed (Local mode doesn't block if no Executing tasks)
    let updated_newer = app_state
        .task_repo
        .get_by_id(&newer_task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_newer.internal_status,
        InternalStatus::Failed,
        "Newer task should also be Failed after ExecutionBlocked (older task failed, not executing)"
    );
}

#[tokio::test]
async fn test_schedules_across_projects() {
    let (execution_state, app_state) = setup_test_state().await;

    // Set high max concurrent
    execution_state.set_max_concurrent(10);

    // Create two projects
    let project1 = Project::new("Project 1".to_string(), "/test/path1".to_string());
    app_state
        .project_repo
        .create(project1.clone())
        .await
        .unwrap();

    let project2 = Project::new("Project 2".to_string(), "/test/path2".to_string());
    app_state
        .project_repo
        .create(project2.clone())
        .await
        .unwrap();

    // Create older task in project 2
    let mut older_task = Task::new(project2.id.clone(), "Older Task (P2)".to_string());
    older_task.internal_status = InternalStatus::Ready;
    app_state
        .task_repo
        .create(older_task.clone())
        .await
        .unwrap();
    let older_task_id = older_task.id.clone();

    // Small delay
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Create newer task in project 1
    let mut newer_task = Task::new(project1.id.clone(), "Newer Task (P1)".to_string());
    newer_task.internal_status = InternalStatus::Ready;
    app_state
        .task_repo
        .create(newer_task.clone())
        .await
        .unwrap();

    let scheduler = build_scheduler(&app_state, &execution_state);

    // Schedule - should pick the older task from project 2
    scheduler.try_schedule_ready_tasks().await;

    // Older task should be Executing
    let updated_older = app_state
        .task_repo
        .get_by_id(&older_task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_older.internal_status,
        InternalStatus::Failed,
        "Older task from Project 2 should be Failed after ExecutionBlocked"
    );
}

#[tokio::test]
async fn test_find_oldest_schedulable_task() {
    let (execution_state, app_state) = setup_test_state().await;

    // Create a project (default is Worktree mode)
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create tasks with different statuses
    let mut ready_task = Task::new(project.id.clone(), "Ready Task".to_string());
    ready_task.internal_status = InternalStatus::Ready;
    app_state
        .task_repo
        .create(ready_task.clone())
        .await
        .unwrap();

    let mut backlog_task = Task::new(project.id.clone(), "Backlog Task".to_string());
    backlog_task.internal_status = InternalStatus::Backlog;
    app_state.task_repo.create(backlog_task).await.unwrap();

    let scheduler = build_scheduler(&app_state, &execution_state);

    // Should find only the Ready task
    let found = scheduler.find_oldest_schedulable_task_for_test().await;
    assert!(found.is_some());
    assert_eq!(found.unwrap().id, ready_task.id);
}

#[tokio::test]
async fn test_trait_object_safety() {
    let (execution_state, app_state) = setup_test_state().await;
    let scheduler = build_scheduler(&app_state, &execution_state);

    // Should be usable as trait object
    let scheduler_trait: Arc<dyn TaskScheduler> = Arc::new(scheduler);
    scheduler_trait.try_schedule_ready_tasks().await;
}

#[tokio::test]
async fn test_worktree_mode_allows_parallel_tasks() {
    let (execution_state, app_state) = setup_test_state().await;
    execution_state.set_max_concurrent(10);

    // Create a Worktree-mode project
    let mut project = Project::new("Worktree Project".to_string(), "/test/wt".to_string());
    project.git_mode = GitMode::Worktree;
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create an Executing task
    let mut executing_task = Task::new(project.id.clone(), "Executing Task".to_string());
    executing_task.internal_status = InternalStatus::Executing;
    app_state.task_repo.create(executing_task).await.unwrap();

    // Create a Ready task
    let mut ready_task = Task::new(project.id.clone(), "Ready Task".to_string());
    ready_task.internal_status = InternalStatus::Ready;
    app_state
        .task_repo
        .create(ready_task.clone())
        .await
        .unwrap();

    let scheduler = build_scheduler(&app_state, &execution_state);

    // Should find the Ready task (Worktree mode allows parallel)
    let found = scheduler.find_oldest_schedulable_task_for_test().await;
    assert!(
        found.is_some(),
        "Worktree mode should allow parallel task execution"
    );
    assert_eq!(found.unwrap().id, ready_task.id);
}

#[tokio::test]
async fn test_scheduler_skips_project_at_capacity_and_picks_other_project() {
    let (execution_state, app_state) = setup_test_state().await;
    execution_state.set_global_max_concurrent(10);

    let mut project_a = Project::new("Project A".to_string(), "/test/a".to_string());
    project_a.git_mode = GitMode::Worktree;
    app_state
        .project_repo
        .create(project_a.clone())
        .await
        .unwrap();

    let project_b = Project::new("Project B".to_string(), "/test/b".to_string());
    app_state
        .project_repo
        .create(project_b.clone())
        .await
        .unwrap();

    app_state
        .execution_settings_repo
        .update_settings(
            Some(&project_a.id),
            &ExecutionSettings {
                max_concurrent_tasks: 1,
                project_ideation_max: 1,
                auto_commit: true,
                pause_on_failure: true,
                ..ExecutionSettings::default()
            },
        )
        .await
        .unwrap();

    let mut occupied_task = Task::new(project_a.id.clone(), "Occupied Task".to_string());
    occupied_task.internal_status = InternalStatus::Executing;
    let occupied_task = app_state.task_repo.create(occupied_task).await.unwrap();
    app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("task_execution", occupied_task.id.as_str()),
            45454,
            "occupied-conv".to_string(),
            "occupied-run".to_string(),
            None,
            None,
        )
        .await;

    let mut blocked_ready = Task::new(project_a.id.clone(), "Blocked Ready".to_string());
    blocked_ready.internal_status = InternalStatus::Ready;
    let blocked_ready = app_state.task_repo.create(blocked_ready).await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;

    let mut runnable_ready = Task::new(project_b.id.clone(), "Runnable Ready".to_string());
    runnable_ready.internal_status = InternalStatus::Ready;
    let runnable_ready = app_state.task_repo.create(runnable_ready).await.unwrap();

    let scheduler = build_scheduler(&app_state, &execution_state);
    let found = scheduler
        .find_oldest_schedulable_task_for_test()
        .await
        .expect("scheduler should find a task in the unconstrained project");

    assert_eq!(
        found.id, runnable_ready.id,
        "scheduler must skip the older task in the saturated project"
    );
    assert_ne!(found.id, blocked_ready.id);
}

#[tokio::test]
async fn test_scheduler_skips_ready_task_from_superseded_execution_plan() {
    let (execution_state, app_state) = setup_test_state().await;
    execution_state.set_global_max_concurrent(10);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let session = app_state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .unwrap();

    let mut old_plan = ExecutionPlan::new(session.id.clone());
    old_plan.status = ralphx_lib::domain::entities::ExecutionPlanStatus::Superseded;
    let old_plan = app_state
        .execution_plan_repo
        .create(old_plan)
        .await
        .unwrap();

    let active_plan = app_state
        .execution_plan_repo
        .create(ExecutionPlan::new(session.id.clone()))
        .await
        .unwrap();

    let mut stale_ready = Task::new(project.id.clone(), "Superseded Plan Task".to_string());
    stale_ready.internal_status = InternalStatus::Ready;
    stale_ready.ideation_session_id = Some(session.id.clone());
    stale_ready.execution_plan_id = Some(old_plan.id.clone());
    let stale_ready = app_state.task_repo.create(stale_ready).await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;

    let mut current_ready = Task::new(project.id.clone(), "Active Plan Task".to_string());
    current_ready.internal_status = InternalStatus::Ready;
    current_ready.ideation_session_id = Some(session.id.clone());
    current_ready.execution_plan_id = Some(active_plan.id.clone());
    let current_ready = app_state.task_repo.create(current_ready).await.unwrap();

    let scheduler = build_scheduler(&app_state, &execution_state);
    let found = scheduler
        .find_oldest_schedulable_task_for_test()
        .await
        .expect("scheduler should choose the active-plan task");

    assert_eq!(
        found.id, current_ready.id,
        "ready tasks from superseded execution plans must be ignored even when they are older"
    );
    assert_ne!(found.id, stale_ready.id);
}

// ═══════════════════════════════════════════════════════════════════════
// Multi-Task Scheduling Tests (Phase 77)
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_schedules_multiple_tasks_up_to_capacity() {
    let (execution_state, app_state) = setup_test_state().await;

    // Set max concurrent to 3
    execution_state.set_max_concurrent(3);

    // Create a Worktree-mode project (allows parallel tasks from same project)
    let mut project = Project::new("Test Project".to_string(), "/test/path".to_string());
    project.git_mode = GitMode::Worktree;
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create 5 Ready tasks
    let mut task_ids = Vec::new();
    for i in 0..5 {
        let mut task = Task::new(project.id.clone(), format!("Task {}", i));
        task.internal_status = InternalStatus::Ready;
        app_state.task_repo.create(task.clone()).await.unwrap();
        task_ids.push(task.id);
        // Small delay to ensure different timestamps
        tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
    }

    let scheduler = build_scheduler(&app_state, &execution_state);

    // Schedule - should pick up to 3 tasks (max_concurrent)
    scheduler.try_schedule_ready_tasks().await;

    // Count tasks in each state
    let mut executing_count = 0;
    let mut ready_count = 0;

    for task_id in &task_ids {
        let task = app_state
            .task_repo
            .get_by_id(task_id)
            .await
            .unwrap()
            .unwrap();
        match task.internal_status {
            InternalStatus::Failed => executing_count += 1,
            InternalStatus::Ready => ready_count += 1,
            _ => panic!("Unexpected status: {:?}", task.internal_status),
        }
    }

    assert_eq!(
        executing_count, 5,
        "All tasks Failed after ExecutionBlocked (capacity check requires Executing state)"
    );
    assert_eq!(
        ready_count, 0,
        "No tasks remain Ready (all attempted scheduling)"
    );
}

#[tokio::test]
async fn test_loop_stops_at_capacity() {
    let (execution_state, app_state) = setup_test_state().await;

    // Set max concurrent to 2, pre-fill 1 running slot
    execution_state.set_max_concurrent(2);
    execution_state.increment_running(); // 1 slot already taken

    // Create multiple Worktree-mode projects with one Ready task each
    // This allows testing capacity limits without Local-mode single-task constraint
    let mut task_ids = Vec::new();
    for i in 0..3 {
        let mut project = Project::new(format!("Project {}", i), format!("/test/path{}", i));
        project.git_mode = GitMode::Worktree;
        app_state
            .project_repo
            .create(project.clone())
            .await
            .unwrap();

        let mut task = Task::new(project.id.clone(), format!("Task {}", i));
        task.internal_status = InternalStatus::Ready;
        app_state.task_repo.create(task.clone()).await.unwrap();
        task_ids.push(task.id);
        tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
    }

    let scheduler = build_scheduler(&app_state, &execution_state);

    // Schedule - should only pick 1 task (only 1 slot available: max=2, pre-filled=1)
    scheduler.try_schedule_ready_tasks().await;

    // Count tasks in each state
    let mut executing_count = 0;
    let mut ready_count = 0;

    for task_id in &task_ids {
        let task = app_state
            .task_repo
            .get_by_id(task_id)
            .await
            .unwrap()
            .unwrap();
        match task.internal_status {
            InternalStatus::Failed => executing_count += 1,
            InternalStatus::Ready => ready_count += 1,
            _ => panic!("Unexpected status: {:?}", task.internal_status),
        }
    }

    assert_eq!(
        executing_count, 3,
        "All tasks Failed after ExecutionBlocked (capacity check requires Executing state)"
    );
    assert_eq!(
        ready_count, 0,
        "No tasks remain Ready (all attempted scheduling)"
    );

    // Running count stays at pre-filled value (tasks failed, not executing)
    assert_eq!(
        execution_state.running_count(),
        1,
        "Running count unchanged (tasks failed during transition)"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Deferred Merge Retry Tests
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_retry_deferred_merges_skips_non_pending_merge_tasks() {
    let (execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create a task in Executing state with merge_deferred metadata (shouldn't happen
    // in practice, but tests the status filter)
    let mut task = Task::new(project.id.clone(), "Executing Task".to_string());
    task.internal_status = InternalStatus::Executing;
    task.metadata = Some(r#"{"merge_deferred": true}"#.to_string());
    app_state.task_repo.create(task.clone()).await.unwrap();

    let scheduler = build_scheduler(&app_state, &execution_state);
    scheduler
        .try_retry_deferred_merges(project.id.as_str())
        .await;

    // Task should still have merge_deferred metadata (not touched)
    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.internal_status, InternalStatus::Executing);
    assert!(updated
        .metadata
        .as_deref()
        .unwrap()
        .contains("merge_deferred"));
}

#[tokio::test]
async fn test_retry_deferred_merges_skips_pending_merge_without_flag() {
    let (execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create a PendingMerge task without merge_deferred metadata
    let mut task = Task::new(project.id.clone(), "Pending Merge Task".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let scheduler = build_scheduler(&app_state, &execution_state);
    scheduler
        .try_retry_deferred_merges(project.id.as_str())
        .await;

    // Task should still be PendingMerge with no metadata changes
    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.internal_status, InternalStatus::PendingMerge);
    assert!(updated.metadata.is_none());
}

#[tokio::test]
async fn test_retry_deferred_merges_clears_flag_on_deferred_task() {
    let (execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create a PendingMerge task WITH merge_deferred metadata
    let mut task = Task::new(project.id.clone(), "Deferred Merge".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.metadata = Some(
        r#"{"merge_deferred": true, "merge_deferred_at": "2026-01-01T00:00:00Z"}"#.to_string(),
    );
    app_state.task_repo.create(task.clone()).await.unwrap();

    let scheduler = build_scheduler(&app_state, &execution_state);
    scheduler
        .try_retry_deferred_merges(project.id.as_str())
        .await;

    // The merge_deferred flag should be cleared
    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    // Metadata should be None (only deferred fields existed)
    assert!(
        updated.metadata.is_none()
            || !updated
                .metadata
                .as_deref()
                .unwrap_or("")
                .contains("merge_deferred"),
        "merge_deferred flag should be cleared"
    );
    let metadata = updated
        .metadata
        .as_deref()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .unwrap_or_default();
    assert_eq!(metadata["trigger_origin"], "recovery");
}

#[tokio::test]
async fn test_retry_deferred_merges_only_retries_one_at_a_time() {
    let (execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create two PendingMerge tasks with merge_deferred metadata
    let mut task1 = Task::new(project.id.clone(), "Deferred Merge 1".to_string());
    task1.internal_status = InternalStatus::PendingMerge;
    task1.metadata = Some(r#"{"merge_deferred": true}"#.to_string());
    app_state.task_repo.create(task1.clone()).await.unwrap();

    let mut task2 = Task::new(project.id.clone(), "Deferred Merge 2".to_string());
    task2.internal_status = InternalStatus::PendingMerge;
    task2.metadata = Some(r#"{"merge_deferred": true}"#.to_string());
    app_state.task_repo.create(task2.clone()).await.unwrap();

    let scheduler = build_scheduler(&app_state, &execution_state);
    scheduler
        .try_retry_deferred_merges(project.id.as_str())
        .await;

    // Only one task should have its flag cleared (serialization)
    let updated1 = app_state
        .task_repo
        .get_by_id(&task1.id)
        .await
        .unwrap()
        .unwrap();
    let updated2 = app_state
        .task_repo
        .get_by_id(&task2.id)
        .await
        .unwrap()
        .unwrap();

    let flag1_cleared = updated1.metadata.is_none()
        || !updated1
            .metadata
            .as_deref()
            .unwrap_or("")
            .contains("merge_deferred");
    let flag2_cleared = updated2.metadata.is_none()
        || !updated2
            .metadata
            .as_deref()
            .unwrap_or("")
            .contains("merge_deferred");

    assert!(
        flag1_cleared ^ flag2_cleared,
        "Exactly one task should have its flag cleared (serialization). \
         task1 cleared={}, task2 cleared={}",
        flag1_cleared,
        flag2_cleared
    );
}

#[tokio::test]
async fn test_retry_deferred_merges_noop_for_wrong_project() {
    let (execution_state, app_state) = setup_test_state().await;

    let project1 = Project::new("Project 1".to_string(), "/test/path1".to_string());
    app_state
        .project_repo
        .create(project1.clone())
        .await
        .unwrap();

    let project2 = Project::new("Project 2".to_string(), "/test/path2".to_string());
    app_state
        .project_repo
        .create(project2.clone())
        .await
        .unwrap();

    // Create a deferred merge task in project 1
    let mut task = Task::new(project1.id.clone(), "Deferred Merge".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.metadata = Some(r#"{"merge_deferred": true}"#.to_string());
    app_state.task_repo.create(task.clone()).await.unwrap();

    let scheduler = build_scheduler(&app_state, &execution_state);

    // Retry for project 2 — should not touch project 1's task
    scheduler
        .try_retry_deferred_merges(project2.id.as_str())
        .await;

    // Task should still have the deferred flag
    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        updated
            .metadata
            .as_deref()
            .unwrap()
            .contains("merge_deferred"),
        "Task in project 1 should not be touched when retrying for project 2"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Active Project Scoping Tests (Phase 82)
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_scheduler_only_schedules_active_project_tasks() {
    let (execution_state, app_state) = setup_test_state().await;
    execution_state.set_max_concurrent(10);

    // Create two projects
    let project1 = Project::new("Project 1".to_string(), "/test/path1".to_string());
    app_state
        .project_repo
        .create(project1.clone())
        .await
        .unwrap();

    let project2 = Project::new("Project 2".to_string(), "/test/path2".to_string());
    app_state
        .project_repo
        .create(project2.clone())
        .await
        .unwrap();

    // Create older Ready task in project 1
    let mut p1_task = Task::new(project1.id.clone(), "Project 1 Task".to_string());
    p1_task.internal_status = InternalStatus::Ready;
    app_state.task_repo.create(p1_task.clone()).await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Create newer Ready task in project 2 (chronologically newer but should be ignored)
    let mut p2_task = Task::new(project2.id.clone(), "Project 2 Task".to_string());
    p2_task.internal_status = InternalStatus::Ready;
    app_state.task_repo.create(p2_task.clone()).await.unwrap();

    let scheduler = build_scheduler(&app_state, &execution_state);

    // Set active project to project 2 only
    scheduler
        .set_active_project(Some(project2.id.clone()))
        .await;

    // Schedule - should only pick task from project 2 (active project)
    scheduler.try_schedule_ready_tasks().await;

    // Project 1 task should still be Ready (not scheduled, not active project)
    let updated_p1 = app_state
        .task_repo
        .get_by_id(&p1_task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_p1.internal_status,
        InternalStatus::Ready,
        "Project 1 task should NOT be scheduled (not active project)"
    );

    // Project 2 task should be Executing (scheduled from active project)
    let updated_p2 = app_state
        .task_repo
        .get_by_id(&p2_task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_p2.internal_status,
        InternalStatus::Failed,
        "Project 2 task should be Failed after ExecutionBlocked (active project)"
    );
}

#[tokio::test]
async fn test_scheduler_schedules_all_projects_when_no_active_project() {
    let (execution_state, app_state) = setup_test_state().await;
    execution_state.set_max_concurrent(10);

    // Create two projects
    let project1 = Project::new("Project 1".to_string(), "/test/path1".to_string());
    app_state
        .project_repo
        .create(project1.clone())
        .await
        .unwrap();

    let project2 = Project::new("Project 2".to_string(), "/test/path2".to_string());
    app_state
        .project_repo
        .create(project2.clone())
        .await
        .unwrap();

    // Create older Ready task in project 2
    let mut p2_task = Task::new(project2.id.clone(), "Project 2 Task".to_string());
    p2_task.internal_status = InternalStatus::Ready;
    app_state.task_repo.create(p2_task.clone()).await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Create newer Ready task in project 1
    let mut p1_task = Task::new(project1.id.clone(), "Project 1 Task".to_string());
    p1_task.internal_status = InternalStatus::Ready;
    app_state.task_repo.create(p1_task.clone()).await.unwrap();

    let scheduler = build_scheduler(&app_state, &execution_state);

    // No active project set (default is None)
    assert_eq!(scheduler.get_active_project().await, None);

    // Schedule - should schedule tasks across all projects
    scheduler.try_schedule_ready_tasks().await;

    // Both tasks should be Executing (no active project filter, both ready)
    let updated_p2 = app_state
        .task_repo
        .get_by_id(&p2_task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_p2.internal_status,
        InternalStatus::Failed,
        "Project 2 task should be Failed after ExecutionBlocked when no active project"
    );

    let updated_p1 = app_state
        .task_repo
        .get_by_id(&p1_task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_p1.internal_status,
        InternalStatus::Failed,
        "Project 1 task should also be Failed after ExecutionBlocked when no active project (max_concurrent=10)"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Main Merge Retry Tests (Global Idle)
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_retry_main_merges_skips_non_pending_merge_tasks() {
    let (execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create a task in Executing state with main_merge_deferred metadata (shouldn't happen
    // in practice, but tests the status filter)
    let mut task = Task::new(project.id.clone(), "Executing Task".to_string());
    task.internal_status = InternalStatus::Executing;
    task.metadata = Some(r#"{"main_merge_deferred": true}"#.to_string());
    app_state.task_repo.create(task.clone()).await.unwrap();

    let scheduler = build_scheduler(&app_state, &execution_state);
    scheduler.try_retry_main_merges().await;

    // Task should still have main_merge_deferred metadata (not touched)
    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.internal_status, InternalStatus::Executing);
    assert!(updated
        .metadata
        .as_deref()
        .unwrap()
        .contains("main_merge_deferred"));
}

#[tokio::test]
async fn test_retry_main_merges_skips_pending_merge_without_flag() {
    let (execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create a PendingMerge task without main_merge_deferred metadata
    let mut task = Task::new(project.id.clone(), "Pending Merge Task".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let scheduler = build_scheduler(&app_state, &execution_state);
    scheduler.try_retry_main_merges().await;

    // Task should still be PendingMerge with no metadata changes
    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.internal_status, InternalStatus::PendingMerge);
    assert!(updated.metadata.is_none());
}

#[tokio::test]
async fn test_retry_main_merges_clears_flag_on_deferred_task() {
    let (execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create a PendingMerge task WITH main_merge_deferred metadata
    let mut task = Task::new(project.id.clone(), "Deferred Main Merge".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.metadata = Some(
        r#"{"main_merge_deferred": true, "main_merge_deferred_at": "2026-02-15T00:00:00Z"}"#
            .to_string(),
    );
    app_state.task_repo.create(task.clone()).await.unwrap();

    let scheduler = build_scheduler(&app_state, &execution_state);
    scheduler.try_retry_main_merges().await;

    // The main_merge_deferred flag should be cleared
    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    // Metadata should not contain main_merge_deferred
    assert!(
        updated.metadata.is_none()
            || !updated
                .metadata
                .as_deref()
                .unwrap_or("")
                .contains("main_merge_deferred"),
        "main_merge_deferred flag should be cleared"
    );
    let metadata = updated
        .metadata
        .as_deref()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .unwrap_or_default();
    assert_eq!(metadata["trigger_origin"], "recovery");
}

#[tokio::test]
async fn test_retry_main_merges_only_retries_one_at_a_time() {
    let (execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create two PendingMerge tasks with main_merge_deferred metadata
    let mut task1 = Task::new(project.id.clone(), "Deferred Main Merge 1".to_string());
    task1.internal_status = InternalStatus::PendingMerge;
    task1.metadata = Some(r#"{"main_merge_deferred": true}"#.to_string());
    app_state.task_repo.create(task1.clone()).await.unwrap();

    let mut task2 = Task::new(project.id.clone(), "Deferred Main Merge 2".to_string());
    task2.internal_status = InternalStatus::PendingMerge;
    task2.metadata = Some(r#"{"main_merge_deferred": true}"#.to_string());
    app_state.task_repo.create(task2.clone()).await.unwrap();

    let scheduler = build_scheduler(&app_state, &execution_state);
    scheduler.try_retry_main_merges().await;

    // Only one task should have its flag cleared (serialization)
    let updated1 = app_state
        .task_repo
        .get_by_id(&task1.id)
        .await
        .unwrap()
        .unwrap();
    let updated2 = app_state
        .task_repo
        .get_by_id(&task2.id)
        .await
        .unwrap()
        .unwrap();

    let flag1_cleared = updated1.metadata.is_none()
        || !updated1
            .metadata
            .as_deref()
            .unwrap_or("")
            .contains("main_merge_deferred");
    let flag2_cleared = updated2.metadata.is_none()
        || !updated2
            .metadata
            .as_deref()
            .unwrap_or("")
            .contains("main_merge_deferred");

    assert!(
        flag1_cleared ^ flag2_cleared,
        "Exactly one task should have its flag cleared (serialization). \
         task1 cleared={}, task2 cleared={}",
        flag1_cleared,
        flag2_cleared
    );
}

#[tokio::test]
async fn test_retry_main_merges_finds_tasks_across_all_projects() {
    let (execution_state, app_state) = setup_test_state().await;

    let project1 = Project::new("Project 1".to_string(), "/test/path1".to_string());
    app_state
        .project_repo
        .create(project1.clone())
        .await
        .unwrap();

    let project2 = Project::new("Project 2".to_string(), "/test/path2".to_string());
    app_state
        .project_repo
        .create(project2.clone())
        .await
        .unwrap();

    // Create a main-merge-deferred task in each project
    let mut task1 = Task::new(project1.id.clone(), "Deferred Main Merge P1".to_string());
    task1.internal_status = InternalStatus::PendingMerge;
    task1.metadata = Some(r#"{"main_merge_deferred": true}"#.to_string());
    app_state.task_repo.create(task1.clone()).await.unwrap();

    let mut task2 = Task::new(project2.id.clone(), "Deferred Main Merge P2".to_string());
    task2.internal_status = InternalStatus::PendingMerge;
    task2.metadata = Some(r#"{"main_merge_deferred": true}"#.to_string());
    app_state.task_repo.create(task2.clone()).await.unwrap();

    let scheduler = build_scheduler(&app_state, &execution_state);
    scheduler.try_retry_main_merges().await;

    // At least one task from any project should have its flag cleared
    let updated1 = app_state
        .task_repo
        .get_by_id(&task1.id)
        .await
        .unwrap()
        .unwrap();
    let updated2 = app_state
        .task_repo
        .get_by_id(&task2.id)
        .await
        .unwrap()
        .unwrap();

    let flag1_cleared = updated1.metadata.is_none()
        || !updated1
            .metadata
            .as_deref()
            .unwrap_or("")
            .contains("main_merge_deferred");
    let flag2_cleared = updated2.metadata.is_none()
        || !updated2
            .metadata
            .as_deref()
            .unwrap_or("")
            .contains("main_merge_deferred");

    // At least one should be cleared (method scans all projects)
    assert!(
        flag1_cleared || flag2_cleared,
        "At least one task across all projects should have its flag cleared"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Lock Contention Retry Tests (S6 fix)
// ═══════════════════════════════════════════════════════════════════════

/// When scheduling_lock is held, a retry should be queued instead of silently dropping.
#[tokio::test]
async fn test_contention_queues_retry_when_self_ref_set() {
    let (execution_state, app_state) = setup_test_state().await;
    execution_state.set_max_concurrent(10);

    let scheduler = Arc::new(build_scheduler(&app_state, &execution_state));
    scheduler.set_self_ref(Arc::clone(&scheduler) as Arc<dyn TaskScheduler>);

    // Verify no pending retries initially
    assert_eq!(
        scheduler.contention_retry_pending_for_test(),
        0,
        "No pending retries at start"
    );

    // Hold the scheduling_lock to simulate contention
    let _guard = scheduler.lock_scheduling_for_test().await;

    // Call try_schedule_ready_tasks while lock is held — should queue a retry
    // We can't await it directly because it would block. Spawn it.
    let scheduler2 = Arc::clone(&scheduler);
    let handle = tokio::spawn(async move {
        // This call should encounter contention and queue a retry
        scheduler2.try_schedule_ready_tasks().await;
    });
    handle.await.unwrap();

    // A retry should now be pending (spawned but sleeping for 200ms)
    assert_eq!(
        scheduler.contention_retry_pending_for_test(),
        1,
        "One retry should be pending after contention"
    );

    // Release the lock so the retry can succeed
    drop(_guard);

    // Wait for the retry to fire (200ms delay + buffer)
    tokio::time::sleep(tokio::time::Duration::from_millis(350)).await;

    // After retry completes, counter should be back to 0
    assert_eq!(
        scheduler.contention_retry_pending_for_test(),
        0,
        "Retry counter should return to 0 after retry fires"
    );
}

/// When scheduling_lock is held and self_ref is NOT set, the call is silently dropped
/// (unchanged from original behaviour — no retry can be queued without a self reference).
#[tokio::test]
async fn test_contention_drops_silently_without_self_ref() {
    let (execution_state, app_state) = setup_test_state().await;
    execution_state.set_max_concurrent(10);

    let scheduler = Arc::new(build_scheduler(&app_state, &execution_state));
    // Deliberately do NOT call set_self_ref

    let _guard = scheduler.lock_scheduling_for_test().await;

    let scheduler2 = Arc::clone(&scheduler);
    tokio::spawn(async move {
        scheduler2.try_schedule_ready_tasks().await;
    })
    .await
    .unwrap();

    // No retry queued because self_ref is None
    assert_eq!(
        scheduler.contention_retry_pending_for_test(),
        0,
        "No retry queued when self_ref is not set"
    );
}

/// When retry limit is reached, further contention attempts are dropped without
/// queuing additional retries.
#[tokio::test]
async fn test_contention_respects_max_retry_limit() {
    let (execution_state, app_state) = setup_test_state().await;
    execution_state.set_max_concurrent(10);

    let scheduler = Arc::new(build_scheduler(&app_state, &execution_state));
    scheduler.set_self_ref(Arc::clone(&scheduler) as Arc<dyn TaskScheduler>);

    // Pre-fill the retry counter to the maximum
    let max_retries = scheduler_config().max_contention_retries as u32;
    scheduler.set_contention_retry_pending_for_test(max_retries);

    let _guard = scheduler.lock_scheduling_for_test().await;

    let scheduler2 = Arc::clone(&scheduler);
    tokio::spawn(async move {
        // Should be dropped: retry limit already at max
        scheduler2.try_schedule_ready_tasks().await;
    })
    .await
    .unwrap();

    // Counter must stay at MAX (not incremented further)
    assert_eq!(
        scheduler.contention_retry_pending_for_test(),
        max_retries,
        "Counter must not exceed max_contention_retries"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Ready Watchdog Tests
// ═══════════════════════════════════════════════════════════════════════

/// Helper to create a ReadyWatchdog with a zero-second staleness threshold
/// (all Ready tasks are immediately stale) for testing.
fn build_watchdog(app_state: &AppState, execution_state: &Arc<ExecutionState>) -> ReadyWatchdog {
    let scheduler = Arc::new(build_scheduler(app_state, execution_state));
    ReadyWatchdog::new(
        scheduler,
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.project_repo),
    )
    .with_stale_threshold_secs(0)
}

struct CountingScheduler {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl TaskScheduler for CountingScheduler {
    async fn try_schedule_ready_tasks(&self) {
        self.calls.fetch_add(1, Ordering::SeqCst);
    }

    async fn try_retry_deferred_merges(&self, _project_id: &str) {}

    async fn try_retry_main_merges(&self) {}
}

#[tokio::test]
async fn test_watchdog_returns_zero_when_no_ready_tasks() {
    let (execution_state, app_state) = setup_test_state().await;
    execution_state.set_max_concurrent(10);

    let watchdog = build_watchdog(&app_state, &execution_state);

    let count = watchdog.run_once().await;
    assert_eq!(count, 0, "No stale tasks when no Ready tasks exist");
}

#[tokio::test]
async fn test_watchdog_detects_stale_ready_task() {
    let (execution_state, app_state) = setup_test_state().await;
    execution_state.set_max_concurrent(10);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create a Ready task (threshold=0 so it's immediately stale)
    let mut task = Task::new(project.id.clone(), "Stale Ready Task".to_string());
    task.internal_status = InternalStatus::Ready;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let watchdog = build_watchdog(&app_state, &execution_state);

    // Watchdog should find the stale task
    let count = watchdog.run_once().await;
    assert_eq!(count, 1, "Should detect 1 stale Ready task");
}

#[tokio::test]
async fn test_watchdog_does_not_detect_non_ready_tasks() {
    let (execution_state, app_state) = setup_test_state().await;
    execution_state.set_max_concurrent(10);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create tasks in non-Ready states
    for status in &[
        InternalStatus::Backlog,
        InternalStatus::Executing,
        InternalStatus::Blocked,
    ] {
        let mut task = Task::new(project.id.clone(), format!("{:?} Task", status));
        task.internal_status = *status;
        app_state.task_repo.create(task).await.unwrap();
    }

    let watchdog = build_watchdog(&app_state, &execution_state);

    // No Ready tasks → watchdog should find 0 stale tasks
    let count = watchdog.run_once().await;
    assert_eq!(count, 0, "Only Ready tasks should be detected as stale");
}

#[tokio::test]
async fn test_watchdog_triggers_scheduling_for_stale_tasks() {
    let (execution_state, app_state) = setup_test_state().await;
    execution_state.set_max_concurrent(10);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create a Ready task
    let mut task = Task::new(project.id.clone(), "Stale Ready Task".to_string());
    task.internal_status = InternalStatus::Ready;
    app_state.task_repo.create(task.clone()).await.unwrap();

    // Watchdog with threshold=0 so the task is immediately stale
    let watchdog = build_watchdog(&app_state, &execution_state);
    watchdog.run_once().await;

    // The task should have been transitioned out of Ready (Failed due to no agent in test)
    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(
        updated.internal_status,
        InternalStatus::Ready,
        "Stale task should be transitioned after watchdog reschedule"
    );
}

#[tokio::test]
async fn test_watchdog_with_high_threshold_skips_fresh_tasks() {
    let (execution_state, app_state) = setup_test_state().await;
    execution_state.set_max_concurrent(10);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create a Ready task (just created → not stale under a large threshold)
    let mut task = Task::new(project.id.clone(), "Fresh Ready Task".to_string());
    task.internal_status = InternalStatus::Ready;
    app_state.task_repo.create(task.clone()).await.unwrap();

    // Watchdog with a 3600-second threshold (task is too fresh to be stale)
    let scheduler = Arc::new(build_scheduler(&app_state, &execution_state));
    let watchdog = ReadyWatchdog::new(
        scheduler,
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.project_repo),
    )
    .with_stale_threshold_secs(3600);

    let count = watchdog.run_once().await;
    assert_eq!(count, 0, "Fresh task should not be detected as stale");

    // Task should still be Ready
    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::Ready,
        "Fresh task should remain Ready with high staleness threshold"
    );
}

#[tokio::test]
async fn test_watchdog_configurable_threshold() {
    let (execution_state, app_state) = setup_test_state().await;

    let watchdog = ReadyWatchdog::new(
        Arc::new(build_scheduler(&app_state, &execution_state)),
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.project_repo),
    )
    .with_stale_threshold_secs(120)
    .with_interval_secs(30);

    assert_eq!(watchdog.stale_threshold_secs_for_test(), 120);
    assert_eq!(watchdog.interval_secs_for_test(), 30);
}

#[tokio::test]
async fn test_scheduler_retries_pending_review_after_freshness_backoff_expiry() {
    let (execution_state, app_state) = setup_test_state().await;
    execution_state.set_max_concurrent(10);

    let temp_dir = TempDir::new().unwrap();
    let project = Project::new(
        "Test Project".to_string(),
        temp_dir.path().display().to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Retry parked review".to_string());
    task.internal_status = InternalStatus::PendingReview;
    task.worktree_path = Some(temp_dir.path().display().to_string());
    task.metadata = Some(
        serde_json::json!({
            "freshness_origin_state": "reviewing",
            "freshness_conflict_count": 1,
            "freshness_backoff_until": (Utc::now() - Duration::seconds(5)).to_rfc3339(),
            "branch_freshness_conflict": true,
            "source_update_conflict": true
        })
        .to_string(),
    );
    app_state.task_repo.create(task.clone()).await.unwrap();

    let scheduler = build_scheduler(&app_state, &execution_state);
    scheduler.try_schedule_ready_tasks().await;

    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(
        updated.internal_status,
        InternalStatus::PendingReview,
        "Expired review-origin freshness backoff should re-arm the parked task"
    );
}

#[tokio::test]
async fn test_watchdog_detects_retryable_pending_review_after_backoff_expiry() {
    let (_execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Retryable PendingReview".to_string());
    task.internal_status = InternalStatus::PendingReview;
    task.metadata = Some(
        serde_json::json!({
            "freshness_origin_state": "reviewing",
            "freshness_conflict_count": 1,
            "freshness_backoff_until": (Utc::now() - Duration::seconds(5)).to_rfc3339(),
            "branch_freshness_conflict": true,
            "source_update_conflict": true
        })
        .to_string(),
    );
    app_state.task_repo.create(task).await.unwrap();

    let calls = Arc::new(AtomicUsize::new(0));
    let scheduler = Arc::new(CountingScheduler {
        calls: Arc::clone(&calls),
    });
    let watchdog = ReadyWatchdog::new(
        scheduler,
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.project_repo),
    )
    .with_stale_threshold_secs(3600);

    let count = watchdog.run_once().await;
    assert_eq!(
        count, 1,
        "Expired review backoff should be retried even with a high Ready-task threshold"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "Watchdog should trigger scheduling when a parked review becomes retryable"
    );
}

#[tokio::test]
async fn test_watchdog_ignores_pending_review_before_freshness_backoff_expires() {
    let (_execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Cooling down PendingReview".to_string());
    task.internal_status = InternalStatus::PendingReview;
    task.metadata = Some(
        serde_json::json!({
            "freshness_origin_state": "reviewing",
            "freshness_conflict_count": 1,
            "freshness_backoff_until": (Utc::now() + Duration::seconds(120)).to_rfc3339(),
            "branch_freshness_conflict": true,
            "source_update_conflict": true
        })
        .to_string(),
    );
    app_state.task_repo.create(task).await.unwrap();

    let calls = Arc::new(AtomicUsize::new(0));
    let scheduler = Arc::new(CountingScheduler {
        calls: Arc::clone(&calls),
    });
    let watchdog = ReadyWatchdog::new(
        scheduler,
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.project_repo),
    )
    .with_stale_threshold_secs(0);

    let count = watchdog.run_once().await;
    assert_eq!(
        count, 0,
        "PendingReview task should stay parked until freshness backoff expires"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "Watchdog must not schedule review retries before backoff expiry"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Deferred Merge Timeout Tests
// ═══════════════════════════════════════════════════════════════════════

/// Helper: create a task with merge_deferred flag and a timestamp in the past
fn make_deferred_task_with_age(project_id: &ProjectId, title: &str, seconds_ago: i64) -> Task {
    let deferred_at = (chrono::Utc::now() - chrono::Duration::seconds(seconds_ago)).to_rfc3339();
    let mut task = Task::new(project_id.clone(), title.to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.metadata = Some(
        serde_json::json!({
            "merge_deferred": true,
            "merge_deferred_at": deferred_at,
            "target_branch": "feature/some-feature"
        })
        .to_string(),
    );
    task
}

/// Helper: create a task with main_merge_deferred flag and a timestamp in the past
fn make_main_deferred_task_with_age(project_id: &ProjectId, title: &str, seconds_ago: i64) -> Task {
    let deferred_at = (chrono::Utc::now() - chrono::Duration::seconds(seconds_ago)).to_rfc3339();
    let mut task = Task::new(project_id.clone(), title.to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.metadata = Some(
        serde_json::json!({
            "main_merge_deferred": true,
            "main_merge_deferred_at": deferred_at,
            "target_branch": "main"
        })
        .to_string(),
    );
    task
}

#[tokio::test]
async fn test_retry_deferred_merges_proceeds_when_within_timeout() {
    let (execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create a deferred task with age well within the timeout (10 seconds old)
    let task = make_deferred_task_with_age(&project.id, "Recent Deferred Merge", 10);
    app_state.task_repo.create(task.clone()).await.unwrap();

    let scheduler = build_scheduler(&app_state, &execution_state);
    scheduler
        .try_retry_deferred_merges(project.id.as_str())
        .await;

    // Task should have had its deferred flag cleared (retry was triggered)
    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    let flag_cleared = updated
        .metadata
        .as_deref()
        .map(|m| !m.contains("\"merge_deferred\":true"))
        .unwrap_or(true);
    assert!(
        flag_cleared,
        "Deferred merge within timeout should still have retry triggered (flag cleared)"
    );
    let _ = DEFERRED_MERGE_TIMEOUT_SECONDS; // silence unused warning
}

#[tokio::test]
async fn test_retry_deferred_merges_logs_warning_when_timeout_exceeded() {
    let (execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create a deferred task older than the timeout
    let seconds_ago = DEFERRED_MERGE_TIMEOUT_SECONDS + 60; // well past timeout
    let task = make_deferred_task_with_age(&project.id, "Timed Out Deferred Merge", seconds_ago);
    app_state.task_repo.create(task.clone()).await.unwrap();

    let scheduler = build_scheduler(&app_state, &execution_state);
    // Should not panic, should log warning and proceed with retry
    scheduler
        .try_retry_deferred_merges(project.id.as_str())
        .await;

    // Task should have retry triggered (flag cleared or metadata updated)
    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    let flag_cleared = updated
        .metadata
        .as_deref()
        .map(|m| !m.contains("\"merge_deferred\":true"))
        .unwrap_or(true);
    assert!(
        flag_cleared,
        "Timed-out deferred merge should have retry triggered (flag cleared)"
    );
}

#[tokio::test]
async fn test_retry_main_merges_bypasses_sibling_guard_when_timeout_exceeded() {
    let (execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create an ideation session
    let session = IdeationSession::new(project.id.clone());
    app_state
        .ideation_session_repo
        .create(session.clone())
        .await
        .unwrap();

    // Create a main-merge-deferred task older than the timeout, linked to the session
    let seconds_ago = DEFERRED_MERGE_TIMEOUT_SECONDS + 60;
    let mut task =
        make_main_deferred_task_with_age(&project.id, "Timed Out Main Merge", seconds_ago);
    task.ideation_session_id = Some(session.id.clone());
    app_state.task_repo.create(task.clone()).await.unwrap();

    // Create a sibling task that is NOT terminal (would normally block the retry)
    let mut sibling = Task::new(project.id.clone(), "Non-Terminal Sibling".to_string());
    sibling.internal_status = InternalStatus::Executing;
    sibling.ideation_session_id = Some(session.id.clone());
    app_state.task_repo.create(sibling.clone()).await.unwrap();

    let scheduler = build_scheduler(&app_state, &execution_state);
    // Should bypass sibling guard because task is timed out
    scheduler.try_retry_main_merges().await;

    // The main-merge-deferred flag should be cleared (retry was forced)
    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    let flag_cleared = updated
        .metadata
        .as_deref()
        .map(|m| !m.contains("\"main_merge_deferred\":true"))
        .unwrap_or(true);
    assert!(
        flag_cleared,
        "Timed-out main merge should bypass sibling guard and have flag cleared"
    );
}

#[tokio::test]
async fn test_retry_main_merges_respects_sibling_guard_when_not_timed_out() {
    let (execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create an ideation session
    let session = IdeationSession::new(project.id.clone());
    app_state
        .ideation_session_repo
        .create(session.clone())
        .await
        .unwrap();

    // Create a main-merge-deferred task RECENTLY (within timeout)
    let mut task = make_main_deferred_task_with_age(&project.id, "Recent Main Merge", 5);
    task.ideation_session_id = Some(session.id.clone());
    app_state.task_repo.create(task.clone()).await.unwrap();

    // Create a sibling task that is NOT terminal
    let mut sibling = Task::new(project.id.clone(), "Non-Terminal Sibling".to_string());
    sibling.internal_status = InternalStatus::Executing;
    sibling.ideation_session_id = Some(session.id.clone());
    app_state.task_repo.create(sibling.clone()).await.unwrap();

    let scheduler = build_scheduler(&app_state, &execution_state);
    // Should NOT bypass sibling guard (task is within timeout)
    scheduler.try_retry_main_merges().await;

    // The main-merge-deferred flag should still be set (sibling guard skipped the retry)
    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    let flag_still_set = updated
        .metadata
        .as_deref()
        .map(|m| m.contains("\"main_merge_deferred\":true"))
        .unwrap_or(false);
    assert!(
        flag_still_set,
        "Recent main merge should respect sibling guard and not retry (flag still set)"
    );
}

#[tokio::test]
async fn test_retry_main_merges_retries_when_no_session_and_timed_out() {
    let (execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Task with no ideation_session_id but timed out — should always retry
    let seconds_ago = DEFERRED_MERGE_TIMEOUT_SECONDS + 30;
    let task =
        make_main_deferred_task_with_age(&project.id, "Sessionless Timed Out Merge", seconds_ago);
    app_state.task_repo.create(task.clone()).await.unwrap();

    let scheduler = build_scheduler(&app_state, &execution_state);
    scheduler.try_retry_main_merges().await;

    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    let flag_cleared = updated
        .metadata
        .as_deref()
        .map(|m| !m.contains("\"main_merge_deferred\":true"))
        .unwrap_or(true);
    assert!(
        flag_cleared,
        "Timed-out main merge without session should be retried"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Dependency Gate Tests (Scheduler should skip Ready tasks with unsatisfied deps)
// ═══════════════════════════════════════════════════════════════════════

/// Ready task whose sole blocker is Failed should NOT be scheduled.
/// The scheduler should re-block it to Blocked with a reason instead.
#[tokio::test]
async fn test_scheduler_skips_ready_task_with_failed_blocker() {
    let (execution_state, app_state) = setup_test_state().await;
    execution_state.set_max_concurrent(10);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create a blocker task in Failed state
    let mut blocker = Task::new(project.id.clone(), "Blocker Task".to_string());
    blocker.internal_status = InternalStatus::Failed;
    app_state.task_repo.create(blocker.clone()).await.unwrap();

    // Create a Ready task that depends on the Failed blocker
    let mut dependent = Task::new(project.id.clone(), "Dependent Task".to_string());
    dependent.internal_status = InternalStatus::Ready;
    app_state.task_repo.create(dependent.clone()).await.unwrap();

    // Wire up the dependency: dependent depends on blocker
    app_state
        .task_dependency_repo
        .add_dependency(&dependent.id, &blocker.id)
        .await
        .unwrap();

    let scheduler = build_scheduler(&app_state, &execution_state);
    scheduler.try_schedule_ready_tasks().await;

    // Dependent should NOT have been scheduled — it should be re-blocked
    let updated = app_state
        .task_repo
        .get_by_id(&dependent.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::Blocked,
        "Ready task with Failed blocker should be moved to Blocked, not scheduled"
    );
    assert!(
        updated.blocked_reason.is_some(),
        "Re-blocked task should have a blocked_reason set"
    );
}

/// Ready task whose blocker is still Blocked (not satisfied) should NOT be scheduled.
#[tokio::test]
async fn test_scheduler_skips_ready_task_with_blocked_blocker() {
    let (execution_state, app_state) = setup_test_state().await;
    execution_state.set_max_concurrent(10);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create a blocker task still in Blocked state
    let mut blocker = Task::new(project.id.clone(), "Blocked Blocker".to_string());
    blocker.internal_status = InternalStatus::Blocked;
    app_state.task_repo.create(blocker.clone()).await.unwrap();

    // Create a Ready task that depends on the Blocked blocker
    let mut dependent = Task::new(project.id.clone(), "Dependent Task".to_string());
    dependent.internal_status = InternalStatus::Ready;
    app_state.task_repo.create(dependent.clone()).await.unwrap();

    // Wire up the dependency
    app_state
        .task_dependency_repo
        .add_dependency(&dependent.id, &blocker.id)
        .await
        .unwrap();

    let scheduler = build_scheduler(&app_state, &execution_state);
    scheduler.try_schedule_ready_tasks().await;

    // Dependent should NOT have been scheduled
    let updated = app_state
        .task_repo
        .get_by_id(&dependent.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::Blocked,
        "Ready task with Blocked blocker should be moved to Blocked, not scheduled"
    );
}

/// Ready task whose sole blocker is Merged (satisfied) SHOULD be scheduled normally.
/// This is the control test confirming satisfied deps don't block scheduling.
#[tokio::test]
async fn test_scheduler_schedules_ready_task_with_merged_blocker() {
    let (execution_state, app_state) = setup_test_state().await;
    execution_state.set_max_concurrent(10);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create a blocker task that is Merged (dependency satisfied)
    let mut blocker = Task::new(project.id.clone(), "Merged Blocker".to_string());
    blocker.internal_status = InternalStatus::Merged;
    app_state.task_repo.create(blocker.clone()).await.unwrap();

    // Create a Ready task that depends on the Merged blocker
    let mut dependent = Task::new(project.id.clone(), "Dependent Task".to_string());
    dependent.internal_status = InternalStatus::Ready;
    app_state.task_repo.create(dependent.clone()).await.unwrap();

    // Wire up the dependency
    app_state
        .task_dependency_repo
        .add_dependency(&dependent.id, &blocker.id)
        .await
        .unwrap();

    let scheduler = build_scheduler(&app_state, &execution_state);
    scheduler.try_schedule_ready_tasks().await;

    // Dependent SHOULD be scheduled (blocker is Merged = satisfied)
    // In tests without a real agent, this transitions to Failed (ExecutionBlocked)
    let updated = app_state
        .task_repo
        .get_by_id(&dependent.id)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(
        updated.internal_status,
        InternalStatus::Ready,
        "Ready task with Merged blocker should be scheduled (moved out of Ready)"
    );
    assert_ne!(
        updated.internal_status,
        InternalStatus::Blocked,
        "Ready task with Merged blocker should NOT be re-blocked"
    );
}

/// Standalone Ready task (no dependencies at all) SHOULD be scheduled normally.
#[tokio::test]
async fn test_scheduler_schedules_ready_task_with_no_blockers() {
    let (execution_state, app_state) = setup_test_state().await;
    execution_state.set_max_concurrent(10);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create a standalone Ready task with NO dependencies
    let mut task = Task::new(project.id.clone(), "Standalone Task".to_string());
    task.internal_status = InternalStatus::Ready;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let scheduler = build_scheduler(&app_state, &execution_state);
    scheduler.try_schedule_ready_tasks().await;

    // Task SHOULD be scheduled (no blockers = no gate)
    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(
        updated.internal_status,
        InternalStatus::Ready,
        "Standalone task with no blockers should be scheduled"
    );
    assert_ne!(
        updated.internal_status,
        InternalStatus::Blocked,
        "Standalone task should NOT be moved to Blocked"
    );
}

/// Ready task whose sole blocker is Cancelled (dependency satisfied) SHOULD be scheduled.
#[tokio::test]
async fn test_scheduler_schedules_ready_task_with_cancelled_blocker() {
    let (execution_state, app_state) = setup_test_state().await;
    execution_state.set_max_concurrent(10);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create a blocker task that is Cancelled (dependency satisfied per is_dependency_satisfied)
    let mut blocker = Task::new(project.id.clone(), "Cancelled Blocker".to_string());
    blocker.internal_status = InternalStatus::Cancelled;
    app_state.task_repo.create(blocker.clone()).await.unwrap();

    // Create a Ready task that depends on the Cancelled blocker
    let mut dependent = Task::new(project.id.clone(), "Dependent Task".to_string());
    dependent.internal_status = InternalStatus::Ready;
    app_state.task_repo.create(dependent.clone()).await.unwrap();

    // Wire up the dependency
    app_state
        .task_dependency_repo
        .add_dependency(&dependent.id, &blocker.id)
        .await
        .unwrap();

    let scheduler = build_scheduler(&app_state, &execution_state);
    scheduler.try_schedule_ready_tasks().await;

    // Dependent SHOULD be scheduled (Cancelled = satisfied)
    let updated = app_state
        .task_repo
        .get_by_id(&dependent.id)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(
        updated.internal_status,
        InternalStatus::Ready,
        "Ready task with Cancelled blocker should be scheduled"
    );
    assert_ne!(
        updated.internal_status,
        InternalStatus::Blocked,
        "Ready task with Cancelled blocker should NOT be re-blocked"
    );
}

// ============================================================================
// Plan branch guard tests
// ============================================================================

/// Helper to create a plan branch linked to an execution plan
fn create_plan_branch_for_exec_plan(
    project_id: &ProjectId,
    session_id: &IdeationSessionId,
    exec_plan_id: &ExecutionPlanId,
    status: PlanBranchStatus,
) -> PlanBranch {
    let mut branch = PlanBranch::new(
        ArtifactId::from_string("art-test"),
        session_id.clone(),
        project_id.clone(),
        format!("ralphx/test/plan-{}", exec_plan_id.as_str()),
        "main".to_string(),
    );
    branch.status = status;
    branch.execution_plan_id = Some(exec_plan_id.clone());
    branch
}

async fn create_active_execution_plan(
    app_state: &AppState,
    session_id: &IdeationSessionId,
    exec_plan_id: &ExecutionPlanId,
) {
    let mut plan = ExecutionPlan::new(session_id.clone());
    plan.id = exec_plan_id.clone();
    app_state.execution_plan_repo.create(plan).await.unwrap();
}

#[tokio::test]
async fn test_scheduler_skips_task_with_merged_plan_branch() {
    let (execution_state, app_state) = setup_test_state().await;
    execution_state.set_max_concurrent(10);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let session_id = IdeationSessionId::from_string("session-merged");
    let exec_plan_id = ExecutionPlanId::from_string("ep-merged");
    create_active_execution_plan(&app_state, &session_id, &exec_plan_id).await;
    let mut task = Task::new(project.id.clone(), "Plan Task".to_string());
    task.internal_status = InternalStatus::Ready;
    task.ideation_session_id = Some(session_id.clone());
    task.execution_plan_id = Some(exec_plan_id.clone());
    app_state.task_repo.create(task.clone()).await.unwrap();

    // Create a Merged plan branch linked by execution_plan_id
    let branch = create_plan_branch_for_exec_plan(
        &project.id,
        &session_id,
        &exec_plan_id,
        PlanBranchStatus::Merged,
    );
    app_state.plan_branch_repo.create(branch).await.unwrap();

    let scheduler = build_scheduler(&app_state, &execution_state)
        .with_plan_branch_repo(Arc::clone(&app_state.plan_branch_repo));

    // find_oldest_schedulable_task is private, so test via try_schedule_ready_tasks
    scheduler.try_schedule_ready_tasks().await;

    // Task should still be Ready (skipped due to merged branch)
    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::Ready,
        "Task on merged branch should not be scheduled"
    );
}

#[tokio::test]
async fn test_scheduler_skips_task_with_abandoned_plan_branch() {
    let (execution_state, app_state) = setup_test_state().await;
    execution_state.set_max_concurrent(10);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let session_id = IdeationSessionId::from_string("session-abandoned");
    let exec_plan_id = ExecutionPlanId::from_string("ep-abandoned");
    create_active_execution_plan(&app_state, &session_id, &exec_plan_id).await;
    let mut task = Task::new(project.id.clone(), "Plan Task".to_string());
    task.internal_status = InternalStatus::Ready;
    task.ideation_session_id = Some(session_id.clone());
    task.execution_plan_id = Some(exec_plan_id.clone());
    app_state.task_repo.create(task.clone()).await.unwrap();

    // Create an Abandoned plan branch linked by execution_plan_id
    let branch = create_plan_branch_for_exec_plan(
        &project.id,
        &session_id,
        &exec_plan_id,
        PlanBranchStatus::Abandoned,
    );
    app_state.plan_branch_repo.create(branch).await.unwrap();

    let scheduler = build_scheduler(&app_state, &execution_state)
        .with_plan_branch_repo(Arc::clone(&app_state.plan_branch_repo));

    scheduler.try_schedule_ready_tasks().await;

    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::Ready,
        "Task on abandoned branch should not be scheduled"
    );
}

#[tokio::test]
async fn test_scheduler_allows_task_with_active_plan_branch() {
    let (execution_state, app_state) = setup_test_state().await;
    execution_state.set_max_concurrent(10);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let session_id = IdeationSessionId::from_string("session-active");
    let exec_plan_id = ExecutionPlanId::from_string("ep-active");
    create_active_execution_plan(&app_state, &session_id, &exec_plan_id).await;
    let mut task = Task::new(project.id.clone(), "Plan Task".to_string());
    task.internal_status = InternalStatus::Ready;
    task.ideation_session_id = Some(session_id.clone());
    task.execution_plan_id = Some(exec_plan_id.clone());
    app_state.task_repo.create(task.clone()).await.unwrap();

    // Create an Active plan branch linked by execution_plan_id
    let branch = create_plan_branch_for_exec_plan(
        &project.id,
        &session_id,
        &exec_plan_id,
        PlanBranchStatus::Active,
    );
    app_state.plan_branch_repo.create(branch).await.unwrap();

    let scheduler = build_scheduler(&app_state, &execution_state)
        .with_plan_branch_repo(Arc::clone(&app_state.plan_branch_repo));

    scheduler.try_schedule_ready_tasks().await;

    // Task should have been scheduled (moved from Ready)
    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(
        updated.internal_status,
        InternalStatus::Ready,
        "Task on active branch should be scheduled"
    );
}

#[tokio::test]
async fn test_scheduler_allows_task_without_execution_plan() {
    let (execution_state, app_state) = setup_test_state().await;
    execution_state.set_max_concurrent(10);

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Task with no execution_plan_id (non-plan task)
    let mut task = Task::new(project.id.clone(), "Standalone Task".to_string());
    task.internal_status = InternalStatus::Ready;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let scheduler = build_scheduler(&app_state, &execution_state)
        .with_plan_branch_repo(Arc::clone(&app_state.plan_branch_repo));

    scheduler.try_schedule_ready_tasks().await;

    // Task should have been scheduled (non-plan tasks bypass the guard)
    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(
        updated.internal_status,
        InternalStatus::Ready,
        "Non-plan task should be scheduled regardless of plan branches"
    );
}
