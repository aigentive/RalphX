//! Tests for migration v67: composite index on tasks(ideation_session_id, internal_status)

use rusqlite::Connection;

use super::helpers;
use super::v67_tasks_session_status_index;

// ---------------------------------------------------------------------------
// Setup helper
// ---------------------------------------------------------------------------

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");

    conn.execute_batch(
        "CREATE TABLE tasks (
            id TEXT PRIMARY KEY,
            ideation_session_id TEXT,
            internal_status TEXT NOT NULL DEFAULT 'draft',
            title TEXT,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
        );",
    )
    .expect("Failed to create test schema");

    conn
}

// ---------------------------------------------------------------------------
// Index existence tests
// ---------------------------------------------------------------------------

#[test]
fn test_index_created() {
    let conn = setup_test_db();

    assert!(
        !helpers::index_exists(&conn, "idx_tasks_session_status"),
        "index should not exist before migration"
    );

    v67_tasks_session_status_index::migrate(&conn).unwrap();

    assert!(
        helpers::index_exists(&conn, "idx_tasks_session_status"),
        "index should exist after migration"
    );
}

// ---------------------------------------------------------------------------
// Idempotency
// ---------------------------------------------------------------------------

#[test]
fn test_migration_idempotent() {
    let conn = setup_test_db();

    v67_tasks_session_status_index::migrate(&conn).unwrap();
    v67_tasks_session_status_index::migrate(&conn).unwrap();

    assert!(helpers::index_exists(&conn, "idx_tasks_session_status"));
}

// ---------------------------------------------------------------------------
// Index covers expected queries
// ---------------------------------------------------------------------------

#[test]
fn test_index_usable_for_session_status_filter() {
    let conn = setup_test_db();
    v67_tasks_session_status_index::migrate(&conn).unwrap();

    conn.execute_batch(
        "INSERT INTO tasks (id, ideation_session_id, internal_status) VALUES
            ('t1', 'sess-1', 'done'),
            ('t2', 'sess-1', 'executing'),
            ('t3', 'sess-2', 'done');",
    )
    .unwrap();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM tasks WHERE ideation_session_id = 'sess-1' AND internal_status = 'done'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(
        count, 1,
        "should find exactly one task matching session+status"
    );
}

#[test]
fn test_existing_rows_unaffected() {
    let conn = setup_test_db();

    conn.execute_batch(
        "INSERT INTO tasks (id, ideation_session_id, internal_status) VALUES
            ('t1', 'sess-1', 'done');",
    )
    .unwrap();

    v67_tasks_session_status_index::migrate(&conn).unwrap();

    let status: String = conn
        .query_row(
            "SELECT internal_status FROM tasks WHERE id = 't1'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(
        status, "done",
        "existing rows should be preserved after migration"
    );
}
