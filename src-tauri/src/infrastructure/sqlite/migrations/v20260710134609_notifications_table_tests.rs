//! Tests for migration v20260710134609: notifications table

use rusqlite::Connection;

use super::v20260710134609_notifications_table;

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

#[test]
fn notifications_table_and_unread_index_are_added() {
    let conn = setup_test_db();
    v20260710134609_notifications_table::migrate(&conn).unwrap();
    let columns: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('notifications')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(columns, 10);
    let index_count: i64 = conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_notifications_unread'", [], |row| row.get(0)).unwrap();
    assert_eq!(index_count, 1);
}
