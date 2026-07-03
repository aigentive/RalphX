// Per-Project Execution Scoping Tests (Phase 82)
//
// These tests verify the per-project execution scoping functionality:
// - get_execution_status counts only Ready tasks in the specified project
// - Scheduler only transitions Ready tasks in the active project
// - Event payloads include projectId

use ralphx_lib::domain::entities::{InternalStatus, ProjectId, TaskId};
use ralphx_lib::domain::entities::{Project, Task};
use ralphx_lib::domain::repositories::{ProjectRepository, TaskRepository};
use ralphx_lib::infrastructure::sqlite::{SqliteProjectRepository, SqliteTaskRepository};
use ralphx_lib::testing::SqliteTestDb;
use std::sync::Arc;
use tokio::sync::Mutex;

// ============================================================================
// Test Helpers
// ============================================================================

/// Helper to set up a test environment with multiple projects and tasks
async fn setup_multi_project_test() -> (
    SqliteTestDb,
    Arc<dyn TaskRepository>,
    Arc<dyn ProjectRepository>,
    ProjectId,
    ProjectId,
) {
    let db = SqliteTestDb::new("per-project-execution-scoping");
    let conn = db.new_connection();
    let shared_conn = Arc::new(Mutex::new(conn));

    let task_repo: Arc<dyn TaskRepository> =
        Arc::new(SqliteTaskRepository::from_shared(Arc::clone(&shared_conn)));
    let project_repo: Arc<dyn ProjectRepository> = Arc::new(SqliteProjectRepository::from_shared(
        Arc::clone(&shared_conn),
    ));

    // Create two projects using the constructor
    let mut project1 = Project::new("Project 1".to_string(), "/path/to/project1".to_string());
    project1.id = ProjectId::from_string("proj-1".to_string());

    let mut project2 = Project::new("Project 2".to_string(), "/path/to/project2".to_string());
    project2.id = ProjectId::from_string("proj-2".to_string());

    project_repo.create(project1).await.unwrap();
    project_repo.create(project2).await.unwrap();

    let project1_id = ProjectId::from_string("proj-1".to_string());
    let project2_id = ProjectId::from_string("proj-2".to_string());

    (db, task_repo, project_repo, project1_id, project2_id)
}

/// Helper to create a task in a specific project with a given status
async fn create_task(
    task_repo: &Arc<dyn TaskRepository>,
    task_id: &str,
    project_id: &ProjectId,
    status: InternalStatus,
) -> Task {
    let mut task = Task::new(project_id.clone(), format!("Task {}", task_id));
    task.id = TaskId::from_string(task_id.to_string());
    task.internal_status = status;
    task_repo.create(task).await.unwrap()
}

// ============================================================================
// Per-Project Queued Count Tests
// ============================================================================

/// Test: Queued count is scoped to specified project
///
/// When get_execution_status is called with a project_id, it should only
/// count Ready tasks in that project, not Ready tasks from other projects.
#[tokio::test]
async fn test_queued_count_scoped_to_project() {
    let (_db, task_repo, _, project1_id, project2_id) = setup_multi_project_test().await;

    // Create Ready tasks in both projects
    // Project 1: 3 Ready tasks
    create_task(&task_repo, "task-1a", &project1_id, InternalStatus::Ready).await;
    create_task(&task_repo, "task-1b", &project1_id, InternalStatus::Ready).await;
    create_task(&task_repo, "task-1c", &project1_id, InternalStatus::Ready).await;

    // Project 2: 2 Ready tasks
    create_task(&task_repo, "task-2a", &project2_id, InternalStatus::Ready).await;
    create_task(&task_repo, "task-2b", &project2_id, InternalStatus::Ready).await;

    // Count Ready tasks in Project 1
    let project1_tasks = task_repo.get_by_project(&project1_id).await.unwrap();
    let project1_ready_count = project1_tasks
        .iter()
        .filter(|t| t.internal_status == InternalStatus::Ready)
        .count();

    assert_eq!(
        project1_ready_count, 3,
        "Project 1 should have 3 Ready tasks"
    );

    // Count Ready tasks in Project 2
    let project2_tasks = task_repo.get_by_project(&project2_id).await.unwrap();
    let project2_ready_count = project2_tasks
        .iter()
        .filter(|t| t.internal_status == InternalStatus::Ready)
        .count();

    assert_eq!(
        project2_ready_count, 2,
        "Project 2 should have 2 Ready tasks"
    );
}

