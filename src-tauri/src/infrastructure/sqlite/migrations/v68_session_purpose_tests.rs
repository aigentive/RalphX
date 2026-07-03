//! Tests for migration v68: session_purpose column on ideation_sessions

use rusqlite::Connection;

use super::helpers;
use super::v68_session_purpose;

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
fn test_session_purpose_column_added() {
    let conn = setup_test_db();

    assert!(
        !helpers::column_exists(&conn, "ideation_sessions", "session_purpose"),
        "session_purpose should not exist before migration"
    );

    v68_session_purpose::migrate(&conn).unwrap();

    assert!(
        helpers::column_exists(&conn, "ideation_sessions", "session_purpose"),
        "session_purpose should exist after migration"
    );
}

// ---------------------------------------------------------------------------
// Default value
// ---------------------------------------------------------------------------

#[test]
fn test_session_purpose_defaults_to_general() {
    let conn = setup_test_db();
    v68_session_purpose::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id) VALUES ('s1', 'p1')",
        [],
    )
    .unwrap();

    let val: String = conn
        .query_row(
            "SELECT session_purpose FROM ideation_sessions WHERE id = 's1'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(
        val, "general",
        "session_purpose should default to 'general'"
    );
}

// ---------------------------------------------------------------------------
// Existing sessions unaffected
// ---------------------------------------------------------------------------

#[test]
fn test_existing_sessions_get_default() {
    let conn = setup_test_db();

    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id, title) VALUES ('existing', 'p1', 'Old Session')",
        [],
    )
    .unwrap();

    v68_session_purpose::migrate(&conn).unwrap();

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

    let purpose: String = conn
        .query_row(
            "SELECT session_purpose FROM ideation_sessions WHERE id = 'existing'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(
        purpose, "general",
        "existing session should get default 'general' purpose"
    );
}

// ---------------------------------------------------------------------------
// Can set values
// ---------------------------------------------------------------------------

#[test]
fn test_can_set_verification_purpose() {
    let conn = setup_test_db();
    v68_session_purpose::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id, session_purpose)
         VALUES ('s1', 'p1', 'verification')",
        [],
    )
    .unwrap();

    let purpose: String = conn
        .query_row(
            "SELECT session_purpose FROM ideation_sessions WHERE id = 's1'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(purpose, "verification");
}

// ---------------------------------------------------------------------------
// Idempotency
// ---------------------------------------------------------------------------

#[test]
fn test_migration_idempotent() {
    let conn = setup_test_db();

    v68_session_purpose::migrate(&conn).unwrap();
    v68_session_purpose::migrate(&conn).unwrap();

    assert!(helpers::column_exists(
        &conn,
        "ideation_sessions",
        "session_purpose"
    ));
}
