//! Tests for migration v20260802215754: add workspace review automation override

use rusqlite::Connection;

use super::{helpers, v20260802215754_add_workspace_review_automation_override};

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("create migration test database");
    conn.execute_batch(
        "CREATE TABLE agent_conversation_workspaces (conversation_id TEXT PRIMARY KEY);",
    )
    .expect("seed legacy workspace table");
    conn
}

#[test]
fn migration_adds_nullable_workspace_review_automation_override() {
    let conn = setup_test_db();
    v20260802215754_add_workspace_review_automation_override::migrate(&conn)
        .expect("migration should succeed");

    assert!(helpers::column_exists(
        &conn,
        "agent_conversation_workspaces",
        "review_automation_override"
    ));
    conn.execute(
        "INSERT INTO agent_conversation_workspaces (conversation_id) VALUES (?1)",
        ["workspace-default-override"],
    )
    .expect("workspace with nullable override should insert");
    let override_value: Option<bool> = conn
        .query_row(
            "SELECT review_automation_override FROM agent_conversation_workspaces WHERE conversation_id = ?1",
            ["workspace-default-override"],
            |row| row.get(0),
        )
        .expect("nullable override should read");
    assert_eq!(override_value, None);
}

#[test]
fn migration_is_idempotent() {
    let conn = setup_test_db();
    v20260802215754_add_workspace_review_automation_override::migrate(&conn)
        .expect("first migration should succeed");
    v20260802215754_add_workspace_review_automation_override::migrate(&conn)
        .expect("second migration should succeed");
}
