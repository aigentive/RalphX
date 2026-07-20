//! Tests for migration v20260720102513: add tasks feature state

use rusqlite::Connection;

use super::v20260720102513_add_tasks_feature_state;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE ideation_settings (
            id INTEGER PRIMARY KEY,
            tasks_enabled INTEGER NOT NULL DEFAULT 0
        );",
    )
    .unwrap();
    conn
}

#[test]
fn migration_seeds_feature_state_from_legacy_boolean() {
    let conn = setup_test_db();
    conn.execute(
        "INSERT INTO ideation_settings (id, tasks_enabled) VALUES (1, 0), (2, 1)",
        [],
    )
    .unwrap();

    v20260720102513_add_tasks_feature_state::migrate(&conn).unwrap();

    let states = conn
        .prepare("SELECT tasks_feature_state FROM ideation_settings ORDER BY id")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(states, vec!["disabled", "enabled"]);
}

#[test]
fn migration_rejects_unknown_feature_state() {
    let conn = setup_test_db();
    v20260720102513_add_tasks_feature_state::migrate(&conn).unwrap();

    let error = conn
        .execute(
            "INSERT INTO ideation_settings (id, tasks_enabled, tasks_feature_state)
             VALUES (1, 0, 'unknown')",
            [],
        )
        .expect_err("CHECK constraint must reject unknown Tasks states");

    assert!(error.to_string().contains("CHECK constraint failed"));
}
