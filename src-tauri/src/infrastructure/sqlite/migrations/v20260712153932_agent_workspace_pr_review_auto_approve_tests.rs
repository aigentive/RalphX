//! Tests for migration v20260712153932: agent workspace pr review auto approve

use rusqlite::Connection;

use super::{helpers, v20260712153932_agent_workspace_pr_review_auto_approve};

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE agent_workspace_pr_review_monitors (
            conversation_id TEXT PRIMARY KEY,
            first_review_completed INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE agent_workspace_pr_review_actions (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL,
            status TEXT NOT NULL
        );",
    )
    .expect("create review monitor schema");
    conn
}

#[test]
fn migration_adds_default_enabled_auto_approval_and_resolution_gate() {
    let conn = setup_test_db();
    v20260712153932_agent_workspace_pr_review_auto_approve::migrate(&conn).unwrap();

    assert!(helpers::column_exists(
        &conn,
        "agent_workspace_pr_review_monitors",
        "auto_approve_enabled"
    ));
    assert!(helpers::column_exists(
        &conn,
        "agent_workspace_pr_review_monitors",
        "first_action_resolved"
    ));
    conn.execute(
        "INSERT INTO agent_workspace_pr_review_monitors (conversation_id) VALUES ('new-review')",
        [],
    )
    .unwrap();
    let defaults: (bool, bool) = conn
        .query_row(
            "SELECT auto_approve_enabled, first_action_resolved
             FROM agent_workspace_pr_review_monitors WHERE conversation_id = 'new-review'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(defaults, (true, false));
}

#[test]
fn migration_marks_existing_resolved_actions_as_eligible() {
    let conn = setup_test_db();
    conn.execute(
        "INSERT INTO agent_workspace_pr_review_monitors (conversation_id) VALUES ('resolved-review')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO agent_workspace_pr_review_actions (id, conversation_id, status)
         VALUES ('action-1', 'resolved-review', 'submitted')",
        [],
    )
    .unwrap();

    v20260712153932_agent_workspace_pr_review_auto_approve::migrate(&conn).unwrap();

    let resolved: bool = conn
        .query_row(
            "SELECT first_action_resolved FROM agent_workspace_pr_review_monitors
             WHERE conversation_id = 'resolved-review'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(resolved);
}

#[test]
fn migration_is_idempotent_and_marks_skipped_actions_as_resolved() {
    let conn = setup_test_db();
    conn.execute(
        "INSERT INTO agent_workspace_pr_review_monitors (conversation_id) VALUES ('skipped-review')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO agent_workspace_pr_review_actions (id, conversation_id, status)
         VALUES ('action-1', 'skipped-review', 'skipped')",
        [],
    )
    .unwrap();

    v20260712153932_agent_workspace_pr_review_auto_approve::migrate(&conn).unwrap();
    v20260712153932_agent_workspace_pr_review_auto_approve::migrate(&conn).unwrap();

    let resolved: bool = conn
        .query_row(
            "SELECT first_action_resolved FROM agent_workspace_pr_review_monitors
             WHERE conversation_id = 'skipped-review'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(resolved);
}
