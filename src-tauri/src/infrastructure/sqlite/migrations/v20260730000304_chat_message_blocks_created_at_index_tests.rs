//! Tests for migration v20260730000304: chat message blocks created at index

use rusqlite::Connection;

use super::{
    v20260510185257_chat_message_blocks_timeline,
    v20260730000304_chat_message_blocks_created_at_index,
};

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    v20260510185257_chat_message_blocks_timeline::migrate(&conn)
        .expect("timeline migration must run first to create chat_message_blocks");
    conn
}

#[test]
fn test_migration_creates_created_at_index() {
    let conn = setup_test_db();
    v20260730000304_chat_message_blocks_created_at_index::migrate(&conn).unwrap();

    let index_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_chat_message_blocks_created_at'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(index_count, 1);
}

#[test]
fn test_migration_is_idempotent() {
    let conn = setup_test_db();
    v20260730000304_chat_message_blocks_created_at_index::migrate(&conn).unwrap();
    v20260730000304_chat_message_blocks_created_at_index::migrate(&conn).unwrap();
}
