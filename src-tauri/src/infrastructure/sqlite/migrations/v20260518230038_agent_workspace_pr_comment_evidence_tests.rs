//! Tests for migration v20260518230038: agent workspace pr comment evidence

use rusqlite::Connection;

use super::helpers;
use super::v20260518230038_agent_workspace_pr_comment_evidence;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE agent_conversation_workspaces (
            conversation_id TEXT PRIMARY KEY
        );",
    )
    .expect("create workspace table");
    conn
}

#[test]
fn agent_workspace_pr_comment_evidence_table_and_indexes_are_added() {
    let conn = setup_test_db();

    v20260518230038_agent_workspace_pr_comment_evidence::migrate(&conn).unwrap();

    assert!(helpers::table_exists(
        &conn,
        "agent_workspace_pr_comment_evidence"
    ));
    assert!(helpers::index_exists(
        &conn,
        "idx_agent_workspace_pr_comment_evidence_workspace_pr"
    ));
    assert!(helpers::index_exists(
        &conn,
        "idx_agent_workspace_pr_comment_evidence_seen"
    ));
}

#[test]
fn agent_workspace_pr_comment_evidence_migration_is_idempotent() {
    let conn = setup_test_db();

    v20260518230038_agent_workspace_pr_comment_evidence::migrate(&conn).unwrap();
    v20260518230038_agent_workspace_pr_comment_evidence::migrate(&conn).unwrap();

    assert!(helpers::table_exists(
        &conn,
        "agent_workspace_pr_comment_evidence"
    ));
}
