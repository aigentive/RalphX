//! Tests for migration v20260731111346: purge empty thinking blocks

use rusqlite::Connection;

use super::v20260731111346_purge_empty_thinking_blocks;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE chat_message_blocks (
            id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            text TEXT
         );
         CREATE TABLE chat_message_block_payloads (
            block_id TEXT PRIMARY KEY,
            updated_at TEXT NOT NULL
         );
         INSERT INTO chat_message_blocks (id, kind, text) VALUES
            ('empty', 'thinking', ''),
            ('whitespace', 'thinking', ''),
            ('null', 'thinking', NULL),
            ('kept-thinking', 'thinking', 'reasoning'),
            ('kept-text', 'text', '');
         INSERT INTO chat_message_block_payloads (block_id, updated_at) VALUES
            ('empty', 'now'),
            ('whitespace', 'now'),
            ('null', 'now'),
            ('kept-thinking', 'now');",
    )
    .expect("seed migration fixture");
    conn.execute(
        "UPDATE chat_message_blocks SET text = ?1 WHERE id = 'whitespace'",
        ["  \n\t"],
    )
    .expect("seed whitespace thinking block");
    conn
}

#[test]
fn migration_purges_only_empty_thinking_blocks_and_their_payloads() {
    let conn = setup_test_db();
    v20260731111346_purge_empty_thinking_blocks::migrate(&conn).unwrap();

    let remaining_blocks = conn
        .prepare("SELECT id FROM chat_message_blocks ORDER BY id")
        .expect("prepare blocks")
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query blocks")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect blocks");
    assert_eq!(remaining_blocks, vec!["kept-text", "kept-thinking"]);

    let remaining_payloads = conn
        .prepare("SELECT block_id FROM chat_message_block_payloads ORDER BY block_id")
        .expect("prepare payloads")
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query payloads")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect payloads");
    assert_eq!(remaining_payloads, vec!["kept-thinking"]);
}
