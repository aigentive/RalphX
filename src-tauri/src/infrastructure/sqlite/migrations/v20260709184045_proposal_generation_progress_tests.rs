//! Tests for migration v20260709184045: proposal generation progress

use rusqlite::Connection;

use super::helpers;
use super::v20260709184045_proposal_generation_progress;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch("CREATE TABLE ideation_sessions (id TEXT PRIMARY KEY);")
        .expect("create ideation_sessions");
    conn
}

#[test]
fn migration_adds_proposal_generation_columns() {
    let conn = setup_test_db();
    v20260709184045_proposal_generation_progress::migrate(&conn).unwrap();

    for column in [
        "proposal_generation_status",
        "proposal_generation_phase",
        "proposal_generation_expected_count",
        "proposal_generation_created_count",
        "proposal_generation_dependency_count",
        "proposal_generation_error",
        "proposal_generation_started_at",
        "proposal_generation_updated_at",
        "proposal_generation_completed_at",
    ] {
        assert!(
            helpers::column_exists(&conn, "ideation_sessions", column),
            "{column} should exist"
        );
    }
}

#[test]
fn migration_defaults_legacy_rows_to_idle() {
    let conn = setup_test_db();
    v20260709184045_proposal_generation_progress::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO ideation_sessions (id) VALUES ('session-1')",
        [],
    )
    .unwrap();

    let (status, created_count, phase): (String, i64, Option<String>) = conn
        .query_row(
            "SELECT proposal_generation_status, proposal_generation_created_count, proposal_generation_phase \
             FROM ideation_sessions WHERE id = 'session-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();

    assert_eq!(status, "idle");
    assert_eq!(created_count, 0);
    assert_eq!(phase, None);
}

#[test]
fn migration_is_idempotent() {
    let conn = setup_test_db();
    v20260709184045_proposal_generation_progress::migrate(&conn).unwrap();
    v20260709184045_proposal_generation_progress::migrate(&conn).unwrap();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('ideation_sessions') \
             WHERE name LIKE 'proposal_generation_%'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(count, 9);
}
