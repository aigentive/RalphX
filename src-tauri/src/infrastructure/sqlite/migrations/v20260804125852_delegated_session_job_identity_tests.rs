//! Tests for migration v20260804125852: delegated session job identity

use rusqlite::Connection;

use super::v20260804125852_delegated_session_job_identity::migrate;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE delegated_sessions (id TEXT PRIMARY KEY);
         INSERT INTO delegated_sessions (id) VALUES ('legacy-session');",
    )
    .expect("seed legacy delegated sessions table");
    conn
}

#[test]
fn migration_preserves_legacy_sessions_and_adds_nullable_identity_columns() {
    let conn = setup_test_db();
    migrate(&conn).expect("migrate delegated session identity");

    let (job_id, parent_agent_run_id): (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT job_id, parent_agent_run_id
             FROM delegated_sessions
             WHERE id = 'legacy-session'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read legacy delegated session");
    assert!(job_id.is_none());
    assert!(parent_agent_run_id.is_none());
}

#[test]
fn migration_is_idempotent() {
    let conn = setup_test_db();
    migrate(&conn).expect("first migration run");
    migrate(&conn).expect("second migration run");

    let columns = conn
        .prepare("PRAGMA table_info(delegated_sessions)")
        .expect("prepare table info")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("query table info")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect table columns");
    assert_eq!(
        columns.iter().filter(|column| *column == "job_id").count(),
        1
    );
    assert_eq!(
        columns
            .iter()
            .filter(|column| *column == "parent_agent_run_id")
            .count(),
        1
    );
}
