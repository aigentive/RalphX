//! Tests for migration v20260618123000: agent workspace PR review monitoring

use rusqlite::Connection;

use super::helpers;
use super::v20260618123000_agent_workspace_pr_review_monitoring;

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
fn pr_review_monitoring_tables_and_indexes_are_added() {
    let conn = setup_test_db();

    v20260618123000_agent_workspace_pr_review_monitoring::migrate(&conn).unwrap();

    assert!(helpers::table_exists(
        &conn,
        "agent_workspace_pr_review_monitors"
    ));
    assert!(helpers::table_exists(
        &conn,
        "agent_workspace_pr_review_actions"
    ));
    assert!(helpers::index_exists(
        &conn,
        "idx_agent_workspace_pr_review_monitors_active"
    ));
    assert!(helpers::index_exists(
        &conn,
        "idx_agent_workspace_pr_review_actions_one_pending_head"
    ));
}

#[test]
fn pr_review_monitoring_migration_is_idempotent() {
    let conn = setup_test_db();

    v20260618123000_agent_workspace_pr_review_monitoring::migrate(&conn).unwrap();
    v20260618123000_agent_workspace_pr_review_monitoring::migrate(&conn).unwrap();

    assert!(helpers::table_exists(
        &conn,
        "agent_workspace_pr_review_monitors"
    ));
    assert!(helpers::table_exists(
        &conn,
        "agent_workspace_pr_review_actions"
    ));
}

#[test]
fn pr_review_actions_allow_only_one_pending_action_per_head() {
    let conn = setup_test_db();
    v20260618123000_agent_workspace_pr_review_monitoring::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO agent_conversation_workspaces (conversation_id) VALUES ('conversation-1')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO agent_workspace_pr_review_actions (
            id, conversation_id, pr_number, head_sha, proposed_action, summary,
            review_body, status, created_at, updated_at
        ) VALUES (
            'action-1', 'conversation-1', 42, 'head-sha', 'request_changes',
            'summary', 'body', 'pending', '2026-06-18T12:30:00Z',
            '2026-06-18T12:30:00Z'
        )",
        [],
    )
    .unwrap();

    let duplicate = conn.execute(
        "INSERT INTO agent_workspace_pr_review_actions (
            id, conversation_id, pr_number, head_sha, proposed_action, summary,
            review_body, status, created_at, updated_at
        ) VALUES (
            'action-2', 'conversation-1', 42, 'head-sha', 'approve',
            'summary', 'body', 'pending', '2026-06-18T12:31:00Z',
            '2026-06-18T12:31:00Z'
        )",
        [],
    );
    assert!(duplicate.is_err());

    conn.execute(
        "UPDATE agent_workspace_pr_review_actions
         SET status = 'skipped', resolved_at = '2026-06-18T12:32:00Z'
         WHERE id = 'action-1'",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO agent_workspace_pr_review_actions (
            id, conversation_id, pr_number, head_sha, proposed_action, summary,
            review_body, status, created_at, updated_at
        ) VALUES (
            'action-2', 'conversation-1', 42, 'head-sha', 'approve',
            'summary', 'body', 'pending', '2026-06-18T12:33:00Z',
            '2026-06-18T12:33:00Z'
        )",
        [],
    )
    .unwrap();
}
