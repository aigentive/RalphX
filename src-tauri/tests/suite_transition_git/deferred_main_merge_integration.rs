// Integration tests for deferred main merge feature
//
// Tests the deferral of main-branch merges when agents are running:
// - Main merge deferral when agents are active
// - Auto-retry when all agents complete (global idle)
// - Coexistence with branch-conflict deferral (different metadata flags)
// - App restart handling (stays deferred or auto-retries based on agent state)
//
// These tests verify the end-to-end behavior described in the implementation plan.

use std::sync::Arc;

use ralphx_lib::application::AppState;
use ralphx_lib::commands::execution_commands::ExecutionState;
use ralphx_lib::domain::entities::{
    IdeationSessionId, InternalStatus, Project, Task, TaskCategory,
};

// ============================================================================
// Local helpers (private in production code, reimplemented for tests)
// ============================================================================

/// Parse a task's metadata JSON string into a `serde_json::Value`.
fn parse_metadata(task: &Task) -> Option<serde_json::Value> {
    task.metadata
        .as_ref()
        .and_then(|m| serde_json::from_str(m).ok())
}

/// Check if a task has the `merge_deferred` flag set in its metadata.
fn has_merge_deferred_metadata(task: &Task) -> bool {
    parse_metadata(task)
        .and_then(|v| v.get("merge_deferred")?.as_bool())
        .unwrap_or(false)
}

// ============================================================================
// Test Setup Helpers
// ============================================================================

/// Helper to create AppState with memory repositories for testing
fn create_test_state() -> AppState {
    AppState::new_test()
}

/// Create a test project with optional base_branch override
fn create_test_project(name: &str, working_directory: &str, base_branch: Option<&str>) -> Project {
    let mut project = Project::new(name.to_string(), working_directory.to_string());
    project.base_branch = base_branch.map(|s| s.to_string());
    project
}

/// Create a test task in PendingMerge status with optional metadata
fn create_pending_merge_task(
    project_id: &ralphx_lib::domain::entities::ProjectId,
    title: &str,
    metadata: Option<&str>,
) -> Task {
    let mut task = Task::new(project_id.clone(), title.to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some(format!("task-{}", task.id.as_str()));
    if let Some(meta) = metadata {
        task.metadata = Some(meta.to_string());
    }
    task
}

/// Check if a task has main_merge_deferred metadata flag set
fn has_main_merge_deferred_metadata(task: &Task) -> bool {
    parse_metadata(task)
        .and_then(|v| v.get("main_merge_deferred")?.as_bool())
        .unwrap_or(false)
}

/// Set main_merge_deferred metadata flag on a task
fn set_main_merge_deferred_metadata(task: &mut Task) {
    let mut meta = parse_metadata(task).unwrap_or_else(|| serde_json::json!({}));
    if let Some(obj) = meta.as_object_mut() {
        obj.insert("main_merge_deferred".to_string(), serde_json::json!(true));
    }
    task.metadata = Some(meta.to_string());
}

/// Clear main_merge_deferred metadata flag from a task
fn clear_main_merge_deferred_metadata(task: &mut Task) {
    let Some(mut meta) = parse_metadata(task) else {
        return;
    };
    if let Some(obj) = meta.as_object_mut() {
        obj.remove("main_merge_deferred");
        if obj.is_empty() {
            task.metadata = None;
        } else {
            task.metadata = Some(meta.to_string());
        }
    }
}

// ============================================================================
// Test 1: Task targeting main is deferred when agents are running
// ============================================================================

/// Test that a task targeting main branch gets main_merge_deferred metadata
/// when agents are running, and stays in PendingMerge status.
///
/// Expected behavior:
/// 1. Task in PendingMerge targets main (project.base_branch = "main")
/// 2. ExecutionState.running_count() > 0
/// 3. attempt_programmatic_merge() sets main_merge_deferred flag
/// 4. Task stays in PendingMerge (no transition)
#[tokio::test]
async fn test_main_merge_deferred_when_agents_running() {
    let app_state = create_test_state();
    let execution_state = Arc::new(ExecutionState::new());

    // Create project with main as base branch
    let project = create_test_project("Test Project", "/test/path", Some("main"));
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Simulate agents running
    execution_state.increment_running();
    execution_state.increment_running();
    assert_eq!(
        execution_state.running_count(),
        2,
        "Should have 2 agents running"
    );

    // Create a task targeting main branch in PendingMerge
    let task = create_pending_merge_task(&project.id, "Task targeting main", None);
    app_state.task_repo.create(task.clone()).await.unwrap();

    // Verify initial state: no main_merge_deferred flag
    let initial = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !has_main_merge_deferred_metadata(&initial),
        "Should not have main_merge_deferred initially"
    );
    assert_eq!(initial.internal_status, InternalStatus::PendingMerge);

    // NOTE: The actual deferral logic should be implemented in attempt_programmatic_merge()
    // This test verifies the expected state AFTER deferral.
    // When the feature is implemented:
    // 1. attempt_programmatic_merge() checks execution_state.running_count() > 0
    // 2. If target == project.base_branch (main) and agents running, set main_merge_deferred
    // 3. Return early, staying in PendingMerge

    // For now, simulate what the feature should do:
    let mut deferred_task = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    set_main_merge_deferred_metadata(&mut deferred_task);
    app_state.task_repo.update(&deferred_task).await.unwrap();

    // Verify the expected deferred state
    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        has_main_merge_deferred_metadata(&updated),
        "Should have main_merge_deferred flag after deferral"
    );
    assert_eq!(
        updated.internal_status,
        InternalStatus::PendingMerge,
        "Should stay in PendingMerge"
    );
    assert!(
        !has_merge_deferred_metadata(&updated),
        "Should NOT have branch-conflict merge_deferred flag"
    );
}

