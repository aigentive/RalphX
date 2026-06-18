//! Tests for migration v20260406120000: add ideation_subagent_model column

use rusqlite::Connection;

use super::v20260406120000_add_ideation_subagent_model;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE ideation_model_settings (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id TEXT,
            primary_model TEXT NOT NULL DEFAULT 'inherit',
            verifier_model TEXT NOT NULL DEFAULT 'inherit',
            verifier_subagent_model TEXT NOT NULL DEFAULT 'inherit',
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
        );
        INSERT INTO ideation_model_settings (project_id, primary_model, verifier_model, verifier_subagent_model)
            VALUES (NULL, 'sonnet', 'opus', 'inherit');",
    )
    .expect("Failed to create ideation_model_settings table");
    conn
}

#[test]
fn test_migration_adds_ideation_subagent_model_column() {
    let conn = setup_test_db();
    v20260406120000_add_ideation_subagent_model::migrate(&conn).unwrap();

    let value: String = conn
        .query_row(
            "SELECT ideation_subagent_model FROM ideation_model_settings WHERE project_id IS NULL",
            [],
            |row| row.get(0),
        )
        .expect("Failed to query ideation_subagent_model column");

    assert_eq!(
        value, "inherit",
        "ideation_subagent_model should default to 'inherit'"
    );
}

#[test]
fn test_migration_existing_rows_get_inherit_default() {
    let conn = setup_test_db();
    conn.execute(
        "INSERT INTO ideation_model_settings (project_id, primary_model, verifier_model, verifier_subagent_model) VALUES ('proj-1', 'haiku', 'sonnet', 'inherit')",
        [],
    )
    .expect("Failed to insert project row");

    v20260406120000_add_ideation_subagent_model::migrate(&conn).unwrap();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM ideation_model_settings WHERE ideation_subagent_model = 'inherit'",
            [],
            |row| row.get(0),
        )
        .expect("Failed to count rows");

    assert_eq!(count, 2, "All existing rows should default to 'inherit'");
}

#[test]
fn test_migration_column_accepts_model_values() {
    let conn = setup_test_db();
    v20260406120000_add_ideation_subagent_model::migrate(&conn).unwrap();

    conn.execute(
        "UPDATE ideation_model_settings SET ideation_subagent_model = 'sonnet' WHERE project_id IS NULL",
        [],
    )
    .expect("Failed to update ideation_subagent_model");

    let value: String = conn
        .query_row(
            "SELECT ideation_subagent_model FROM ideation_model_settings WHERE project_id IS NULL",
            [],
            |row| row.get(0),
        )
        .expect("Failed to query ideation_subagent_model");

    assert_eq!(value, "sonnet");
}
