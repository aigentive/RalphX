use super::*;
use crate::domain::entities::{Project, ProjectId};
use crate::testing::SqliteTestDb;

fn setup_test_db() -> SqliteTestDb {
    SqliteTestDb::new("sqlite-task-dependency-repo")
}

fn create_test_project(db: &SqliteTestDb, id: &ProjectId, name: &str, path: &str) {
    let mut project = Project::new(name.to_string(), path.to_string());
    project.id = id.clone();
    db.insert_project(project);
}

fn create_test_task(db: &SqliteTestDb, project_id: &ProjectId, title: &str) -> TaskId {
    db.seed_task(project_id.clone(), title).id
}

// ==================== ADD DEPENDENCY TESTS ====================

#[tokio::test]
async fn test_add_dependency_creates_record() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let task_a = create_test_task(&db, &project_id, "Task A");
    let task_b = create_test_task(&db, &project_id, "Task B");

    let repo = SqliteTaskDependencyRepository::new(db.new_connection());

    let result = repo.add_dependency(&task_a, &task_b).await;

    assert!(result.is_ok());

    // Verify dependency was created (A depends on B, so B is a blocker of A)
    let blockers = repo.get_blockers(&task_a).await.unwrap();
    assert_eq!(blockers.len(), 1);
    assert_eq!(blockers[0], task_b);
}

#[tokio::test]
async fn test_add_dependency_duplicate_is_ignored() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let task_a = create_test_task(&db, &project_id, "Task A");
    let task_b = create_test_task(&db, &project_id, "Task B");

    let repo = SqliteTaskDependencyRepository::new(db.new_connection());

    // Add same dependency twice
    repo.add_dependency(&task_a, &task_b).await.unwrap();
    let result = repo.add_dependency(&task_a, &task_b).await;

    assert!(result.is_ok());

    // Should only have one dependency
    let blockers = repo.get_blockers(&task_a).await.unwrap();
    assert_eq!(blockers.len(), 1);
}

#[tokio::test]
async fn test_add_multiple_dependencies() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let task_a = create_test_task(&db, &project_id, "Task A");
    let task_b = create_test_task(&db, &project_id, "Task B");
    let task_c = create_test_task(&db, &project_id, "Task C");

    let repo = SqliteTaskDependencyRepository::new(db.new_connection());

    // A depends on B and C
    repo.add_dependency(&task_a, &task_b).await.unwrap();
    repo.add_dependency(&task_a, &task_c).await.unwrap();

    let blockers = repo.get_blockers(&task_a).await.unwrap();
    assert_eq!(blockers.len(), 2);
    assert!(blockers.contains(&task_b));
    assert!(blockers.contains(&task_c));
}

// ==================== REMOVE DEPENDENCY TESTS ====================

#[tokio::test]
async fn test_remove_dependency_deletes_record() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let task_a = create_test_task(&db, &project_id, "Task A");
    let task_b = create_test_task(&db, &project_id, "Task B");

    let repo = SqliteTaskDependencyRepository::new(db.new_connection());

    repo.add_dependency(&task_a, &task_b).await.unwrap();
    let result = repo.remove_dependency(&task_a, &task_b).await;

    assert!(result.is_ok());

    let blockers = repo.get_blockers(&task_a).await.unwrap();
    assert!(blockers.is_empty());
}

#[tokio::test]
async fn test_remove_nonexistent_dependency_succeeds() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let task_a = create_test_task(&db, &project_id, "Task A");
    let task_b = create_test_task(&db, &project_id, "Task B");

    let repo = SqliteTaskDependencyRepository::new(db.new_connection());

    // Should not error
    let result = repo.remove_dependency(&task_a, &task_b).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_remove_only_specified_dependency() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let task_a = create_test_task(&db, &project_id, "Task A");
    let task_b = create_test_task(&db, &project_id, "Task B");
    let task_c = create_test_task(&db, &project_id, "Task C");

    let repo = SqliteTaskDependencyRepository::new(db.new_connection());

    repo.add_dependency(&task_a, &task_b).await.unwrap();
    repo.add_dependency(&task_a, &task_c).await.unwrap();

    // Remove only B dependency
    repo.remove_dependency(&task_a, &task_b).await.unwrap();

    let blockers = repo.get_blockers(&task_a).await.unwrap();
    assert_eq!(blockers.len(), 1);
    assert!(blockers.contains(&task_c));
}

// ==================== GET BLOCKERS TESTS ====================

