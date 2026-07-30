//! Tests for migration v20260730025727: chat message blocks thinking kind

use rusqlite::Connection;

use super::{
    v20260510185257_chat_message_blocks_timeline, v20260730025727_chat_message_blocks_thinking_kind,
};

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

fn migrated_connection() -> Connection {
    let conn = setup_test_db();
    // The baseline migration backfills blocks from chat_messages, so the
    // fixture needs the columns that backfill reads.
    conn.execute_batch(
        "CREATE TABLE chat_conversations (id TEXT PRIMARY KEY);
         CREATE TABLE chat_messages (
             id TEXT PRIMARY KEY,
             conversation_id TEXT,
             role TEXT,
             content TEXT,
             content_blocks TEXT,
             created_at TEXT
         );",
    )
    .unwrap();
    v20260510185257_chat_message_blocks_timeline::migrate(&conn).unwrap();
    conn.execute(
        "INSERT INTO chat_conversations (id) VALUES ('conversation')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO chat_messages (id, conversation_id, role, content, created_at)
         VALUES ('message', 'conversation', 'assistant', '', 'now')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO chat_message_blocks (id, conversation_id, message_id, sequence, block_index, role, kind, status, text, metadata, created_at, updated_at)
         VALUES ('existing', 'conversation', 'message', 1, 0, 'assistant', 'text', 'finalized', 'saved text', '{\"saved\":true}', 'now', 'now')",
        [],
    )
    .unwrap();
    v20260730025727_chat_message_blocks_thinking_kind::migrate(&conn).unwrap();
    conn
}

#[test]
fn preserves_rows_and_enforces_rebuilt_chat_message_block_constraints() {
    let conn = migrated_connection();

    let existing: (String, String, String) = conn
        .query_row(
            "SELECT kind, text, metadata FROM chat_message_blocks WHERE id = 'existing'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        existing,
        (
            "text".into(),
            "saved text".into(),
            "{\"saved\":true}".into()
        )
    );

    conn.execute(
        "INSERT INTO chat_message_blocks (id, conversation_id, message_id, sequence, block_index, role, kind, status, created_at, updated_at)
         VALUES ('thinking', 'conversation', 'message', 2, 1, 'assistant', 'thinking', 'finalized', 'now', 'now')",
        [],
    )
    .unwrap();
    assert!(conn.execute(
        "INSERT INTO chat_message_blocks (id, conversation_id, sequence, block_index, role, kind, status, created_at, updated_at)
         VALUES ('bogus', 'conversation', 3, 2, 'assistant', 'bogus', 'finalized', 'now', 'now')", [],).is_err());
    assert!(conn.execute(
        "INSERT INTO chat_message_blocks (id, conversation_id, sequence, block_index, role, kind, status, created_at, updated_at)
         VALUES ('duplicate-sequence', 'conversation', 2, 2, 'assistant', 'text', 'finalized', 'now', 'now')", [],).is_err());
    assert!(conn.execute(
        "INSERT INTO chat_message_blocks (id, conversation_id, message_id, sequence, block_index, role, kind, status, created_at, updated_at)
         VALUES ('duplicate-index', 'conversation', 'message', 3, 1, 'assistant', 'text', 'finalized', 'now', 'now')", [],).is_err());
    assert!(conn.execute(
        "INSERT INTO chat_message_block_payloads (block_id, updated_at) VALUES ('missing', 'now')", [],).is_err());
}

/// A table rebuild drops the old table's indices with it; forgetting to
/// recreate one degrades every timeline page read to a scan without failing
/// anything visibly.
#[test]
fn rebuild_recreates_every_chat_message_block_index() {
    let conn = migrated_connection();

    let mut stmt = conn
        .prepare(
            "SELECT name FROM sqlite_master
             WHERE type = 'index' AND tbl_name = 'chat_message_blocks' AND name LIKE 'idx_%'
             ORDER BY name",
        )
        .unwrap();
    let indices: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();

    assert_eq!(
        indices,
        vec![
            "idx_chat_message_blocks_conversation_sequence".to_string(),
            "idx_chat_message_blocks_message".to_string(),
            "idx_chat_message_blocks_tool_call".to_string(),
        ]
    );
}
