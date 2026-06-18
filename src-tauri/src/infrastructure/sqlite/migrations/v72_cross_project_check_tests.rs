//! Tests for migration v72: cross_project_checked column on ideation_sessions

use rusqlite::Connection;

use super::helpers;
use super::v72_cross_project_check;

// ---------------------------------------------------------------------------
// Setup helper
// ---------------------------------------------------------------------------

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");

    conn.execute_batch(
        "CREATE TABLE ideation_sessions (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            title TEXT,
            status TEXT NOT NULL DEFAULT 'active',
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
        );",
    )
    .expect("Failed to create test schema");

    conn
}

// ---------------------------------------------------------------------------
// Column existence tests
// ---------------------------------------------------------------------------

#[test]
fn test_cross_project_checked_column_added() {
    let conn = setup_test_db();

    assert!(
        !helpers::column_exists(&conn, "ideation_sessions", "cross_project_checked"),
        "cross_project_checked should not exist before migration"
    );

    v72_cross_project_check::migrate(&conn).unwrap();

    assert!(
        helpers::column_exists(&conn, "ideation_sessions", "cross_project_checked"),
        "cross_project_checked should exist after migration"
    );
}

// ---------------------------------------------------------------------------
// Default value (existing rows get 1 = true)
// ---------------------------------------------------------------------------

#[test]
fn test_cross_project_checked_defaults_to_true() {
    let conn = setup_test_db();
    v72_cross_project_check::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id) VALUES ('s1', 'p1')",
        [],
    )
    .unwrap();

    let val: i64 = conn
        .query_row(
            "SELECT cross_project_checked FROM ideation_sessions WHERE id = 's1'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(val, 1, "cross_project_checked should default to 1 (true)");
}

// ---------------------------------------------------------------------------
// Existing sessions are unaffected (they inherit the default 1)
// ---------------------------------------------------------------------------

#[test]
fn test_existing_sessions_get_default_true() {
    let conn = setup_test_db();

    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id, title) VALUES ('existing', 'p1', 'Old Session')",
        [],
    )
    .unwrap();

    v72_cross_project_check::migrate(&conn).unwrap();

    let title: String = conn
        .query_row(
            "SELECT title FROM ideation_sessions WHERE id = 'existing'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(
        title, "Old Session",
        "existing session data should be preserved"
    );

    let checked: i64 = conn
        .query_row(
            "SELECT cross_project_checked FROM ideation_sessions WHERE id = 'existing'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(checked, 1, "existing session should get default 1 (true)");
}

// ---------------------------------------------------------------------------
// Can set to false
// ---------------------------------------------------------------------------

#[test]
fn test_can_set_cross_project_checked_to_false() {
    let conn = setup_test_db();
    v72_cross_project_check::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id, cross_project_checked)
         VALUES ('s1', 'p1', 0)",
        [],
    )
    .unwrap();

    let val: i64 = conn
        .query_row(
            "SELECT cross_project_checked FROM ideation_sessions WHERE id = 's1'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(
        val, 0,
        "cross_project_checked should be settable to 0 (false)"
    );
}

// ---------------------------------------------------------------------------
// Idempotency
// ---------------------------------------------------------------------------

#[test]
fn test_migration_idempotent() {
    let conn = setup_test_db();

    v72_cross_project_check::migrate(&conn).unwrap();
    v72_cross_project_check::migrate(&conn).unwrap();

    assert!(helpers::column_exists(
        &conn,
        "ideation_sessions",
        "cross_project_checked"
    ));
}
