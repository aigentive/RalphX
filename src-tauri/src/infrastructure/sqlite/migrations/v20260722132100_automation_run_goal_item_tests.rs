//! Tests for migration v20260722132100: automation run goal item

use rusqlite::Connection;

use super::v20260722132100_automation_run_goal_item;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE automation_runs (
            id TEXT PRIMARY KEY,
            automation_id TEXT NOT NULL,
            run_index INTEGER NOT NULL
        );",
    )
    .unwrap();
    conn
}

#[test]
fn migration_adds_nullable_goal_item_id_column() {
    let conn = setup_test_db();
    conn.execute(
        "INSERT INTO automation_runs (id, automation_id, run_index) VALUES ('run-1', 'auto-1', 1)",
        [],
    )
    .unwrap();

    v20260722132100_automation_run_goal_item::migrate(&conn).unwrap();

    let goal_item_id: Option<String> = conn
        .query_row(
            "SELECT goal_item_id FROM automation_runs WHERE id = 'run-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(goal_item_id, None, "existing runs must stay unmapped");
}

#[test]
fn migration_is_idempotent_and_preserves_existing_values() {
    let conn = setup_test_db();
    v20260722132100_automation_run_goal_item::migrate(&conn).unwrap();
    conn.execute(
        "INSERT INTO automation_runs (id, automation_id, run_index, goal_item_id)
         VALUES ('run-1', 'auto-1', 1, 'item-b1')",
        [],
    )
    .unwrap();

    v20260722132100_automation_run_goal_item::migrate(&conn)
        .expect("migration rerun must tolerate a previously added column");

    let goal_item_id: Option<String> = conn
        .query_row(
            "SELECT goal_item_id FROM automation_runs WHERE id = 'run-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(goal_item_id, Some("item-b1".to_string()));
}
