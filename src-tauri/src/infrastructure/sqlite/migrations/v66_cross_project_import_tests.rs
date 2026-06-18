//! Tests for migration v66: source_project_id and source_session_id columns on ideation_sessions

use rusqlite::Connection;

use super::helpers;
use super::v66_cross_project_import;

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
fn test_source_project_id_column_added() {
    let conn = setup_test_db();

    assert!(
        !helpers::column_exists(&conn, "ideation_sessions", "source_project_id"),
        "source_project_id should not exist before migration"
    );

    v66_cross_project_import::migrate(&conn).unwrap();

    assert!(
        helpers::column_exists(&conn, "ideation_sessions", "source_project_id"),
        "source_project_id should exist after migration"
    );
}

#[test]
fn test_source_session_id_column_added() {
    let conn = setup_test_db();

    assert!(
        !helpers::column_exists(&conn, "ideation_sessions", "source_session_id"),
        "source_session_id should not exist before migration"
    );

    v66_cross_project_import::migrate(&conn).unwrap();

    assert!(
        helpers::column_exists(&conn, "ideation_sessions", "source_session_id"),
        "source_session_id should exist after migration"
    );
}

// ---------------------------------------------------------------------------
// Default values (NULL)
// ---------------------------------------------------------------------------

#[test]
fn test_source_project_id_defaults_to_null() {
    let conn = setup_test_db();
    v66_cross_project_import::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id) VALUES ('s1', 'p1')",
        [],
    )
    .unwrap();

    let val: Option<String> = conn
        .query_row(
            "SELECT source_project_id FROM ideation_sessions WHERE id = 's1'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert!(val.is_none(), "source_project_id should default to NULL");
}

#[test]
fn test_source_session_id_defaults_to_null() {
    let conn = setup_test_db();
    v66_cross_project_import::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id) VALUES ('s1', 'p1')",
        [],
    )
    .unwrap();

    let val: Option<String> = conn
        .query_row(
            "SELECT source_session_id FROM ideation_sessions WHERE id = 's1'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert!(val.is_none(), "source_session_id should default to NULL");
}

// ---------------------------------------------------------------------------
// Existing sessions unaffected
// ---------------------------------------------------------------------------

#[test]
fn test_existing_sessions_unaffected() {
    let conn = setup_test_db();

    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id, title) VALUES ('existing', 'p1', 'Old Session')",
        [],
    )
    .unwrap();

    v66_cross_project_import::migrate(&conn).unwrap();

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

    let src_proj: Option<String> = conn
        .query_row(
            "SELECT source_project_id FROM ideation_sessions WHERE id = 'existing'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert!(
        src_proj.is_none(),
        "existing session source_project_id should be NULL"
    );
}

// ---------------------------------------------------------------------------
// Can set values
// ---------------------------------------------------------------------------

#[test]
fn test_can_set_source_fields() {
    let conn = setup_test_db();
    v66_cross_project_import::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id, source_project_id, source_session_id)
         VALUES ('s1', 'p1', 'proj-src', 'sess-src')",
        [],
    )
    .unwrap();

    let (proj, sess): (String, String) = conn
        .query_row(
            "SELECT source_project_id, source_session_id FROM ideation_sessions WHERE id = 's1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();

    assert_eq!(proj, "proj-src");
    assert_eq!(sess, "sess-src");
}

// ---------------------------------------------------------------------------
// Idempotency
// ---------------------------------------------------------------------------

#[test]
fn test_migration_idempotent() {
    let conn = setup_test_db();

    v66_cross_project_import::migrate(&conn).unwrap();
    v66_cross_project_import::migrate(&conn).unwrap();

    assert!(helpers::column_exists(
        &conn,
        "ideation_sessions",
        "source_project_id"
    ));
    assert!(helpers::column_exists(
        &conn,
        "ideation_sessions",
        "source_session_id"
    ));
}
