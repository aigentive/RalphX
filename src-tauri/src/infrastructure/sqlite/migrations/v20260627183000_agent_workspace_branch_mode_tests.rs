use rusqlite::Connection;

use super::helpers;
use crate::infrastructure::sqlite::connection::open_memory_connection;

fn setup_workspace_table(conn: &Connection) {
    conn.execute(
        "CREATE TABLE agent_conversation_workspaces (
            conversation_id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            branch_name TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'active'
        )",
        [],
    )
    .unwrap();
}

#[test]
fn migration_adds_isolated_branch_mode_default_and_conflict_index() {
    let conn = open_memory_connection().unwrap();
    setup_workspace_table(&conn);

    super::v20260627183000_agent_workspace_branch_mode::migrate(&conn).unwrap();

    assert!(helpers::column_exists(
        &conn,
        "agent_conversation_workspaces",
        "branch_mode"
    ));
    let default_value: String = conn
        .query_row(
            "SELECT dflt_value FROM pragma_table_info('agent_conversation_workspaces')
             WHERE name = 'branch_mode'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(default_value, "'isolated'");
    assert!(helpers::index_exists(
        &conn,
        "idx_agent_conversation_workspaces_project_branch_status"
    ));
}

#[test]
fn migration_is_idempotent() {
    let conn = open_memory_connection().unwrap();
    setup_workspace_table(&conn);

    super::v20260627183000_agent_workspace_branch_mode::migrate(&conn).unwrap();
    super::v20260627183000_agent_workspace_branch_mode::migrate(&conn).unwrap();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('agent_conversation_workspaces')
             WHERE name = 'branch_mode'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}