#[tokio::test]
async fn test_get_blockers_empty() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let task = create_test_task(&db, &project_id, "Task");

    let repo = SqliteTaskDependencyRepository::new(db.new_connection());

    let blockers = repo.get_blockers(&task).await.unwrap();
    assert!(blockers.is_empty());
}

#[tokio::test]
async fn test_get_blockers_returns_correct_direction() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let task_a = create_test_task(&db, &project_id, "Task A");
    let task_b = create_test_task(&db, &project_id, "Task B");

    let repo = SqliteTaskDependencyRepository::new(db.new_connection());

    // A depends on B
    repo.add_dependency(&task_a, &task_b).await.unwrap();

    // A's blockers should include B
    let a_blockers = repo.get_blockers(&task_a).await.unwrap();
    assert_eq!(a_blockers.len(), 1);
    assert!(a_blockers.contains(&task_b));

    // B should have no blockers
    let b_blockers = repo.get_blockers(&task_b).await.unwrap();
    assert!(b_blockers.is_empty());
}

// ==================== GET BLOCKED BY TESTS ====================

#[tokio::test]
async fn test_get_blocked_by_empty() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let task = create_test_task(&db, &project_id, "Task");

    let repo = SqliteTaskDependencyRepository::new(db.new_connection());

    let blocked_by = repo.get_blocked_by(&task).await.unwrap();
    assert!(blocked_by.is_empty());
}

#[tokio::test]
async fn test_get_blocked_by_returns_correct_direction() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let task_a = create_test_task(&db, &project_id, "Task A");
    let task_b = create_test_task(&db, &project_id, "Task B");

    let repo = SqliteTaskDependencyRepository::new(db.new_connection());

    // A depends on B (B blocks A)
    repo.add_dependency(&task_a, &task_b).await.unwrap();

    // B's blocked_by should include A
    let b_blocked_by = repo.get_blocked_by(&task_b).await.unwrap();
    assert_eq!(b_blocked_by.len(), 1);
    assert!(b_blocked_by.contains(&task_a));

    // A should have no tasks blocked by it
    let a_blocked_by = repo.get_blocked_by(&task_a).await.unwrap();
    assert!(a_blocked_by.is_empty());
}

#[tokio::test]
async fn test_get_blocked_by_multiple() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let task_a = create_test_task(&db, &project_id, "Task A");
    let task_b = create_test_task(&db, &project_id, "Task B");
    let task_c = create_test_task(&db, &project_id, "Task C");

    let repo = SqliteTaskDependencyRepository::new(db.new_connection());

    // A and B both depend on C
    repo.add_dependency(&task_a, &task_c).await.unwrap();
    repo.add_dependency(&task_b, &task_c).await.unwrap();

    let blocked_by = repo.get_blocked_by(&task_c).await.unwrap();
    assert_eq!(blocked_by.len(), 2);
    assert!(blocked_by.contains(&task_a));
    assert!(blocked_by.contains(&task_b));
}

// ==================== HAS CIRCULAR DEPENDENCY TESTS ====================

#[tokio::test]
async fn test_has_circular_dependency_self() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let task = create_test_task(&db, &project_id, "Task");

    let repo = SqliteTaskDependencyRepository::new(db.new_connection());

    let result = repo.has_circular_dependency(&task, &task).await;
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[tokio::test]
async fn test_has_circular_dependency_direct_cycle() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let task_a = create_test_task(&db, &project_id, "Task A");
    let task_b = create_test_task(&db, &project_id, "Task B");

    let repo = SqliteTaskDependencyRepository::new(db.new_connection());

    // B depends on A
    repo.add_dependency(&task_b, &task_a).await.unwrap();

    // Would adding A -> B create a cycle? Yes (A -> B -> A)
    let result = repo.has_circular_dependency(&task_a, &task_b).await;
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[tokio::test]
async fn test_has_circular_dependency_indirect_cycle() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let task_a = create_test_task(&db, &project_id, "Task A");
    let task_b = create_test_task(&db, &project_id, "Task B");
    let task_c = create_test_task(&db, &project_id, "Task C");

    let repo = SqliteTaskDependencyRepository::new(db.new_connection());

    // B -> C, C -> A (existing chain: B depends on C, C depends on A)
    repo.add_dependency(&task_b, &task_c).await.unwrap();
    repo.add_dependency(&task_c, &task_a).await.unwrap();

    // Would adding A -> B create a cycle? Yes (A -> B -> C -> A)
    let result = repo.has_circular_dependency(&task_a, &task_b).await;
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[tokio::test]
async fn test_has_circular_dependency_no_cycle() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let task_a = create_test_task(&db, &project_id, "Task A");
    let task_b = create_test_task(&db, &project_id, "Task B");
    let task_c = create_test_task(&db, &project_id, "Task C");

    let repo = SqliteTaskDependencyRepository::new(db.new_connection());

    // A -> B (existing)
    repo.add_dependency(&task_a, &task_b).await.unwrap();

    // Would adding B -> C create a cycle? No
    let result = repo.has_circular_dependency(&task_b, &task_c).await;
    assert!(result.is_ok());
    assert!(!result.unwrap());
}

