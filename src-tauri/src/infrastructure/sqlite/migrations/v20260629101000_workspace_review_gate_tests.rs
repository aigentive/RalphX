use rusqlite::Connection;

use super::{v20260622103000_agent_workspace_reviews, v20260629101000_workspace_review_gate};

#[test]
fn workspace_review_gate_columns_and_indexes_are_added() {
    let conn = Connection::open_in_memory().unwrap();

    v20260622103000_agent_workspace_reviews::migrate(&conn).unwrap();
    v20260629101000_workspace_review_gate::migrate(&conn).unwrap();

    for column in [
        "review_outcome",
        "review_gate_status",
        "review_blocking_summary",
        "review_blocking_fingerprint",
        "review_fixer_run_id",
        "review_fixer_conversation_id",
        "review_fixer_status",
    ] {
        let column_exists: i64 = conn
            .query_row(
                "SELECT COUNT(*)
                 FROM pragma_table_info('agent_workspace_review_monitors')
                 WHERE name = ?1",
                [column],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(column_exists, 1, "missing column {column}");
    }

    for index in [
        "idx_agent_workspace_review_monitors_gate",
        "idx_agent_workspace_review_monitors_blocking_fingerprint",
    ] {
        let index_exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'index'
                   AND name = ?1",
                [index],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(index_exists, 1, "missing index {index}");
    }
}

#[test]
fn workspace_review_gate_migration_is_idempotent() {
    let conn = Connection::open_in_memory().unwrap();

    v20260622103000_agent_workspace_reviews::migrate(&conn).unwrap();
    v20260629101000_workspace_review_gate::migrate(&conn).unwrap();
    v20260629101000_workspace_review_gate::migrate(&conn).unwrap();

    let outcome_column_count: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM pragma_table_info('agent_workspace_review_monitors')
             WHERE name = 'review_outcome'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(outcome_column_count, 1);
}
