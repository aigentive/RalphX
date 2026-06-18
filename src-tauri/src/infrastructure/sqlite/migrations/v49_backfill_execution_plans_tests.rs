//! Tests for migration v49: backfill execution_plans for existing accepted/archived sessions

use rusqlite::Connection;

use super::v49_backfill_execution_plans;

/// Set up a minimal in-memory database for v49 testing.
/// Includes all tables involved in the backfill.
fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");

    conn.execute_batch(
        "PRAGMA foreign_keys = OFF;

        CREATE TABLE projects (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL
        );

        CREATE TABLE ideation_sessions (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            title TEXT,
            status TEXT NOT NULL DEFAULT 'active',
            created_at TEXT NOT NULL DEFAULT '2026-01-01T00:00:00+00:00',
            updated_at TEXT NOT NULL DEFAULT '2026-01-01T00:00:00+00:00',
            converted_at TEXT
        );

        CREATE TABLE execution_plans (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'active',
            created_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
        );

        CREATE TABLE tasks (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            title TEXT NOT NULL,
            ideation_session_id TEXT,
            execution_plan_id TEXT
        );

        CREATE TABLE plan_branches (
            id TEXT PRIMARY KEY,
            plan_artifact_id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            project_id TEXT NOT NULL,
            branch_name TEXT NOT NULL,
            source_branch TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'active',
            execution_plan_id TEXT
        );

        CREATE TABLE project_active_plan (
            project_id TEXT PRIMARY KEY,
            ideation_session_id TEXT NOT NULL,
            execution_plan_id TEXT,
            updated_at TEXT NOT NULL DEFAULT '2026-01-01T00:00:00+00:00'
        );",
    )
    .expect("Failed to create test schema");

    conn
}

// ---------------------------------------------------------------------------
// Core backfill tests
// ---------------------------------------------------------------------------

#[test]
fn test_creates_execution_plan_for_accepted_session() {
    let conn = setup_test_db();

    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id, status) VALUES ('s1', 'p1', 'accepted')",
        [],
    )
    .unwrap();

    v49_backfill_execution_plans::migrate(&conn).unwrap();

    let count: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM execution_plans WHERE session_id = 's1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        count, 1,
        "should create one execution_plan for accepted session"
    );

    let status: String = conn
        .query_row(
            "SELECT status FROM execution_plans WHERE session_id = 's1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "active");
}

#[test]
fn test_creates_execution_plan_for_archived_session() {
    let conn = setup_test_db();

    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id, status) VALUES ('s1', 'p1', 'archived')",
        [],
    )
    .unwrap();

    v49_backfill_execution_plans::migrate(&conn).unwrap();

    let count: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM execution_plans WHERE session_id = 's1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        count, 1,
        "should create one execution_plan for archived session"
    );
}

#[test]
fn test_skips_active_session() {
    let conn = setup_test_db();

    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id, status) VALUES ('s1', 'p1', 'active')",
        [],
    )
    .unwrap();

    v49_backfill_execution_plans::migrate(&conn).unwrap();

    let count: i32 = conn
        .query_row("SELECT COUNT(*) FROM execution_plans", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        count, 0,
        "should not create execution_plan for active session"
    );
}

// ---------------------------------------------------------------------------
// Task and plan_branch linking tests
// ---------------------------------------------------------------------------

#[test]
fn test_links_tasks_to_execution_plan() {
    let conn = setup_test_db();

    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id, status) VALUES ('s1', 'p1', 'accepted')",
        [],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO tasks (id, project_id, title, ideation_session_id, execution_plan_id)
         VALUES ('t1', 'p1', 'Task 1', 's1', NULL)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tasks (id, project_id, title, ideation_session_id, execution_plan_id)
         VALUES ('t2', 'p1', 'Task 2', 's1', NULL)",
        [],
    )
    .unwrap();

    v49_backfill_execution_plans::migrate(&conn).unwrap();

    // Get the execution_plan created for s1
    let ep_id: String = conn
        .query_row(
            "SELECT id FROM execution_plans WHERE session_id = 's1'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    let t1_ep: Option<String> = conn
        .query_row(
            "SELECT execution_plan_id FROM tasks WHERE id = 't1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        t1_ep,
        Some(ep_id.clone()),
        "task t1 should be linked to execution_plan"
    );

    let t2_ep: Option<String> = conn
        .query_row(
            "SELECT execution_plan_id FROM tasks WHERE id = 't2'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        t2_ep,
        Some(ep_id),
        "task t2 should be linked to execution_plan"
    );
}

#[test]
fn test_links_plan_branches_to_execution_plan() {
    let conn = setup_test_db();

    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id, status) VALUES ('s1', 'p1', 'accepted')",
        [],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO plan_branches (id, plan_artifact_id, session_id, project_id, branch_name, source_branch, execution_plan_id)
         VALUES ('pb1', 'a1', 's1', 'p1', 'ralphx/test/plan-abc', 'main', NULL)",
        [],
    )
    .unwrap();

    v49_backfill_execution_plans::migrate(&conn).unwrap();

    let ep_id: String = conn
        .query_row(
            "SELECT id FROM execution_plans WHERE session_id = 's1'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    let pb_ep: Option<String> = conn
        .query_row(
            "SELECT execution_plan_id FROM plan_branches WHERE id = 'pb1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        pb_ep,
        Some(ep_id),
        "plan_branch should be linked to execution_plan"
    );
}

