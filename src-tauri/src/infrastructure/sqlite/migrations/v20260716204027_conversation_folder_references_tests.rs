//! Tests for migration v20260716204027: conversation folder references

use rusqlite::Connection;

use super::v20260716204027_conversation_folder_references;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE ui_feature_flag_overrides (
            id INTEGER PRIMARY KEY CHECK(id = 1),
            agent_personas INTEGER NULL
        );",
    )
    .expect("create feature flags dependency");
    conn
}

#[test]
fn test_migration_runs() {
    let conn = setup_test_db();
    conn.execute(
        "CREATE TABLE chat_conversations (id TEXT PRIMARY KEY NOT NULL)",
        [],
    )
    .expect("create dependency");
    v20260716204027_conversation_folder_references::migrate(&conn).unwrap();

    let index_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_conversation_folder_references_conversation_id'",
            [],
            |row| row.get(0),
        )
        .expect("query index");
    assert_eq!(index_count, 1);
    let unique_index_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_conversation_folder_references_live_path'",
            [],
            |row| row.get(0),
        )
        .expect("query unique index");
    assert_eq!(unique_index_count, 1);
    let composer_flag_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('ui_feature_flag_overrides') WHERE name = 'composer_folder_references'",
            [],
            |row| row.get(0),
        )
        .expect("query composer flag column");
    assert_eq!(composer_flag_count, 1);
}