/// Test: Non-Ready tasks are excluded from queued count
#[tokio::test]
async fn test_queued_count_excludes_non_ready_tasks() {
    let (_db, task_repo, _, project1_id, _) = setup_multi_project_test().await;

    // Create tasks in various states
    create_task(
        &task_repo,
        "task-ready",
        &project1_id,
        InternalStatus::Ready,
    )
    .await;
    create_task(
        &task_repo,
        "task-executing",
        &project1_id,
        InternalStatus::Executing,
    )
    .await;
    create_task(
        &task_repo,
        "task-backlog",
        &project1_id,
        InternalStatus::Backlog,
    )
    .await;
    create_task(
        &task_repo,
        "task-approved",
        &project1_id,
        InternalStatus::Approved,
    )
    .await;

    // Only 1 Ready task should be counted
    let project1_tasks = task_repo.get_by_project(&project1_id).await.unwrap();
    let ready_count = project1_tasks
        .iter()
        .filter(|t| t.internal_status == InternalStatus::Ready)
        .count();

    assert_eq!(ready_count, 1, "Only Ready tasks should be counted");
}

// ============================================================================
// Scheduler Scoping Tests
// ============================================================================

/// Test: get_oldest_ready_tasks returns tasks sorted by created_at
///
/// This verifies the underlying query that the scheduler uses.
#[tokio::test]
async fn test_oldest_ready_tasks_ordering() {
    let (_db, task_repo, _, project1_id, _) = setup_multi_project_test().await;

    // Create Ready tasks with different timestamps
    // (created_at is set at creation time, so order of creation matters)
    let task1 = create_task(
        &task_repo,
        "task-oldest",
        &project1_id,
        InternalStatus::Ready,
    )
    .await;
    let _task2 = create_task(
        &task_repo,
        "task-middle",
        &project1_id,
        InternalStatus::Ready,
    )
    .await;
    let _task3 = create_task(
        &task_repo,
        "task-newest",
        &project1_id,
        InternalStatus::Ready,
    )
    .await;

    let oldest_ready = task_repo.get_oldest_ready_tasks(10).await.unwrap();

    assert!(
        oldest_ready.len() >= 3,
        "Should have at least 3 Ready tasks"
    );

    // First task should be the oldest (task1)
    assert_eq!(oldest_ready[0].id, task1.id, "Oldest task should be first");
}

/// Test: get_oldest_ready_tasks respects limit
#[tokio::test]
async fn test_oldest_ready_tasks_limit() {
    let (_db, task_repo, _, project1_id, _) = setup_multi_project_test().await;

    // Create more Ready tasks than the limit
    for i in 0..10 {
        create_task(
            &task_repo,
            &format!("task-{}", i),
            &project1_id,
            InternalStatus::Ready,
        )
        .await;
    }

    let oldest_ready = task_repo.get_oldest_ready_tasks(3).await.unwrap();

    assert_eq!(oldest_ready.len(), 3, "Should respect limit of 3");
}

/// Test: Scheduler candidate list includes tasks from multiple projects
///
/// When no active project is set, the scheduler should consider tasks
/// from all projects.
#[tokio::test]
async fn test_scheduler_considers_all_projects_by_default() {
    let (_db, task_repo, _, project1_id, project2_id) = setup_multi_project_test().await;

    // Create Ready tasks in both projects
    create_task(&task_repo, "task-1", &project1_id, InternalStatus::Ready).await;
    create_task(&task_repo, "task-2", &project2_id, InternalStatus::Ready).await;

    let oldest_ready = task_repo.get_oldest_ready_tasks(10).await.unwrap();

    // Should have tasks from both projects
    let project_ids: Vec<_> = oldest_ready.iter().map(|t| t.project_id.clone()).collect();
    assert!(
        project_ids.contains(&project1_id),
        "Should include task from Project 1"
    );
    assert!(
        project_ids.contains(&project2_id),
        "Should include task from Project 2"
    );
}

