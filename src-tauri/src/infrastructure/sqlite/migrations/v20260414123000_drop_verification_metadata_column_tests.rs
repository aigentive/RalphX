use rusqlite::Connection;

use super::helpers;
use super::v20260414123000_drop_verification_metadata_column;
use super::v57_plan_verification;

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
fn test_drop_verification_metadata_column_removes_dead_column() {
    let conn = setup_test_db();
    v57_plan_verification::migrate(&conn).unwrap();
    assert!(helpers::column_exists(
        &conn,
        "ideation_sessions",
        "verification_metadata"
    ));

    v20260414123000_drop_verification_metadata_column::migrate(&conn).unwrap();

    assert!(!helpers::column_exists(
        &conn,
        "ideation_sessions",
        "verification_metadata"
    ));
    assert!(helpers::column_exists(
        &conn,
        "ideation_sessions",
        "verification_status"
    ));
    assert!(helpers::column_exists(
        &conn,
        "ideation_sessions",
        "verification_in_progress"
    ));
}

#[test]
fn test_drop_verification_metadata_column_preserves_session_rows() {
    let conn = setup_test_db();
    v57_plan_verification::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO ideation_sessions (
            id, project_id, verification_status, verification_in_progress, verification_metadata
        ) VALUES ('s1', 'p1', 'reviewing', 1, '{\"v\":1}')",
        [],
    )
    .unwrap();

    v20260414123000_drop_verification_metadata_column::migrate(&conn).unwrap();

    let (status, in_progress): (String, i64) = conn
        .query_row(
            "SELECT verification_status, verification_in_progress
             FROM ideation_sessions
             WHERE id = 's1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();

    assert_eq!(status, "reviewing");
    assert_eq!(in_progress, 1);
}

#[test]
fn test_drop_verification_metadata_column_is_idempotent() {
    let conn = setup_test_db();
    v57_plan_verification::migrate(&conn).unwrap();

    v20260414123000_drop_verification_metadata_column::migrate(&conn).unwrap();
    v20260414123000_drop_verification_metadata_column::migrate(&conn).unwrap();

    assert!(!helpers::column_exists(
        &conn,
        "ideation_sessions",
        "verification_metadata"
    ));
}
