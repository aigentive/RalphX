use rusqlite::Connection;

use super::{
    v20260622103000_agent_workspace_reviews, v20260628010000_workspace_review_child_conversation,
};

#[test]
fn workspace_review_child_conversation_column_and_index_are_added() {
    let conn = Connection::open_in_memory().unwrap();

    v20260622103000_agent_workspace_reviews::migrate(&conn).unwrap();
    v20260628010000_workspace_review_child_conversation::migrate(&conn).unwrap();

    let column_exists: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM pragma_table_info('agent_workspace_review_monitors')
             WHERE name = 'review_conversation_id'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(column_exists, 1);

    let index_exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'index'
               AND name = 'idx_agent_workspace_review_monitors_review_conversation'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(index_exists, 1);
}

#[test]
fn workspace_review_child_conversation_migration_is_idempotent() {
    let conn = Connection::open_in_memory().unwrap();

    v20260622103000_agent_workspace_reviews::migrate(&conn).unwrap();
    v20260628010000_workspace_review_child_conversation::migrate(&conn).unwrap();
    v20260628010000_workspace_review_child_conversation::migrate(&conn).unwrap();

    let column_count: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM pragma_table_info('agent_workspace_review_monitors')
             WHERE name = 'review_conversation_id'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(column_count, 1);
}