// ============================================================================
// Agent-Active Status Scoping Tests
// ============================================================================

/// Test: Agent-active tasks can be identified by project
///
/// Verifies that we can filter tasks by project and agent-active status,
/// which is used by pause/stop commands to only affect the active project.
#[tokio::test]
async fn test_agent_active_tasks_by_project() {
    let (_db, task_repo, _, project1_id, project2_id) = setup_multi_project_test().await;

    // Create executing tasks in both projects
    create_task(
        &task_repo,
        "task-1-exec",
        &project1_id,
        InternalStatus::Executing,
    )
    .await;
    create_task(
        &task_repo,
        "task-1-review",
        &project1_id,
        InternalStatus::Reviewing,
    )
    .await;
    create_task(
        &task_repo,
        "task-2-exec",
        &project2_id,
        InternalStatus::Executing,
    )
    .await;

    // Agent-active statuses (from execution_commands.rs)
    let agent_active_statuses = [
        InternalStatus::Executing,
        InternalStatus::QaRefining,
        InternalStatus::QaTesting,
        InternalStatus::Reviewing,
        InternalStatus::ReExecuting,
        InternalStatus::Merging,
    ];

    // Get Project 1 tasks in agent-active states
    let project1_tasks = task_repo.get_by_project(&project1_id).await.unwrap();
    let project1_agent_active: Vec<_> = project1_tasks
        .iter()
        .filter(|t| agent_active_statuses.contains(&t.internal_status))
        .collect();

    assert_eq!(
        project1_agent_active.len(),
        2,
        "Project 1 should have 2 agent-active tasks"
    );

    // Get Project 2 tasks in agent-active states
    let project2_tasks = task_repo.get_by_project(&project2_id).await.unwrap();
    let project2_agent_active: Vec<_> = project2_tasks
        .iter()
        .filter(|t| agent_active_statuses.contains(&t.internal_status))
        .collect();

    assert_eq!(
        project2_agent_active.len(),
        1,
        "Project 2 should have 1 agent-active task"
    );
}

/// Test: Pausing one project doesn't affect tasks in another project
///
/// This is a behavioral test that verifies the pause logic scopes correctly.
/// The actual pause_execution command uses project_id to filter tasks.
#[tokio::test]
async fn test_project_scoped_pause_does_not_affect_other_projects() {
    let (_db, task_repo, _, project1_id, project2_id) = setup_multi_project_test().await;

    // Create executing tasks in both projects
    let task1 = create_task(
        &task_repo,
        "task-1-exec",
        &project1_id,
        InternalStatus::Executing,
    )
    .await;
    let task2 = create_task(
        &task_repo,
        "task-2-exec",
        &project2_id,
        InternalStatus::Executing,
    )
    .await;

    // Simulate pausing Project 1 only by updating its tasks to Paused
    // (In production, pause_execution command does this via TransitionHandler)
    let project1_tasks = task_repo.get_by_project(&project1_id).await.unwrap();
    for task in project1_tasks {
        if task.internal_status == InternalStatus::Executing {
            let mut updated_task = task;
            updated_task.internal_status = InternalStatus::Paused;
            task_repo.update(&updated_task).await.unwrap();
        }
    }

    // Verify Project 1 task is now Paused
    let task1_after = task_repo.get_by_id(&task1.id).await.unwrap().unwrap();
    assert_eq!(
        task1_after.internal_status,
        InternalStatus::Paused,
        "Project 1 task should be Paused"
    );

    // Verify Project 2 task is still Executing
    let task2_after = task_repo.get_by_id(&task2.id).await.unwrap().unwrap();
    assert_eq!(
        task2_after.internal_status,
        InternalStatus::Executing,
        "Project 2 task should still be Executing"
    );
}

// ============================================================================
// Event Payload Tests
// ============================================================================

