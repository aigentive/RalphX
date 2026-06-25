//! Tests for migration v20260406000000: add verifier_subagent_model column

use rusqlite::Connection;

use super::v20260406000000_verifier_subagent_model;

fn setup_test_db_with_ideation_model_settings() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE ideation_model_settings (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id TEXT,
            primary_model TEXT NOT NULL DEFAULT 'inherit',
            verifier_model TEXT NOT NULL DEFAULT 'inherit',
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
        );
        INSERT INTO ideation_model_settings (project_id, primary_model, verifier_model)
            VALUES (NULL, 'sonnet', 'opus');",
    )
    .expect("Failed to create ideation_model_settings table");
    conn
}

#[test]
fn test_migration_adds_verifier_subagent_model_column() {
    let conn = setup_test_db_with_ideation_model_settings();
    v20260406000000_verifier_subagent_model::migrate(&conn).unwrap();

    // Verify the column exists and defaults to 'inherit'
    let value: String = conn
        .query_row(
            "SELECT verifier_subagent_model FROM ideation_model_settings WHERE project_id IS NULL",
            [],
            |row| row.get(0),
        )
        .expect("Failed to query verifier_subagent_model column");

    assert_eq!(
        value, "inherit",
        "verifier_subagent_model should default to 'inherit'"
    );
}

#[test]
fn test_migration_existing_rows_get_inherit_default() {
    let conn = setup_test_db_with_ideation_model_settings();
    // Insert a second row before migration
    conn.execute(
        "INSERT INTO ideation_model_settings (project_id, primary_model, verifier_model) VALUES ('proj-1', 'haiku', 'sonnet')",
        [],
    )
    .expect("Failed to insert project row");

    v20260406000000_verifier_subagent_model::migrate(&conn).unwrap();

    // Both rows should have 'inherit' as default
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM ideation_model_settings WHERE verifier_subagent_model = 'inherit'",
            [],
            |row| row.get(0),
        )
        .expect("Failed to count rows");

    assert_eq!(count, 2, "All existing rows should default to 'inherit'");
}

#[test]
fn test_migration_column_accepts_model_values() {
    let conn = setup_test_db_with_ideation_model_settings();
    v20260406000000_verifier_subagent_model::migrate(&conn).unwrap();

    // Verify the column can be updated to model level values
    conn.execute(
        "UPDATE ideation_model_settings SET verifier_subagent_model = 'haiku' WHERE project_id IS NULL",
        [],
    )
    .expect("Failed to update verifier_subagent_model");

    let value: String = conn
        .query_row(
            "SELECT verifier_subagent_model FROM ideation_model_settings WHERE project_id IS NULL",
            [],
            |row| row.get(0),
        )
        .expect("Failed to query verifier_subagent_model");

    assert_eq!(value, "haiku");
}
