use rusqlite::Connection;

use super::v20260723012559_agent_workspace_pr_metadata_decision;

#[test]
fn migration_adds_nullable_metadata_decision_column_idempotently() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE agent_conversation_workspaces (conversation_id TEXT PRIMARY KEY);",
    )
    .unwrap();
    v20260723012559_agent_workspace_pr_metadata_decision::migrate(&conn).unwrap();
    v20260723012559_agent_workspace_pr_metadata_decision::migrate(&conn).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('agent_conversation_workspaces') WHERE name = 'publication_pr_metadata_decision'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}
