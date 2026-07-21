//! Tests for migration v20260714184430: workspace review auto merge guard

use rusqlite::Connection;

use super::{
    v20260622103000_agent_workspace_reviews, v20260629101000_workspace_review_gate,
    v20260714184430_workspace_review_auto_merge_guard,
};

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

#[test]
fn test_migration_runs() {
    let conn = setup_test_db();
    v20260622103000_agent_workspace_reviews::migrate(&conn).unwrap();
    v20260629101000_workspace_review_gate::migrate(&conn).unwrap();
    v20260714184430_workspace_review_auto_merge_guard::migrate(&conn).unwrap();

    let columns = conn
        .prepare("SELECT name FROM pragma_table_info('agent_workspace_review_monitors')")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    for column in [
        "auto_merge_guard_status",
        "auto_merge_guard_pr_number",
        "auto_merge_guard_method",
        "auto_merge_guard_target_scope",
        "auto_merge_guard_diff_fingerprint",
        "auto_merge_guard_head_sha",
        "auto_merge_guard_last_error",
    ] {
        assert!(
            columns.contains(&column.to_string()),
            "missing column {column}"
        );
    }
}
