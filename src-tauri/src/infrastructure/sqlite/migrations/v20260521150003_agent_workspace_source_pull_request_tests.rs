//! Tests for migration v20260521150003: agent workspace source pull request

use rusqlite::Connection;

use super::v20260521150003_agent_workspace_source_pull_request;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute(
        "CREATE TABLE agent_conversation_workspaces (
            conversation_id TEXT PRIMARY KEY
        )",
        [],
    )
    .expect("test table should be created");
    conn
}

#[test]
fn test_migration_adds_source_pull_request_columns() {
    let conn = setup_test_db();
    v20260521150003_agent_workspace_source_pull_request::migrate(&conn).unwrap();

    for column in [
        "source_pr_number",
        "source_pr_url",
        "source_pr_title",
        "source_pr_head_ref",
        "source_pr_base_ref",
        "source_pr_head_sha",
    ] {
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('agent_conversation_workspaces') WHERE name = ?1",
                [column],
                |row| row.get(0),
            )
            .expect("column lookup should work");
        assert_eq!(exists, 1, "missing column {column}");
    }
}

#[test]
fn test_migration_is_idempotent() {
    let conn = setup_test_db();
    v20260521150003_agent_workspace_source_pull_request::migrate(&conn).unwrap();
    v20260521150003_agent_workspace_source_pull_request::migrate(&conn).unwrap();
}
