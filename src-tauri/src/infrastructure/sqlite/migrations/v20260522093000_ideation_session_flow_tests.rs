//! Tests for migration v20260522093000: ideation session flow.

use rusqlite::Connection;

use super::helpers::column_exists;
use super::v20260522093000_ideation_session_flow::migrate;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("create in-memory database");
    conn.execute(
        "CREATE TABLE ideation_sessions (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            title TEXT
        )",
        [],
    )
    .expect("create legacy ideation_sessions table");
    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id, title)
         VALUES ('session-existing', 'project-1', 'Existing Session')",
        [],
    )
    .expect("insert legacy session");
    conn
}

#[test]
fn migration_adds_session_flow_with_ideation_default() {
    let conn = setup_test_db();

    migrate(&conn).unwrap();

    assert!(column_exists(&conn, "ideation_sessions", "session_flow"));
    let existing_flow: String = conn
        .query_row(
            "SELECT session_flow FROM ideation_sessions WHERE id = 'session-existing'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(existing_flow, "ideation");

    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id, title, session_flow)
         VALUES ('session-planning', 'project-1', 'Planning Session', 'planning')",
        [],
    )
    .unwrap();
    let planning_flow: String = conn
        .query_row(
            "SELECT session_flow FROM ideation_sessions WHERE id = 'session-planning'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(planning_flow, "planning");
}

#[test]
fn migration_is_idempotent() {
    let conn = setup_test_db();

    migrate(&conn).unwrap();
    migrate(&conn).unwrap();

    assert!(column_exists(&conn, "ideation_sessions", "session_flow"));
}