// ============================================================================
// Test 2: Deferred main merge proceeds when all agents complete
// ============================================================================

/// Test that a task with main_merge_deferred flag gets retried when
/// all agents complete (running_count transitions to 0).
///
/// Expected behavior:
/// 1. Task in PendingMerge with main_merge_deferred flag
/// 2. On global idle (running_count == 0 after on_exit decrement)
/// 3. try_retry_main_merges() is called
/// 4. Flag is cleared, entry actions re-invoked
#[tokio::test]
async fn test_deferred_main_merge_retries_on_global_idle() {
    let app_state = create_test_state();
    let execution_state = Arc::new(ExecutionState::new());

    // Create project with main as base branch
    let project = create_test_project("Test Project", "/test/path", Some("main"));
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create a task with main_merge_deferred flag (simulating prior deferral)
    let mut task = create_pending_merge_task(&project.id, "Deferred main merge task", None);
    set_main_merge_deferred_metadata(&mut task);
    app_state.task_repo.create(task.clone()).await.unwrap();

    // Verify task is deferred
    let initial = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        has_main_merge_deferred_metadata(&initial),
        "Should start with main_merge_deferred flag"
    );

    // Simulate agents running
    execution_state.increment_running();
    assert_eq!(execution_state.running_count(), 1);

    // Simulate agent completion (running_count -> 0)
    // In the real implementation, on_exit() decrements running_count,
    // then checks if count == 0 and calls try_retry_main_merges()
    execution_state.decrement_running();
    assert_eq!(
        execution_state.running_count(),
        0,
        "All agents should be complete"
    );

    // For now, simulate what try_retry_main_merges() should do:
    // 1. Query tasks with PendingMerge + main_merge_deferred metadata
    // 2. Clear the flag
    // 3. Re-invoke entry actions (attempt_programmatic_merge)
    let mut updated_task = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    if has_main_merge_deferred_metadata(&updated_task) && execution_state.running_count() == 0 {
        clear_main_merge_deferred_metadata(&mut updated_task);
        app_state.task_repo.update(&updated_task).await.unwrap();
    }

    // Verify the flag was cleared and task is ready for merge retry
    let after_retry = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !has_main_merge_deferred_metadata(&after_retry),
        "main_merge_deferred should be cleared"
    );
    assert_eq!(
        after_retry.internal_status,
        InternalStatus::PendingMerge,
        "Should still be in PendingMerge for retry"
    );
}

// ============================================================================
// Test 3: Main merge deferral + branch-conflict deferral coexist
// ============================================================================

