//! Tests for migration v20260803105827: agent workspace repair ci rerun deferral

use rusqlite::Connection;

use super::v20260803105827_agent_workspace_repair_ci_rerun_deferral::migrate;

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

#[test]
fn migration_adds_nullable_deferred_ci_rerun_columns() {
    let conn = setup_test_db();
    conn.execute_batch(
        "CREATE TABLE agent_workspace_repair_attempts (id TEXT PRIMARY KEY);
         INSERT INTO agent_workspace_repair_attempts (id) VALUES ('repair-1');",
    )
    .expect("legacy repair attempts table should exist");

    migrate(&conn).expect("migration should succeed");

    let deferral: (Option<i64>, Option<String>) = conn
        .query_row(
            "SELECT ci_rerun_pending_run_id, ci_rerun_deferred_since
             FROM agent_workspace_repair_attempts WHERE id = 'repair-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("existing attempt should have no pending rerun by default");
    assert_eq!(deferral, (None, None));
}
