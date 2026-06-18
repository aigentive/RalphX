//! Tests for migration v20260617122430: agent workspace initial auto publish

use rusqlite::Connection;

use super::helpers;
use super::v20260617122430_agent_workspace_initial_auto_publish;

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

#[test]
fn test_migration_runs() {
    let conn = setup_test_db();
    conn.execute(
        "CREATE TABLE agent_conversation_workspaces (
            conversation_id TEXT PRIMARY KEY
        )",
        [],
    )
    .unwrap();

    v20260617122430_agent_workspace_initial_auto_publish::migrate(&conn).unwrap();

    assert!(helpers::column_exists(
        &conn,
        "agent_conversation_workspaces",
        "auto_publish_initial_pr_enabled"
    ));

    let default_value: Option<String> = conn
        .query_row(
            "SELECT dflt_value FROM pragma_table_info('agent_conversation_workspaces')
             WHERE name = 'auto_publish_initial_pr_enabled'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(default_value.as_deref(), Some("0"));
}