#[tokio::test]
async fn test_has_circular_dependency_empty_graph() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let task_a = create_test_task(&db, &project_id, "Task A");
    let task_b = create_test_task(&db, &project_id, "Task B");

    let repo = SqliteTaskDependencyRepository::new(db.new_connection());

    // No existing dependencies, would A -> B create a cycle? No
    let result = repo.has_circular_dependency(&task_a, &task_b).await;
    assert!(result.is_ok());
    assert!(!result.unwrap());
}

#[tokio::test]
async fn test_has_circular_dependency_long_chain() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let task_a = create_test_task(&db, &project_id, "Task A");
    let task_b = create_test_task(&db, &project_id, "Task B");
    let task_c = create_test_task(&db, &project_id, "Task C");
    let task_d = create_test_task(&db, &project_id, "Task D");
    let task_e = create_test_task(&db, &project_id, "Task E");

    let repo = SqliteTaskDependencyRepository::new(db.new_connection());

    // Chain: B -> C -> D -> E -> A
    repo.add_dependency(&task_b, &task_c).await.unwrap();
    repo.add_dependency(&task_c, &task_d).await.unwrap();
    repo.add_dependency(&task_d, &task_e).await.unwrap();
    repo.add_dependency(&task_e, &task_a).await.unwrap();

    // Would adding A -> B create a cycle? Yes (A -> B -> C -> D -> E -> A)
    let result = repo.has_circular_dependency(&task_a, &task_b).await;
    assert!(result.is_ok());
    assert!(result.unwrap());
}

// ==================== CLEAR DEPENDENCIES TESTS ====================

#[tokio::test]
async fn test_clear_dependencies_removes_outgoing() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let task_a = create_test_task(&db, &project_id, "Task A");
    let task_b = create_test_task(&db, &project_id, "Task B");
    let task_c = create_test_task(&db, &project_id, "Task C");

    let repo = SqliteTaskDependencyRepository::new(db.new_connection());

    // A -> B, A -> C
    repo.add_dependency(&task_a, &task_b).await.unwrap();
    repo.add_dependency(&task_a, &task_c).await.unwrap();

    repo.clear_dependencies(&task_a).await.unwrap();

    let blockers = repo.get_blockers(&task_a).await.unwrap();
    assert!(blockers.is_empty());
}

#[tokio::test]
async fn test_clear_dependencies_removes_incoming() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let task_a = create_test_task(&db, &project_id, "Task A");
    let task_b = create_test_task(&db, &project_id, "Task B");
    let task_c = create_test_task(&db, &project_id, "Task C");

    let repo = SqliteTaskDependencyRepository::new(db.new_connection());

    // B -> A, C -> A
    repo.add_dependency(&task_b, &task_a).await.unwrap();
    repo.add_dependency(&task_c, &task_a).await.unwrap();

    repo.clear_dependencies(&task_a).await.unwrap();

    // A should have no tasks blocked by it anymore
    let blocked_by = repo.get_blocked_by(&task_a).await.unwrap();
    assert!(blocked_by.is_empty());

    // B and C should have no blockers anymore
    let b_blockers = repo.get_blockers(&task_b).await.unwrap();
    assert!(b_blockers.is_empty());
    let c_blockers = repo.get_blockers(&task_c).await.unwrap();
    assert!(c_blockers.is_empty());
}

