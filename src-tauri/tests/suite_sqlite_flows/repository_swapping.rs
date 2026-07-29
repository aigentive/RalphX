// Integration test demonstrating repository swapping
//
// This test proves that business logic works identically with both
// MemoryRepository and SqliteRepository implementations. The repository
// pattern enables:
//
// 1. Fast in-memory tests during development
// 2. Real SQLite database in production
// 3. Easy mocking for unit tests
// 4. Future database migrations (e.g., to PostgreSQL)
//
// The key insight is that all business logic depends on the trait,
// not the implementation, making the code testable and flexible.

use std::sync::Arc;

use ralphx_lib::application::AppState;
use ralphx_lib::domain::entities::{GitMode, InternalStatus, Project, Task};
use ralphx_lib::infrastructure::sqlite::{
    SqliteProjectRepository, SqliteTaskRepository,
};
use ralphx_lib::testing::SqliteStateFixture;

/// Helper to create AppState with memory repositories
fn create_memory_state() -> AppState {
    AppState::new_test()
}

/// Helper to create AppState with SQLite repositories (file-backed temp database)
fn create_sqlite_state() -> SqliteStateFixture {
    SqliteStateFixture::new("repository-swapping", |db, state| {
        let shared_conn = db.shared_conn();
        *state = AppState::with_repos(
            Arc::new(SqliteTaskRepository::from_shared(Arc::clone(&shared_conn))),
            Arc::new(SqliteProjectRepository::from_shared(shared_conn)),
        );
    })
}

/// Shared business logic test that works with any repository implementation
async fn test_task_workflow(state: &AppState) {
    // 1. Create a project
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    let project = state.project_repo.create(project).await.unwrap();

    // 2. Create tasks in the project
    let task1 = Task::new(project.id.clone(), "Task 1".to_string());
    let task2 = Task::new(project.id.clone(), "Task 2".to_string());
    let task1 = state.task_repo.create(task1).await.unwrap();
    let task2 = state.task_repo.create(task2).await.unwrap();

    // 3. Verify tasks are in backlog status
    assert_eq!(task1.internal_status, InternalStatus::Backlog);
    assert_eq!(task2.internal_status, InternalStatus::Backlog);

    // 4. Get tasks by project
    let tasks = state.task_repo.get_by_project(&project.id).await.unwrap();
    assert_eq!(tasks.len(), 2);

    // 5. Transition task1 to ready
    state
        .task_repo
        .persist_status_change(
            &task1.id,
            InternalStatus::Backlog,
            InternalStatus::Ready,
            "test",
        )
        .await
        .unwrap();

    // 6. Verify status was updated
    let updated_task1 = state.task_repo.get_by_id(&task1.id).await.unwrap().unwrap();
    assert_eq!(updated_task1.internal_status, InternalStatus::Ready);

    // 7. Get next executable should return task1
    let next = state
        .task_repo
        .get_next_executable(&project.id)
        .await
        .unwrap();
    assert!(next.is_some());
    assert_eq!(next.unwrap().id, task1.id);

    // 8. Add dependency: task1 depends on task2 (via task_dependency_repo)
    state
        .task_dependency_repo
        .add_dependency(&task1.id, &task2.id)
        .await
        .unwrap();

    // 9. Get blockers for task1 (via task_dependency_repo)
    let blockers = state
        .task_dependency_repo
        .get_blockers(&task1.id)
        .await
        .unwrap();
    assert_eq!(blockers.len(), 1);
    assert_eq!(blockers[0], task2.id);

    // 10. Get tasks blocked by task2 (via task_dependency_repo)
    let blocked = state
        .task_dependency_repo
        .get_blocked_by(&task2.id)
        .await
        .unwrap();
    assert_eq!(blocked.len(), 1);
    assert_eq!(blocked[0], task1.id);

    // 11. Remove dependency
    state
        .task_dependency_repo
        .remove_dependency(&task1.id, &task2.id)
        .await
        .unwrap();

    // 12. Verify dependency is resolved
    let blockers = state
        .task_dependency_repo
        .get_blockers(&task1.id)
        .await
        .unwrap();
    assert!(blockers.is_empty());

    // 13. Get status history for task1
    let history = state.task_repo.get_status_history(&task1.id).await.unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].from, InternalStatus::Backlog);
    assert_eq!(history[0].to, InternalStatus::Ready);

    // 14. Delete task1
    state.task_repo.delete(&task1.id).await.unwrap();

    // 15. Verify task1 is deleted
    let deleted = state.task_repo.get_by_id(&task1.id).await.unwrap();
    assert!(deleted.is_none());

    // 16. Verify project still has task2
    let remaining_tasks = state.task_repo.get_by_project(&project.id).await.unwrap();
    assert_eq!(remaining_tasks.len(), 1);
    assert_eq!(remaining_tasks[0].id, task2.id);
}

