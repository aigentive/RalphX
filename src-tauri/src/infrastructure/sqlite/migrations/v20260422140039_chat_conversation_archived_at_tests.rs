//! Tests for migration v20260422140039: chat conversation archived at

use rusqlite::Connection;

use super::v20260422140039_chat_conversation_archived_at;

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

#[test]
fn test_migration_runs() {
    let conn = setup_test_db();
    conn.execute_batch(
        "CREATE TABLE chat_conversations (
            id TEXT PRIMARY KEY
        );",
    )
    .unwrap();

    v20260422140039_chat_conversation_archived_at::migrate(&conn).unwrap();

    let mut stmt = conn
        .prepare("PRAGMA table_info(chat_conversations)")
        .unwrap();
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(columns.iter().any(|column| column == "archived_at"));

    let index_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'index'
               AND name = 'idx_chat_conversations_archived_at'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(index_count, 1);
}
