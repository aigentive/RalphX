//! Tests for migration v20260619093000: PR review artifact monitor fields.

use rusqlite::Connection;

use super::helpers;
use super::v20260618123000_agent_workspace_pr_review_monitoring;
use super::v20260619093000_agent_workspace_pr_review_artifacts;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE agent_conversation_workspaces (
            conversation_id TEXT PRIMARY KEY
        );",
    )
    .expect("create workspace table");
    v20260618123000_agent_workspace_pr_review_monitoring::migrate(&conn).unwrap();
    conn
}

#[test]
fn pr_review_artifact_columns_are_added() {
    let conn = setup_test_db();

    v20260619093000_agent_workspace_pr_review_artifacts::migrate(&conn).unwrap();

    assert!(helpers::column_exists(
        &conn,
        "agent_workspace_pr_review_monitors",
        "review_artifact_id"
    ));
    assert!(helpers::column_exists(
        &conn,
        "agent_workspace_pr_review_monitors",
        "review_artifact_version"
    ));
    assert!(helpers::column_exists(
        &conn,
        "agent_workspace_pr_review_monitors",
        "review_artifact_head_sha"
    ));
    assert!(helpers::column_exists(
        &conn,
        "agent_workspace_pr_review_monitors",
        "review_artifact_updated_at"
    ));
    assert!(helpers::index_exists(
        &conn,
        "idx_agent_workspace_pr_review_monitors_artifact"
    ));
}

#[test]
fn pr_review_artifact_migration_is_idempotent() {
    let conn = setup_test_db();

    v20260619093000_agent_workspace_pr_review_artifacts::migrate(&conn).unwrap();
    v20260619093000_agent_workspace_pr_review_artifacts::migrate(&conn).unwrap();

    assert!(helpers::column_exists(
        &conn,
        "agent_workspace_pr_review_monitors",
        "review_artifact_id"
    ));
}