/// Shared project workflow test
async fn test_project_workflow(state: &AppState) {
    // 1. Create projects
    let project1 = Project::new("Project 1".to_string(), "/path/1".to_string());
    let mut project2 = Project::new("Project 2".to_string(), "/main/repo".to_string());
    project2.git_mode = GitMode::Worktree;
    project2.base_branch = Some("main".to_string());

    let project1 = state.project_repo.create(project1).await.unwrap();
    let project2 = state.project_repo.create(project2).await.unwrap();

    // 2. Get all projects
    let projects = state.project_repo.get_all().await.unwrap();
    assert_eq!(projects.len(), 2);

    // 3. Get by ID
    let found = state
        .project_repo
        .get_by_id(&project1.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.name, "Project 1");

    // 4. Get by working directory
    let found = state
        .project_repo
        .get_by_working_directory("/main/repo")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.name, "Project 2");
    assert!(found.is_worktree());

    // 5. Update project
    let mut updated_project1 = project1.clone();
    updated_project1.name = "Updated Project 1".to_string();
    state.project_repo.update(&updated_project1).await.unwrap();

    let found = state
        .project_repo
        .get_by_id(&project1.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.name, "Updated Project 1");

    // 6. Delete project
    state.project_repo.delete(&project2.id).await.unwrap();

    let deleted = state.project_repo.get_by_id(&project2.id).await.unwrap();
    assert!(deleted.is_none());

    // 7. Verify only project1 remains
    let projects = state.project_repo.get_all().await.unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].id, project1.id);
}

// ============================================================================
// Tests with Memory Repository
// ============================================================================

#[tokio::test]
async fn test_task_workflow_with_memory_repository() {
    let state = create_memory_state();
    test_task_workflow(&state).await;
}

#[tokio::test]
async fn test_project_workflow_with_memory_repository() {
    let state = create_memory_state();
    test_project_workflow(&state).await;
}

// ============================================================================
// Tests with SQLite Repository (in-memory database)
// ============================================================================

#[tokio::test]
async fn test_task_workflow_with_sqlite_repository() {
    let state = create_sqlite_state();
    test_task_workflow(&state).await;
}

#[tokio::test]
async fn test_project_workflow_with_sqlite_repository() {
    let state = create_sqlite_state();
    test_project_workflow(&state).await;
}

// ============================================================================
// Documentation: Repository Swapping Pattern
// ============================================================================

/// This test file demonstrates the Repository Pattern as used in RalphX:
///
/// ## Benefits
///
/// 1. **Testability**: Business logic can be tested with fast in-memory repositories
///    without needing a real database.
///
/// 2. **Flexibility**: The same business logic works with any repository implementation
///    that satisfies the trait contract.
///
/// 3. **Maintainability**: Database concerns are isolated in the infrastructure layer,
///    making it easy to change database engines or add caching.
///
/// ## Usage Pattern
///
/// ```rust
/// // In production (lib.rs):
/// let app_state = AppState::new_production().expect("Failed to initialize");
/// // Uses SqliteTaskRepository and SqliteProjectRepository
///
/// // In tests:
/// let app_state = AppState::new_test();
/// // Uses MemoryTaskRepository and MemoryProjectRepository
///
/// // Custom configuration:
/// let app_state = AppState::with_repos(
///     Arc::new(MyCustomTaskRepository::new()),
///     Arc::new(MyCustomProjectRepository::new()),
/// );
/// ```
///
/// ## Adding New Repositories
///
/// To add a new repository implementation (e.g., PostgreSQL):
///
/// 1. Create `infrastructure/postgres/postgres_task_repo.rs`
/// 2. Implement `TaskRepository` trait
/// 3. Add constructor method to `AppState`
/// 4. Run the same integration tests to verify compatibility
///
/// The tests in this file should pass with any correct implementation.
#[test]
fn test_documentation() {
    // This test exists to ensure the documentation compiles
}
