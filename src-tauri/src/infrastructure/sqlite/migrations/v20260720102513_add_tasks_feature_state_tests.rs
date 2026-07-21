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

#[test]
fn migration_can_resume_after_the_column_exists_without_overwriting_draining() {
    let conn = setup_test_db();
    conn.execute(
        "INSERT INTO ideation_settings (id, tasks_enabled) VALUES (1, 1)",
        [],
    )
    .unwrap();

    v20260720102513_add_tasks_feature_state::migrate(&conn).unwrap();
    conn.execute(
        "UPDATE ideation_settings
         SET tasks_enabled = 0, tasks_feature_state = 'draining' WHERE id = 1",
        [],
    )
    .unwrap();

    v20260720102513_add_tasks_feature_state::migrate(&conn)
        .expect("migration rerun must tolerate a previously added column");

    let state: String = conn
        .query_row(
            "SELECT tasks_feature_state FROM ideation_settings WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        state, "draining",
        "rerun backfill must preserve an in-progress shutdown"
    );
}

#[test]
fn migration_reports_a_missing_settings_table() {
    let conn = Connection::open_in_memory().expect("in-memory database should open");

    let error = v20260720102513_add_tasks_feature_state::migrate(&conn)
        .expect_err("migration must report a missing settings table");

    assert!(error.to_string().contains("ideation_settings"));
}

#[test]
fn migration_reports_a_missing_legacy_tasks_enabled_column() {
    let conn = Connection::open_in_memory().expect("in-memory database should open");
    conn.execute_batch(
        "CREATE TABLE ideation_settings (
            id INTEGER PRIMARY KEY,
            tasks_feature_state TEXT NOT NULL DEFAULT 'disabled'
                CHECK (tasks_feature_state IN ('enabled', 'draining', 'disabled'))
        );",
    )
    .unwrap();

    let error = v20260720102513_add_tasks_feature_state::migrate(&conn)
        .expect_err("migration backfill must report a missing legacy column");

    assert!(error.to_string().contains("tasks_enabled"));
}
