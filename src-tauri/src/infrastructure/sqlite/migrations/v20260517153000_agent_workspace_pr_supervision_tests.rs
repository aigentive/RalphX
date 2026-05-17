//! Tests for migration v20260517153000: agent workspace PR supervision

use rusqlite::Connection;

use super::helpers;
use super::v20260517153000_agent_workspace_pr_supervision;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("create in-memory db");
    conn.execute_batch(
        "CREATE TABLE agent_conversation_workspaces (
            conversation_id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            publication_pr_number INTEGER NULL
        );",
    )
    .expect("create test schema");
    conn
}

#[test]
fn agent_workspace_pr_supervision_columns_are_added() {
    let conn = setup_test_db();

    v20260517153000_agent_workspace_pr_supervision::migrate(&conn).unwrap();

    for column in [
        "pr_autofix_enabled",
        "pr_auto_merge_desired",
        "pr_auto_merge_method",
        "pr_auto_merge_current",
        "pr_supervision_status",
        "pr_supervision_summary",
        "pr_supervision_updated_at",
    ] {
        assert!(helpers::column_exists(
            &conn,
            "agent_conversation_workspaces",
            column
        ));
    }
    assert!(helpers::index_exists(
        &conn,
        "idx_agent_workspaces_pr_supervision"
    ));
}

#[test]
fn agent_workspace_pr_supervision_migration_is_idempotent() {
    let conn = setup_test_db();

    v20260517153000_agent_workspace_pr_supervision::migrate(&conn).unwrap();
    v20260517153000_agent_workspace_pr_supervision::migrate(&conn).unwrap();

    assert!(helpers::column_exists(
        &conn,
        "agent_conversation_workspaces",
        "pr_auto_merge_method"
    ));
}
