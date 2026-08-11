//! Tests for migration v20260710000000: task branch base

use rusqlite::Connection;

use super::helpers::column_exists;
use super::v20260710000000_task_branch_base;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute(
        "CREATE TABLE tasks (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            category TEXT NOT NULL DEFAULT 'feature',
            title TEXT NOT NULL
        )",
        [],
    )
    .unwrap();
    conn
}

#[test]
fn adds_task_branch_base_columns() {
    let conn = setup_test_db();
    v20260710000000_task_branch_base::migrate(&conn).unwrap();

    assert!(column_exists(&conn, "tasks", "task_branch_base_ref"));
    assert!(column_exists(&conn, "tasks", "task_branch_base_sha"));
}

#[test]
fn stores_task_branch_base_values_and_allows_nulls() {
    let conn = setup_test_db();
    v20260710000000_task_branch_base::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO tasks (id, project_id, title, task_branch_base_ref, task_branch_base_sha)
         VALUES ('task-with-base', 'project-1', 'Task With Base', 'ralphx/project/agent-1', 'abc123')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tasks (id, project_id, title)
         VALUES ('task-without-base', 'project-1', 'Task Without Base')",
        [],
    )
    .unwrap();

    let with_base: (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT task_branch_base_ref, task_branch_base_sha FROM tasks WHERE id = 'task-with-base'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(with_base.0.as_deref(), Some("ralphx/project/agent-1"));
    assert_eq!(with_base.1.as_deref(), Some("abc123"));

    let without_base: (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT task_branch_base_ref, task_branch_base_sha FROM tasks WHERE id = 'task-without-base'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(without_base, (None, None));
}

#[test]
fn migration_is_idempotent() {
    let conn = setup_test_db();
    v20260710000000_task_branch_base::migrate(&conn).unwrap();
    v20260710000000_task_branch_base::migrate(&conn).unwrap();

    let column_count: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM pragma_table_info('tasks')
             WHERE name IN ('task_branch_base_ref', 'task_branch_base_sha')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(column_count, 2);
}