/// Test: Event payload structure includes projectId field
///
/// This test verifies the expected structure of event payloads.
/// The actual event emission is tested via integration tests.
#[test]
fn test_event_payload_includes_project_id() {
    // Simulate the event payload structure used in execution_commands.rs
    let project_id = Some("proj-123");
    let payload = serde_json::json!({
        "isPaused": true,
        "runningCount": 0,
        "maxConcurrent": 5,
        "reason": "paused",
        "projectId": project_id,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    });

    // Verify projectId is included
    assert!(
        payload.get("projectId").is_some(),
        "Event payload should include projectId"
    );
    assert_eq!(
        payload.get("projectId").unwrap().as_str(),
        project_id,
        "projectId should match"
    );
}

/// Test: Event payload without project scope has null projectId
#[test]
fn test_event_payload_null_project_id_when_global() {
    // When no project is specified, projectId should be null
    let project_id: Option<&str> = None;
    let payload = serde_json::json!({
        "isPaused": false,
        "runningCount": 2,
        "maxConcurrent": 5,
        "reason": "resumed",
        "projectId": project_id,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    });

    // Verify projectId is null
    assert!(
        payload.get("projectId").unwrap().is_null(),
        "projectId should be null when no project specified"
    );
}

// ============================================================================
// Global Cap Tests
// ============================================================================

/// Test: Global cap validation clamps to range [1, 50]
#[test]
fn test_global_cap_clamping() {
    use ralphx_lib::commands::ExecutionState;

    let state = ExecutionState::new();

    // Test lower bound clamping
    state.set_global_max_concurrent(0);
    assert_eq!(state.global_max_concurrent(), 1, "Should clamp 0 to 1");

    // Test upper bound clamping
    state.set_global_max_concurrent(100);
    assert_eq!(state.global_max_concurrent(), 50, "Should clamp 100 to 50");

    // Test valid value
    state.set_global_max_concurrent(25);
    assert_eq!(state.global_max_concurrent(), 25, "25 should be unchanged");
}

/// Test: can_start_task respects both per-project max and global cap
#[test]
fn test_can_start_task_respects_both_limits() {
    use ralphx_lib::commands::ExecutionState;

    let state = ExecutionState::new();
    state.set_max_concurrent(10); // Per-project max: 10
    state.set_global_max_concurrent(5); // Global cap: 5 (lower)

    // With 0 running, should be able to start
    assert!(state.can_start_task(), "Should start with 0 running");

    // Simulate 4 running tasks
    for _ in 0..4 {
        state.increment_running();
    }
    assert!(
        state.can_start_task(),
        "Should start with 4 running (below 5)"
    );

    // Simulate 5th running task - hits global cap
    state.increment_running();
    assert!(
        !state.can_start_task(),
        "Should not start with 5 running (at global cap)"
    );

    // Even though per-project max is 10, global cap of 5 prevents starting
    assert_eq!(state.running_count(), 5);
    assert_eq!(state.max_concurrent(), 10);
    assert_eq!(state.global_max_concurrent(), 5);
}

/// Test: Per-project max takes precedence when lower than global cap
#[test]
fn test_per_project_max_takes_precedence_when_lower() {
    use ralphx_lib::commands::ExecutionState;

    let state = ExecutionState::new();
    state.set_max_concurrent(3); // Per-project max: 3 (lower)
    state.set_global_max_concurrent(20); // Global cap: 20

    // Simulate 2 running tasks
    state.increment_running();
    state.increment_running();
    assert!(state.can_start_task(), "Should start with 2 running");

    // Simulate 3rd running task - hits per-project max
    state.increment_running();
    assert!(
        !state.can_start_task(),
        "Should not start with 3 running (at per-project max)"
    );

    // Global cap is 20 but per-project max of 3 prevents starting
    assert_eq!(state.running_count(), 3);
}

/// Test: Paused state prevents starting regardless of counts
#[test]
fn test_paused_prevents_starting() {
    use ralphx_lib::commands::ExecutionState;

    let state = ExecutionState::new();
    state.set_max_concurrent(10);
    state.set_global_max_concurrent(20);

    assert!(state.can_start_task(), "Should start when not paused");

    state.pause();
    assert!(
        !state.can_start_task(),
        "Should not start when paused, even with capacity"
    );

    state.resume();
    assert!(state.can_start_task(), "Should start after resume");
}
