//! Tests for migration v20260527033000: agent workspace auto publish gate

use rusqlite::Connection;

use super::helpers;
use super::v20260527033000_agent_workspace_auto_publish;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("create in-memory db");
    conn.execute_batch(
        "CREATE TABLE agent_conversation_workspaces (
            conversation_id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            publication_pr_number INTEGER NULL,
            publication_push_status TEXT NULL
        );",
    )
    .expect("create test schema");
    conn
}

#[test]
fn agent_workspace_auto_publish_columns_are_added() {
    let conn = setup_test_db();

    v20260527033000_agent_workspace_auto_publish::migrate(&conn).unwrap();

    for column in [
        "auto_publish_enabled",
        "auto_publish_paused_pr_autofix_enabled",
        "auto_publish_paused_pr_auto_merge_desired",
    ] {
        assert!(helpers::column_exists(
            &conn,
            "agent_conversation_workspaces",
            column
        ));
    }
    assert!(helpers::index_exists(
        &conn,
        "idx_agent_workspaces_auto_publish"
    ));
}

#[test]
fn agent_workspace_auto_publish_migration_is_idempotent() {
    let conn = setup_test_db();

    v20260527033000_agent_workspace_auto_publish::migrate(&conn).unwrap();
    v20260527033000_agent_workspace_auto_publish::migrate(&conn).unwrap();

    assert!(helpers::column_exists(
        &conn,
        "agent_conversation_workspaces",
        "auto_publish_enabled"
    ));
}