#[tokio::test]
async fn test_clear_dependencies_removes_both_directions() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let task_a = create_test_task(&db, &project_id, "Task A");
    let task_b = create_test_task(&db, &project_id, "Task B");
    let task_c = create_test_task(&db, &project_id, "Task C");

    let repo = SqliteTaskDependencyRepository::new(db.new_connection());

    // A -> B (A depends on B), C -> A (C depends on A)
    repo.add_dependency(&task_a, &task_b).await.unwrap();
    repo.add_dependency(&task_c, &task_a).await.unwrap();

    repo.clear_dependencies(&task_a).await.unwrap();

    // A should have no blockers
    let a_blockers = repo.get_blockers(&task_a).await.unwrap();
    assert!(a_blockers.is_empty());

    // A should have no tasks blocked by it
    let a_blocked_by = repo.get_blocked_by(&task_a).await.unwrap();
    assert!(a_blocked_by.is_empty());

    // C should have no blockers (was depending on A)
    let c_blockers = repo.get_blockers(&task_c).await.unwrap();
    assert!(c_blockers.is_empty());
}

// ==================== COUNT BLOCKERS TESTS ====================

#[tokio::test]
async fn test_count_blockers_zero() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let task = create_test_task(&db, &project_id, "Task");

    let repo = SqliteTaskDependencyRepository::new(db.new_connection());

    let count = repo.count_blockers(&task).await.unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn test_count_blockers_multiple() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let task_a = create_test_task(&db, &project_id, "Task A");
    let task_b = create_test_task(&db, &project_id, "Task B");
    let task_c = create_test_task(&db, &project_id, "Task C");

    let repo = SqliteTaskDependencyRepository::new(db.new_connection());

    // A depends on B and C
    repo.add_dependency(&task_a, &task_b).await.unwrap();
    repo.add_dependency(&task_a, &task_c).await.unwrap();

    let count = repo.count_blockers(&task_a).await.unwrap();
    assert_eq!(count, 2);
}

// ==================== COUNT BLOCKED BY TESTS ====================

#[tokio::test]
async fn test_count_blocked_by_zero() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let task = create_test_task(&db, &project_id, "Task");

    let repo = SqliteTaskDependencyRepository::new(db.new_connection());

    let count = repo.count_blocked_by(&task).await.unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn test_count_blocked_by_multiple() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let task_a = create_test_task(&db, &project_id, "Task A");
    let task_b = create_test_task(&db, &project_id, "Task B");
    let task_c = create_test_task(&db, &project_id, "Task C");

    let repo = SqliteTaskDependencyRepository::new(db.new_connection());

    // B and C depend on A
    repo.add_dependency(&task_b, &task_a).await.unwrap();
    repo.add_dependency(&task_c, &task_a).await.unwrap();

    let count = repo.count_blocked_by(&task_a).await.unwrap();
    assert_eq!(count, 2);
}

// ==================== HAS DEPENDENCY TESTS ====================

#[tokio::test]
async fn test_has_dependency_true() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let task_a = create_test_task(&db, &project_id, "Task A");
    let task_b = create_test_task(&db, &project_id, "Task B");

    let repo = SqliteTaskDependencyRepository::new(db.new_connection());

    repo.add_dependency(&task_a, &task_b).await.unwrap();

    let has_dep = repo.has_dependency(&task_a, &task_b).await.unwrap();
    assert!(has_dep);
}

#[tokio::test]
async fn test_has_dependency_false() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let task_a = create_test_task(&db, &project_id, "Task A");
    let task_b = create_test_task(&db, &project_id, "Task B");

    let repo = SqliteTaskDependencyRepository::new(db.new_connection());

    let has_dep = repo.has_dependency(&task_a, &task_b).await.unwrap();
    assert!(!has_dep);
}

#[tokio::test]
async fn test_has_dependency_direction_matters() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let task_a = create_test_task(&db, &project_id, "Task A");
    let task_b = create_test_task(&db, &project_id, "Task B");

    let repo = SqliteTaskDependencyRepository::new(db.new_connection());

    // A depends on B
    repo.add_dependency(&task_a, &task_b).await.unwrap();

    // A -> B exists
    assert!(repo.has_dependency(&task_a, &task_b).await.unwrap());

    // B -> A does NOT exist
    assert!(!repo.has_dependency(&task_b, &task_a).await.unwrap());
}

// ==================== SHARED CONNECTION TESTS ====================

#[tokio::test]
async fn test_from_shared_works_correctly() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let task_a = create_test_task(&db, &project_id, "Task A");
    let task_b = create_test_task(&db, &project_id, "Task B");

    let repo = SqliteTaskDependencyRepository::from_shared(db.shared_conn());

    repo.add_dependency(&task_a, &task_b).await.unwrap();

    let blockers = repo.get_blockers(&task_a).await.unwrap();
    assert_eq!(blockers.len(), 1);
}

