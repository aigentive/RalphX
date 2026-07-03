//! Tests for migration v20260523070000: plan artifact approvals.

use rusqlite::Connection;

use super::helpers::table_exists;
use super::v20260523070000_plan_artifact_approvals::migrate;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("create in-memory database");
    conn.execute(
        "CREATE TABLE ideation_sessions (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL
        )",
        [],
    )
    .expect("create ideation_sessions table");
    conn.execute(
        "CREATE TABLE artifacts (
            id TEXT PRIMARY KEY,
            version INTEGER DEFAULT 1
        )",
        [],
    )
    .expect("create artifacts table");
    conn
}

#[test]
fn migration_creates_plan_artifact_approvals_table() {
    let conn = setup_test_db();

    migrate(&conn).unwrap();

    assert!(table_exists(&conn, "plan_artifact_approvals"));
    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id) VALUES ('session-1', 'project-1')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO artifacts (id, version) VALUES ('artifact-1', 2)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO plan_artifact_approvals (
            session_id, artifact_id, artifact_version, status, approved_at, approved_by
         ) VALUES ('session-1', 'artifact-1', 2, 'approved', '2026-05-23T00:00:00Z', 'user')",
        [],
    )
    .unwrap();
}

#[test]
fn migration_is_idempotent() {
    let conn = setup_test_db();

    migrate(&conn).unwrap();
    migrate(&conn).unwrap();

    assert!(table_exists(&conn, "plan_artifact_approvals"));
}
