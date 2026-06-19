use rusqlite::Connection;

use super::helpers::{index_exists, table_exists};
use super::v20260619144000_durable_queued_messages;

#[test]
fn creates_queued_messages_table_and_indexes() {
    let conn = Connection::open_in_memory().expect("create in-memory db");

    v20260619144000_durable_queued_messages::migrate(&conn).unwrap();

    assert!(table_exists(&conn, "queued_messages"));
    assert!(index_exists(&conn, "idx_queued_messages_context_order"));
    assert!(index_exists(&conn, "idx_queued_messages_context"));
    assert!(index_exists(&conn, "idx_queued_messages_updated_at"));
}

#[test]
fn migration_is_idempotent() {
    let conn = Connection::open_in_memory().expect("create in-memory db");

    v20260619144000_durable_queued_messages::migrate(&conn).unwrap();
    v20260619144000_durable_queued_messages::migrate(&conn).unwrap();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'queued_messages'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}
