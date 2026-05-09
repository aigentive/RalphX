//! Tests for migration v20260506131356: agent workspace pr descriptions

use rusqlite::Connection;

use super::v20260506131356_agent_workspace_pr_descriptions;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE agent_conversation_workspaces (
            conversation_id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            mode TEXT NOT NULL DEFAULT 'edit',
            base_ref_kind TEXT NOT NULL,
            base_ref TEXT NOT NULL,
            branch_name TEXT NOT NULL,
            worktree_path TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );",
    )
    .expect("create agent workspace table");
    conn
}

#[test]
fn test_migration_runs() {
    let conn = setup_test_db();
    v20260506131356_agent_workspace_pr_descriptions::migrate(&conn).unwrap();
    v20260506131356_agent_workspace_pr_descriptions::migrate(&conn).unwrap();

    let mut stmt = conn
        .prepare("PRAGMA table_info(agent_conversation_workspaces)")
        .unwrap();
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert!(columns.contains(&"publication_pr_title".to_string()));
    assert!(columns.contains(&"publication_pr_body".to_string()));
}