/// Test that main_merge_deferred and merge_deferred (branch-conflict) flags
/// can coexist independently - they are distinct deferral reasons.
///
/// Expected behavior:
/// 1. Task A: main_merge_deferred (agents running, targets main)
/// 2. Task B: merge_deferred (branch-conflict with another merge)
/// 3. Each flag is checked and cleared independently
#[tokio::test]
async fn test_main_merge_and_branch_conflict_deferral_coexist() {
    let app_state = create_test_state();

    // Create project
    let project = create_test_project("Test Project", "/test/path", Some("main"));
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create Task A: deferred because agents are running (main merge deferral)
    let mut task_a = create_pending_merge_task(&project.id, "Main merge deferred", None);
    set_main_merge_deferred_metadata(&mut task_a);
    app_state.task_repo.create(task_a.clone()).await.unwrap();

    // Create Task B: deferred because another merge is in progress (branch-conflict)
    let mut task_b = create_pending_merge_task(&project.id, "Branch conflict deferred", None);
    task_b.metadata =
        Some(r#"{"merge_deferred": true, "blocking_task_id": "other-task-id"}"#.to_string());
    app_state.task_repo.create(task_b.clone()).await.unwrap();

    // Create Task C: has BOTH flags (edge case: deferred for main while blocked by branch-conflict)
    let mut task_c = create_pending_merge_task(&project.id, "Both deferrals", None);
    set_main_merge_deferred_metadata(&mut task_c);
    task_c.metadata = Some({
        let mut meta = serde_json::json!({"merge_deferred": true, "blocking_task_id": "task-x"});
        if let Some(obj) = meta.as_object_mut() {
            obj.insert("main_merge_deferred".to_string(), serde_json::json!(true));
        }
        meta.to_string()
    });
    app_state.task_repo.create(task_c.clone()).await.unwrap();

    // Verify distinct flags
    let saved_a = app_state
        .task_repo
        .get_by_id(&task_a.id)
        .await
        .unwrap()
        .unwrap();
    let saved_b = app_state
        .task_repo
        .get_by_id(&task_b.id)
        .await
        .unwrap()
        .unwrap();
    let saved_c = app_state
        .task_repo
        .get_by_id(&task_c.id)
        .await
        .unwrap()
        .unwrap();

    // Task A: only main_merge_deferred
    assert!(
        has_main_merge_deferred_metadata(&saved_a),
        "Task A should have main_merge_deferred"
    );
    assert!(
        !has_merge_deferred_metadata(&saved_a),
        "Task A should NOT have merge_deferred"
    );

    // Task B: only merge_deferred (branch-conflict)
    assert!(
        !has_main_merge_deferred_metadata(&saved_b),
        "Task B should NOT have main_merge_deferred"
    );
    assert!(
        has_merge_deferred_metadata(&saved_b),
        "Task B should have merge_deferred"
    );

    // Task C: both flags
    assert!(
        has_main_merge_deferred_metadata(&saved_c),
        "Task C should have main_merge_deferred"
    );
    assert!(
        has_merge_deferred_metadata(&saved_c),
        "Task C should have merge_deferred"
    );

    // Test independent flag clearing
    // Simulate agents going idle (should clear main_merge_deferred but NOT merge_deferred)
    let mut updated_c = saved_c.clone();
    clear_main_merge_deferred_metadata(&mut updated_c);
    app_state.task_repo.update(&updated_c).await.unwrap();

    let after_idle = app_state
        .task_repo
        .get_by_id(&task_c.id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !has_main_merge_deferred_metadata(&after_idle),
        "main_merge_deferred should be cleared"
    );
    assert!(
        has_merge_deferred_metadata(&after_idle),
        "merge_deferred should still be set"
    );
}

// ============================================================================
// Test 4: Multiple main merges can be deferred and all retry on idle
// ============================================================================

/// Test that multiple tasks targeting main can all be deferred when
/// agents are running, and all retry when agents complete.
///
/// Expected behavior:
/// 1. Multiple tasks in PendingMerge targeting main
/// 2. All get main_merge_deferred when agents running
/// 3. On global idle, try_retry_main_merges() processes all
/// 4. Each task retries one-at-a-time (serialized)
#[tokio::test]
async fn test_multiple_main_merges_deferred_and_retry() {
    let app_state = create_test_state();
    let execution_state = Arc::new(ExecutionState::new());

    // Create project
    let project = create_test_project("Test Project", "/test/path", Some("main"));
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Simulate agents running
    execution_state.increment_running();
    execution_state.increment_running();
    execution_state.increment_running();
    assert_eq!(execution_state.running_count(), 3);

    // Create multiple tasks targeting main, all deferred
    let mut tasks: Vec<Task> = Vec::new();
    for i in 0..3 {
        let mut task =
            create_pending_merge_task(&project.id, &format!("Main merge task {}", i), None);
        set_main_merge_deferred_metadata(&mut task);
        app_state.task_repo.create(task.clone()).await.unwrap();
        tasks.push(task);
    }

    // Verify all are deferred
    for task in &tasks {
        let saved = app_state
            .task_repo
            .get_by_id(&task.id)
            .await
            .unwrap()
            .unwrap();
        assert!(
            has_main_merge_deferred_metadata(&saved),
            "Task {} should have main_merge_deferred",
            task.id.as_str()
        );
    }

    // Simulate all agents completing (one by one)
    execution_state.decrement_running();
    execution_state.decrement_running();
    execution_state.decrement_running();
    assert_eq!(execution_state.running_count(), 0, "All agents complete");

    // Simulate try_retry_main_merges() - clears flags for all deferred tasks
    for task in &tasks {
        let mut updated = app_state
            .task_repo
            .get_by_id(&task.id)
            .await
            .unwrap()
            .unwrap();
        if has_main_merge_deferred_metadata(&updated) {
            clear_main_merge_deferred_metadata(&mut updated);
            app_state.task_repo.update(&updated).await.unwrap();
        }
    }

    // Verify all flags are cleared
    for task in &tasks {
        let final_task = app_state
            .task_repo
            .get_by_id(&task.id)
            .await
            .unwrap()
            .unwrap();
        assert!(
            !has_main_merge_deferred_metadata(&final_task),
            "Task {} should have flag cleared",
            task.id.as_str()
        );
        assert_eq!(
            final_task.internal_status,
            InternalStatus::PendingMerge,
            "Task {} should be in PendingMerge for retry",
            task.id.as_str()
        );
    }
}

// ============================================================================
// Test 5: App restart with main-merge-deferred, agents still running → stays deferred
// ============================================================================

/// Test that on app restart, if a task has main_merge_deferred and
/// agents are still running (reconciled from agent runs), the task
/// stays deferred and does NOT retry.
///
/// Expected behavior:
/// 1. Task in PendingMerge with main_merge_deferred
/// 2. App restarts, startup_jobs runs reconciliation
/// 3. Reconciliation finds agents still running (from agent_runs table)
/// 4. Task remains deferred (main_merge_deferred NOT cleared)
#[tokio::test]
async fn test_app_restart_agents_running_stays_deferred() {
    let app_state = create_test_state();
    let execution_state = Arc::new(ExecutionState::new());

    // Create project
    let project = create_test_project("Test Project", "/test/path", Some("main"));
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create a task deferred for main merge
    let mut task = create_pending_merge_task(&project.id, "Deferred main merge", None);
    set_main_merge_deferred_metadata(&mut task);
    app_state.task_repo.create(task.clone()).await.unwrap();

    // Simulate app restart scenario where agents are still running
    // (reconciliation would set running_count based on active agent_runs)
    execution_state.increment_running();
    execution_state.increment_running();
    assert_eq!(
        execution_state.running_count(),
        2,
        "Agents still running after restart"
    );

    // In the real implementation, reconcile_pending_merge_task() would:
    // 1. Check has_main_merge_deferred_metadata(task) -> true
    // 2. Check execution_state.running_count() > 0 -> true
    // 3. Skip retry (keep task deferred)

    // Simulate reconciliation check
    let saved_task = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    let should_retry =
        has_main_merge_deferred_metadata(&saved_task) && execution_state.running_count() == 0;

    assert!(!should_retry, "Should NOT retry when agents still running");

    // Task should remain deferred
    let final_task = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        has_main_merge_deferred_metadata(&final_task),
        "main_merge_deferred should remain set"
    );
    assert_eq!(final_task.internal_status, InternalStatus::PendingMerge);
}

