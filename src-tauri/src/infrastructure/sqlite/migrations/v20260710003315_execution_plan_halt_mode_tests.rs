//! Tests for migration v20260710003315: execution plan halt mode

use rusqlite::Connection;

use super::v20260710003315_execution_plan_halt_mode;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE execution_plans (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'active',
            created_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
        );",
    )
    .unwrap();
    conn
}

#[test]
fn test_migration_adds_halt_mode_column_with_running_default() {
    let conn = setup_test_db();
    conn.execute(
        "INSERT INTO execution_plans (id, session_id, status)
         VALUES ('plan-1', 'session-1', 'active')",
        [],
    )
    .unwrap();

    v20260710003315_execution_plan_halt_mode::migrate(&conn).unwrap();

    let halt_mode: String = conn
        .query_row(
            "SELECT halt_mode FROM execution_plans WHERE id = 'plan-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(halt_mode, "running");
}

#[test]
fn test_migration_is_idempotent() {
    let conn = setup_test_db();

    v20260710003315_execution_plan_halt_mode::migrate(&conn).unwrap();
    v20260710003315_execution_plan_halt_mode::migrate(&conn).unwrap();

    let column_count: i64 = conn
        .prepare("PRAGMA table_info(execution_plans)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|column| column == "halt_mode")
        .count() as i64;

    assert_eq!(column_count, 1);
}

#[test]
fn test_migration_rejects_unknown_halt_mode_values() {
    let conn = setup_test_db();

    v20260710003315_execution_plan_halt_mode::migrate(&conn).unwrap();

    let result = conn.execute(
        "INSERT INTO execution_plans (id, session_id, status, halt_mode)
         VALUES ('plan-invalid', 'session-1', 'active', 'invalid')",
        [],
    );

    assert!(
        result.is_err(),
        "halt_mode should be constrained to known execution plan halt modes"
    );
}
