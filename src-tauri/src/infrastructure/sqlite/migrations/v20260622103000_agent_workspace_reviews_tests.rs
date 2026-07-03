use rusqlite::Connection;

use super::v20260622103000_agent_workspace_reviews;

#[test]
fn agent_workspace_review_monitor_table_and_indexes_are_added() {
    let conn = Connection::open_in_memory().unwrap();

    v20260622103000_agent_workspace_reviews::migrate(&conn).unwrap();

    let table_exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name = 'agent_workspace_review_monitors'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(table_exists, 1);

    for index_name in [
        "idx_agent_workspace_review_monitors_status",
        "idx_agent_workspace_review_monitors_artifact",
    ] {
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'index' AND name = ?1",
                [index_name],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(exists, 1, "missing index {index_name}");
    }
}

#[test]
fn agent_workspace_review_monitor_migration_is_idempotent() {
    let conn = Connection::open_in_memory().unwrap();

    v20260622103000_agent_workspace_reviews::migrate(&conn).unwrap();
    v20260622103000_agent_workspace_reviews::migrate(&conn).unwrap();

    let table_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name = 'agent_workspace_review_monitors'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(table_count, 1);
}
