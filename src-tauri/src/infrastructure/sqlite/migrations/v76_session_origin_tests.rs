//! Tests for migration v76: origin column on ideation_sessions

use rusqlite::Connection;

use super::helpers;
use super::v76_session_origin;

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

#[test]
fn test_origin_column_added() {
    let conn = setup_test_db();

    assert!(
        !helpers::column_exists(&conn, "ideation_sessions", "origin"),
        "origin should not exist before migration"
    );

    v76_session_origin::migrate(&conn).unwrap();

    assert!(
        helpers::column_exists(&conn, "ideation_sessions", "origin"),
        "origin should exist after migration"
    );
}

#[test]
fn test_origin_defaults_to_internal() {
    let conn = setup_test_db();
    v76_session_origin::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id) VALUES ('s1', 'p1')",
        [],
    )
    .unwrap();

    let val: String = conn
        .query_row(
            "SELECT origin FROM ideation_sessions WHERE id = 's1'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(val, "internal", "origin should default to 'internal'");
}

#[test]
fn test_existing_sessions_get_default_internal() {
    let conn = setup_test_db();

    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id, title) VALUES ('existing', 'p1', 'Old Session')",
        [],
    )
    .unwrap();

    v76_session_origin::migrate(&conn).unwrap();

    let origin: String = conn
        .query_row(
            "SELECT origin FROM ideation_sessions WHERE id = 'existing'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(
        origin, "internal",
        "existing session should get default 'internal' origin"
    );
}

#[test]
fn test_can_set_external_origin() {
    let conn = setup_test_db();
    v76_session_origin::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id, origin)
         VALUES ('s1', 'p1', 'external')",
        [],
    )
    .unwrap();

    let origin: String = conn
        .query_row(
            "SELECT origin FROM ideation_sessions WHERE id = 's1'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(origin, "external");
}

#[test]
fn test_migration_idempotent() {
    let conn = setup_test_db();

    v76_session_origin::migrate(&conn).unwrap();
    v76_session_origin::migrate(&conn).unwrap();

    assert!(helpers::column_exists(&conn, "ideation_sessions", "origin"));
}
