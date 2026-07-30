//! Tests for migration v20260730151837: agent workspace repair ci rerun reservations

use rusqlite::Connection;

use super::v20260730151837_agent_workspace_repair_ci_rerun_reservations::migrate;

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

#[test]
fn migration_adds_durable_ci_rerun_reservation_columns() {
    let conn = setup_test_db();
    conn.execute_batch(
        "CREATE TABLE agent_workspace_repair_attempts (id TEXT PRIMARY KEY);
         INSERT INTO agent_workspace_repair_attempts (id) VALUES ('repair-1');",
    )
    .expect("legacy repair attempts table should exist");

    migrate(&conn).expect("migration should succeed");

    let reservation: (i64, Option<String>) = conn
        .query_row(
            "SELECT ci_rerun_count, ci_rerun_fingerprint
             FROM agent_workspace_repair_attempts WHERE id = 'repair-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("existing attempt should retain a default reservation state");
    assert_eq!(reservation, (0, None));
}