#[test]
fn test_does_not_overwrite_existing_task_execution_plan_id() {
    let conn = setup_test_db();

    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id, status) VALUES ('s1', 'p1', 'accepted')",
        [],
    )
    .unwrap();

    // Pre-existing execution_plan (created by new flow, post-v46)
    conn.execute(
        "INSERT INTO execution_plans (id, session_id, status) VALUES ('ep-existing', 's1', 'active')",
        [],
    )
    .unwrap();

    // Task already linked to the existing execution_plan
    conn.execute(
        "INSERT INTO tasks (id, project_id, title, ideation_session_id, execution_plan_id)
         VALUES ('t1', 'p1', 'Task 1', 's1', 'ep-existing')",
        [],
    )
    .unwrap();

    // Old task with NULL execution_plan_id should NOT be touched (session already has a plan)
    // Actually - session already has an execution_plan so the backfill won't create a new one
    v49_backfill_execution_plans::migrate(&conn).unwrap();

    // Should still have exactly 1 execution_plan for this session
    let count: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM execution_plans WHERE session_id = 's1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "should not create duplicate execution_plan");

    let t1_ep: Option<String> = conn
        .query_row(
            "SELECT execution_plan_id FROM tasks WHERE id = 't1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        t1_ep,
        Some("ep-existing".to_string()),
        "existing task link should not be overwritten"
    );
}

// ---------------------------------------------------------------------------
// Active plan reference migration
// ---------------------------------------------------------------------------

#[test]
fn test_populates_active_plan_execution_plan_id() {
    let conn = setup_test_db();

    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id, status) VALUES ('s1', 'p1', 'accepted')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO project_active_plan (project_id, ideation_session_id)
         VALUES ('p1', 's1')",
        [],
    )
    .unwrap();

    v49_backfill_execution_plans::migrate(&conn).unwrap();

    let ep_id: String = conn
        .query_row(
            "SELECT id FROM execution_plans WHERE session_id = 's1'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    let active_ep: Option<String> = conn
        .query_row(
            "SELECT execution_plan_id FROM project_active_plan WHERE project_id = 'p1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        active_ep,
        Some(ep_id),
        "project_active_plan.execution_plan_id should be populated"
    );
}

#[test]
fn test_active_plan_execution_plan_id_column_added() {
    let conn = setup_test_db();

    // Run migration on a db that already has the column (from setup_test_db)
    // Should not fail due to add_column_if_not_exists being idempotent
    v49_backfill_execution_plans::migrate(&conn).unwrap();
    v49_backfill_execution_plans::migrate(&conn).unwrap();
}

// ---------------------------------------------------------------------------
// Idempotency test
// ---------------------------------------------------------------------------

#[test]
fn test_migration_is_idempotent() {
    let conn = setup_test_db();

    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id, status) VALUES ('s1', 'p1', 'accepted')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tasks (id, project_id, title, ideation_session_id) VALUES ('t1', 'p1', 'T', 's1')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO plan_branches (id, plan_artifact_id, session_id, project_id, branch_name, source_branch)
         VALUES ('pb1', 'a1', 's1', 'p1', 'ralphx/p/plan-abc', 'main')",
        [],
    )
    .unwrap();

    // Run twice
    v49_backfill_execution_plans::migrate(&conn).unwrap();
    v49_backfill_execution_plans::migrate(&conn).unwrap();

    // Should still have exactly 1 execution_plan
    let count: i32 = conn
        .query_row("SELECT COUNT(*) FROM execution_plans", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        count, 1,
        "idempotent: should not create duplicate execution_plans"
    );
}

// ---------------------------------------------------------------------------
// Multi-session test
// ---------------------------------------------------------------------------

#[test]
fn test_multiple_sessions_get_separate_execution_plans() {
    let conn = setup_test_db();

    for i in 1..=3 {
        conn.execute(
            &format!(
                "INSERT INTO ideation_sessions (id, project_id, status) VALUES ('s{}', 'p1', 'accepted')",
                i
            ),
            [],
        )
        .unwrap();
        conn.execute(
            &format!(
                "INSERT INTO tasks (id, project_id, title, ideation_session_id) VALUES ('t{}', 'p1', 'Task', 's{}')",
                i, i
            ),
            [],
        )
        .unwrap();
    }

    v49_backfill_execution_plans::migrate(&conn).unwrap();

    let ep_count: i32 = conn
        .query_row("SELECT COUNT(*) FROM execution_plans", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        ep_count, 3,
        "each session should get its own execution_plan"
    );

    // All tasks should be linked
    let unlinked: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM tasks WHERE execution_plan_id IS NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        unlinked, 0,
        "all tasks should be linked to an execution_plan"
    );

    // Each execution_plan has a unique id
    let distinct_ep_ids: i32 = conn
        .query_row(
            "SELECT COUNT(DISTINCT id) FROM execution_plans",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        distinct_ep_ids, 3,
        "each execution_plan should have a unique id"
    );
}

// ---------------------------------------------------------------------------
// No-op test
// ---------------------------------------------------------------------------

#[test]
fn test_no_sessions_to_backfill() {
    let conn = setup_test_db();

    // Only active sessions - nothing to backfill
    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id, status) VALUES ('s1', 'p1', 'active')",
        [],
    )
    .unwrap();

    v49_backfill_execution_plans::migrate(&conn).unwrap();

    let count: i32 = conn
        .query_row("SELECT COUNT(*) FROM execution_plans", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}
