//! Tests for migration v20260708131548: chat conversation coordination mode

use rusqlite::Connection;

use super::helpers::column_exists;
use super::v20260708131548_chat_conversation_coordination_mode;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute(
        "CREATE TABLE chat_conversations (
            id TEXT PRIMARY KEY,
            context_type TEXT NOT NULL,
            context_id TEXT NOT NULL
        )",
        [],
    )
    .unwrap();
    conn
}

#[test]
fn adds_coordination_mode_with_solo_default() {
    let conn = setup_test_db();
    v20260708131548_chat_conversation_coordination_mode::migrate(&conn).unwrap();

    assert!(column_exists(
        &conn,
        "chat_conversations",
        "coordination_mode"
    ));

    conn.execute(
        "INSERT INTO chat_conversations (id, context_type, context_id)
         VALUES ('conv-solo', 'project', 'project-1')",
        [],
    )
    .unwrap();

    let coordination_mode: String = conn
        .query_row(
            "SELECT coordination_mode FROM chat_conversations WHERE id = 'conv-solo'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(coordination_mode, "solo");
}

#[test]
fn accepts_rx_native_team_values_and_rejects_invalid_values() {
    let conn = setup_test_db();
    v20260708131548_chat_conversation_coordination_mode::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO chat_conversations (id, context_type, context_id, coordination_mode)
         VALUES ('conv-team', 'project', 'project-1', 'rx_native_team')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO chat_conversations (id, context_type, context_id, coordination_mode)
         VALUES ('conv-legacy', 'project', 'project-1', 'legacy_claude_team')",
        [],
    )
    .unwrap();

    let result = conn.execute(
        "INSERT INTO chat_conversations (id, context_type, context_id, coordination_mode)
         VALUES ('conv-invalid', 'project', 'project-1', 'legacy_native')",
        [],
    );
    assert!(result.is_err());
}

#[test]
fn migration_is_idempotent() {
    let conn = setup_test_db();
    v20260708131548_chat_conversation_coordination_mode::migrate(&conn).unwrap();
    v20260708131548_chat_conversation_coordination_mode::migrate(&conn).unwrap();

    let column_count: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM pragma_table_info('chat_conversations')
             WHERE name = 'coordination_mode'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(column_count, 1);
}