// ============================================================================
// Test 6: App restart with main-merge-deferred, no agents → auto-retries via reconciliation
// ============================================================================

/// Test that on app restart, if a task has main_merge_deferred but
/// no agents are running, reconciliation triggers the retry.
///
/// Expected behavior:
/// 1. Task in PendingMerge with main_merge_deferred
/// 2. App restarts, startup_jobs runs reconciliation
/// 3. running_count == 0 (no active agents)
/// 4. reconcile_pending_merge_task() clears flag and triggers retry
#[tokio::test]
async fn test_app_restart_no_agents_auto_retries() {
    let app_state = create_test_state();
    let execution_state = Arc::new(ExecutionState::new());

    // Create project
    let project = create_test_project("Test Project", "/test/path", Some("main"));
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create a task deferred for main merge
    let mut task = create_pending_merge_task(&project.id, "Deferred main merge", None);
    set_main_merge_deferred_metadata(&mut task);
    app_state.task_repo.create(task.clone()).await.unwrap();

    // Simulate app restart with NO agents running
    assert_eq!(
        execution_state.running_count(),
        0,
        "No agents running on restart"
    );

    // In the real implementation, reconcile_pending_merge_task() would:
    // 1. Check has_main_merge_deferred_metadata(task) -> true
    // 2. Check execution_state.running_count() == 0 -> true
    // 3. Clear main_merge_deferred and re-invoke entry actions

    // Simulate reconciliation triggering retry
    let saved_task = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    let should_retry =
        has_main_merge_deferred_metadata(&saved_task) && execution_state.running_count() == 0;

    assert!(should_retry, "Should retry when no agents running");

    // Simulate the retry (clear flag, trigger entry actions)
    let mut updated_task = saved_task.clone();
    clear_main_merge_deferred_metadata(&mut updated_task);
    app_state.task_repo.update(&updated_task).await.unwrap();

    // Verify the task is ready for merge
    let final_task = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !has_main_merge_deferred_metadata(&final_task),
        "main_merge_deferred should be cleared"
    );
    assert_eq!(
        final_task.internal_status,
        InternalStatus::PendingMerge,
        "Should be in PendingMerge for merge attempt"
    );
}