// ==================== CASCADE DELETE TESTS ====================

#[tokio::test]
async fn test_cascade_deletes_when_task_deleted() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let task_a = create_test_task(&db, &project_id, "Task A");
    let task_b = create_test_task(&db, &project_id, "Task B");

    // Add dependency
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO task_dependencies (id, task_id, depends_on_task_id)
             VALUES ('dep-1', ?1, ?2)",
            rusqlite::params![task_a.as_str(), task_b.as_str()],
        )
        .unwrap();
    });

    let count: i32 = db
        .with_connection(|conn| {
            conn.query_row(
            "SELECT COUNT(*) FROM task_dependencies WHERE task_id = ?1",
            [task_a.as_str()],
            |row| row.get(0),
        )
        })
        .unwrap();
    assert_eq!(count, 1);

    db.with_connection(|conn| {
        conn.execute("DELETE FROM tasks WHERE id = ?1", [task_a.as_str()])
            .unwrap();
    });

    let count_after: i32 = db
        .with_connection(|conn| {
            conn.query_row(
            "SELECT COUNT(*) FROM task_dependencies WHERE task_id = ?1",
            [task_a.as_str()],
            |row| row.get(0),
        )
        })
        .unwrap();
    assert_eq!(count_after, 0);
}

#[tokio::test]
async fn test_cascade_deletes_when_depends_on_task_deleted() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let task_a = create_test_task(&db, &project_id, "Task A");
    let task_b = create_test_task(&db, &project_id, "Task B");

    // A depends on B
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO task_dependencies (id, task_id, depends_on_task_id)
             VALUES ('dep-1', ?1, ?2)",
            rusqlite::params![task_a.as_str(), task_b.as_str()],
        )
        .unwrap();
    });

    db.with_connection(|conn| {
        conn.execute("DELETE FROM tasks WHERE id = ?1", [task_b.as_str()])
            .unwrap();
    });

    let count_after: i32 = db
        .with_connection(|conn| {
            conn.query_row(
            "SELECT COUNT(*) FROM task_dependencies WHERE depends_on_task_id = ?1",
            [task_b.as_str()],
            |row| row.get(0),
        )
        })
        .unwrap();
    assert_eq!(count_after, 0);
}

// ==================== CHECK CONSTRAINT TESTS ====================

#[tokio::test]
async fn test_self_dependency_check_constraint() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");
    let task = create_test_task(&db, &project_id, "Task");

    // Direct insert should fail due to CHECK constraint
    let result = db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO task_dependencies (id, task_id, depends_on_task_id)
             VALUES ('dep-1', ?1, ?1)",
            [task.as_str()],
        )
    });

    assert!(result.is_err());
}

// ==================== COMPLEX GRAPH TESTS ====================

#[tokio::test]
async fn test_diamond_dependency_no_cycle() {
    let db = setup_test_db();
    let project_id = ProjectId::new();
    create_test_project(&db, &project_id, "Test", "/test");

    // Diamond pattern:
    //     A
    //    / \
    //   B   C
    //    \ /
    //     D
    let task_a = create_test_task(&db, &project_id, "Task A");
    let task_b = create_test_task(&db, &project_id, "Task B");
    let task_c = create_test_task(&db, &project_id, "Task C");
    let task_d = create_test_task(&db, &project_id, "Task D");

    let repo = SqliteTaskDependencyRepository::new(db.new_connection());

    // A depends on nothing
    // B depends on A, C depends on A
    // D depends on B and C
    repo.add_dependency(&task_b, &task_a).await.unwrap();
    repo.add_dependency(&task_c, &task_a).await.unwrap();
    repo.add_dependency(&task_d, &task_b).await.unwrap();
    repo.add_dependency(&task_d, &task_c).await.unwrap();

    // This is valid, no cycle
    // Would adding a new dependency E -> D create a cycle? No
    let task_e = TaskId::new();
    let has_cycle = repo
        .has_circular_dependency(&task_e, &task_d)
        .await
        .unwrap();
    assert!(!has_cycle);

    // D has 2 blockers
    assert_eq!(repo.count_blockers(&task_d).await.unwrap(), 2);

    // A has 2 tasks blocked by it (B and C)
    assert_eq!(repo.count_blocked_by(&task_a).await.unwrap(), 2);
}
