//! Tests for migration v20260406043151: add last_effective_model to ideation_sessions

use rusqlite::Connection;

use super::v20260406043151_add_last_effective_model_to_ideation_sessions;

fn setup_test_db_with_ideation_sessions() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE ideation_sessions (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL DEFAULT 'active',
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')),
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
        );
        INSERT INTO ideation_sessions (id, title) VALUES ('session-1', 'Test Session');",
    )
    .expect("Failed to create ideation_sessions table");
    conn
}

#[test]
fn test_migration_adds_column() {
    let conn = setup_test_db_with_ideation_sessions();
    v20260406043151_add_last_effective_model_to_ideation_sessions::migrate(&conn).unwrap();

    // Verify the column exists and is NULL by default
    let value: Option<String> = conn
        .query_row(
            "SELECT last_effective_model FROM ideation_sessions WHERE id = 'session-1'",
            [],
            |row| row.get(0),
        )
        .expect("Failed to query last_effective_model column");

    assert_eq!(value, None, "last_effective_model should default to NULL");
}

#[test]
fn test_migration_column_accepts_model_value() {
    let conn = setup_test_db_with_ideation_sessions();
    v20260406043151_add_last_effective_model_to_ideation_sessions::migrate(&conn).unwrap();

    conn.execute(
        "UPDATE ideation_sessions SET last_effective_model = 'claude-sonnet-4-6' WHERE id = 'session-1'",
        [],
    )
    .expect("Failed to update last_effective_model");

    let value: Option<String> = conn
        .query_row(
            "SELECT last_effective_model FROM ideation_sessions WHERE id = 'session-1'",
            [],
            |row| row.get(0),
        )
        .expect("Failed to query last_effective_model");

    assert_eq!(value, Some("claude-sonnet-4-6".to_string()));
}

#[test]
fn test_migration_existing_rows_get_null_default() {
    let conn = setup_test_db_with_ideation_sessions();
    // Insert another row before migration
    conn.execute(
        "INSERT INTO ideation_sessions (id, title) VALUES ('session-2', 'Another Session')",
        [],
    )
    .expect("Failed to insert second session");

    v20260406043151_add_last_effective_model_to_ideation_sessions::migrate(&conn).unwrap();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM ideation_sessions WHERE last_effective_model IS NULL",
            [],
            |row| row.get(0),
        )
        .expect("Failed to count rows");

    assert_eq!(
        count, 2,
        "All existing rows should have NULL last_effective_model"
    );
}