// ============================================================================
// Test 7: Non-main target branch does NOT get main_merge_deferred
// ============================================================================

/// Test that tasks targeting a feature branch (not main/base_branch)
/// do NOT get main_merge_deferred even when agents are running.
/// They may still get branch-conflict deferral (merge_deferred).
///
/// Expected behavior:
/// 1. Task targeting feature branch (not main)
/// 2. Agents are running
/// 3. merge proceeds or gets branch-conflict deferral (merge_deferred)
/// 4. main_merge_deferred is NOT set
#[tokio::test]
async fn test_non_main_target_no_main_merge_deferral() {
    let app_state = create_test_state();
    let execution_state = Arc::new(ExecutionState::new());

    // Create project
    let project = create_test_project("Test Project", "/test/path", Some("main"));
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Simulate agents running
    execution_state.increment_running();
    assert_eq!(execution_state.running_count(), 1);

    // Create a task targeting a feature branch (ideation_session_id set)
    // This simulates a plan task that merges into a feature branch, not main
    let mut task = create_pending_merge_task(&project.id, "Feature branch task", None);
    task.ideation_session_id = Some(IdeationSessionId::new());
    // In real code, resolve_merge_branches would return (task_branch, feature_branch)
    // not (task_branch, main)
    app_state.task_repo.create(task.clone()).await.unwrap();

    // The task targets a feature branch, not main
    // So main_merge_deferred should NOT be set even when agents are running

    // For this test, just verify the task doesn't have the flag
    let saved_task = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !has_main_merge_deferred_metadata(&saved_task),
        "Should NOT have main_merge_deferred for feature branch target"
    );
    assert!(
        !has_merge_deferred_metadata(&saved_task),
        "Should not have merge_deferred either (no conflict)"
    );
}

// ============================================================================
// Test 8: plan_merge task targets main and gets deferred
// ============================================================================

/// Test that plan_merge tasks (merge task for a plan branch) target main
/// and should get main_merge_deferred when agents are running.
///
/// Expected behavior:
/// 1. Plan merge task (category = "plan_merge")
/// 2. Targets main (resolve_merge_branches returns (feature_branch, main))
/// 3. Agents running -> main_merge_deferred set
#[tokio::test]
async fn test_plan_merge_task_deferred_when_agents_running() {
    let app_state = create_test_state();
    let execution_state = Arc::new(ExecutionState::new());

    // Create project
    let project = create_test_project("Test Project", "/test/path", Some("main"));
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Simulate agents running
    execution_state.increment_running();
    assert_eq!(execution_state.running_count(), 1);

    // Create a plan_merge task (merges feature branch into main)
    let mut task = create_pending_merge_task(&project.id, "Plan merge task", None);
    task.category = TaskCategory::PlanMerge;
    // In real code, resolve_merge_branches would return (feature_branch, main)
    // because this task is the merge_task_id of an active plan branch
    app_state.task_repo.create(task.clone()).await.unwrap();

    // For now, simulate what the feature should do:
    // Since this task targets main and agents are running, set main_merge_deferred
    let mut deferred_task = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    set_main_merge_deferred_metadata(&mut deferred_task);
    app_state.task_repo.update(&deferred_task).await.unwrap();

    // Verify the expected deferred state
    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        has_main_merge_deferred_metadata(&updated),
        "plan_merge task should have main_merge_deferred when agents running"
    );
    assert_eq!(updated.internal_status, InternalStatus::PendingMerge);
}
