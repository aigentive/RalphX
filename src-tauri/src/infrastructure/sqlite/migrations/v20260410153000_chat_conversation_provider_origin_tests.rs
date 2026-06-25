use rusqlite::Connection;

use super::v20260410153000_chat_conversation_provider_origin;

#[test]
fn test_chat_conversation_provider_origin_migration_adds_columns() {
    let conn = Connection::open_in_memory().unwrap();

    conn.execute_batch(
        "CREATE TABLE chat_conversations (
            id TEXT PRIMARY KEY
        );",
    )
    .unwrap();

    v20260410153000_chat_conversation_provider_origin::migrate(&conn).unwrap();

    let mut stmt = conn
        .prepare("PRAGMA table_info(chat_conversations)")
        .unwrap();
    let columns: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    for expected in ["upstream_provider", "provider_profile"] {
        assert!(columns.iter().any(|value| value == expected));
    }
}
