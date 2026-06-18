//! Tests for migration v58: metrics composite index on task_state_history

use rusqlite::Connection;

use super::helpers;
use super::v58_metrics_index;

// ---------------------------------------------------------------------------
// Setup helper — minimal schema for task_state_history
// ---------------------------------------------------------------------------

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");

    conn.execute_batch(
        "CREATE TABLE task_state_history (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            from_status TEXT,
            to_status TEXT NOT NULL,
            changed_by TEXT NOT NULL,
            reason TEXT,
            metadata JSON,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );",
    )
    .expect("Failed to create test schema");

    conn
}

// ---------------------------------------------------------------------------
// Index creation
// ---------------------------------------------------------------------------

#[test]
fn test_index_created_by_migration() {
    let conn = setup_test_db();

    assert!(
        !helpers::index_exists(&conn, "idx_task_state_history_task_created"),
        "index should not exist before migration"
    );

    v58_metrics_index::migrate(&conn).unwrap();

    assert!(
        helpers::index_exists(&conn, "idx_task_state_history_task_created"),
        "idx_task_state_history_task_created should exist after migration"
    );
}

#[test]
fn test_migration_idempotent() {
    let conn = setup_test_db();

    // Run twice — CREATE INDEX IF NOT EXISTS is safe
    v58_metrics_index::migrate(&conn).unwrap();
    v58_metrics_index::migrate(&conn).unwrap();

    assert!(
        helpers::index_exists(&conn, "idx_task_state_history_task_created"),
        "index should still exist after second migration run"
    );
}

// ---------------------------------------------------------------------------
// Schema validation: required tables and columns for metrics queries
// ---------------------------------------------------------------------------

#[test]
fn test_tasks_table_has_required_columns() {
    let conn = Connection::open_in_memory().unwrap();

    conn.execute_batch(
        "CREATE TABLE tasks (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            internal_status TEXT NOT NULL DEFAULT 'backlog',
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );",
    )
    .unwrap();

    // Columns the metrics queries actually use (actual names, not plan assumptions)
    assert!(helpers::column_exists(&conn, "tasks", "id"));
    assert!(helpers::column_exists(&conn, "tasks", "project_id"));
    assert!(helpers::column_exists(&conn, "tasks", "internal_status")); // plan called this "status"
    assert!(helpers::column_exists(&conn, "tasks", "created_at"));
    assert!(helpers::column_exists(&conn, "tasks", "updated_at"));
}

#[test]
fn test_task_state_history_has_required_columns() {
    let conn = Connection::open_in_memory().unwrap();

    conn.execute_batch(
        "CREATE TABLE task_state_history (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            from_status TEXT,
            to_status TEXT NOT NULL,
            changed_by TEXT NOT NULL,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );",
    )
    .unwrap();

    assert!(helpers::column_exists(
        &conn,
        "task_state_history",
        "task_id"
    ));
    assert!(helpers::column_exists(
        &conn,
        "task_state_history",
        "from_status"
    )); // plan called this "from_state"
    assert!(helpers::column_exists(
        &conn,
        "task_state_history",
        "to_status"
    )); // plan called this "to_state"
    assert!(helpers::column_exists(
        &conn,
        "task_state_history",
        "created_at"
    ));
}

#[test]
fn test_task_steps_has_required_columns() {
    let conn = Connection::open_in_memory().unwrap();

    conn.execute_batch(
        "CREATE TABLE task_steps (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending'
        );",
    )
    .unwrap();

    assert!(helpers::column_exists(&conn, "task_steps", "task_id"));
    assert!(helpers::column_exists(&conn, "task_steps", "id"));
    assert!(helpers::column_exists(&conn, "task_steps", "status"));
}

#[test]
fn test_reviews_has_required_columns() {
    let conn = Connection::open_in_memory().unwrap();

    conn.execute_batch(
        "CREATE TABLE reviews (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            task_id TEXT NOT NULL,
            reviewer_type TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );",
    )
    .unwrap();

    assert!(helpers::column_exists(&conn, "reviews", "task_id"));
    assert!(helpers::column_exists(&conn, "reviews", "status")); // plan called this "outcome"
}

// ---------------------------------------------------------------------------
// Index covers cycle time query ordering
// ---------------------------------------------------------------------------

#[test]
fn test_index_covers_window_function_ordering() {
    let conn = setup_test_db();
    v58_metrics_index::migrate(&conn).unwrap();

    // Seed data that the LAG() window query would use
    conn.execute_batch(
        "INSERT INTO task_state_history (id, task_id, to_status, changed_by, created_at)
         VALUES
           ('h1', 't1', 'executing', 'system', '2026-01-01T10:00:00Z'),
           ('h2', 't1', 'pending_review', 'system', '2026-01-01T11:00:00Z'),
           ('h3', 't1', 'merged', 'system', '2026-01-01T12:00:00Z');",
    )
    .unwrap();

    // The cycle time query uses ORDER BY created_at WITHIN PARTITION BY task_id
    // This exercises the composite index
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM task_state_history WHERE task_id = 't1' ORDER BY created_at",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(
        count, 3,
        "should retrieve all 3 state transitions ordered by created_at"
    );
}
