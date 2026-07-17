//! Tests for migration v20260716202015: workspace review bypass and bound agent

use rusqlite::Connection;

use super::v20260716202015_workspace_review_bypass_and_bound_agent;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE agent_workspace_review_monitors (
            conversation_id TEXT PRIMARY KEY
         );
         CREATE TABLE chat_conversations (
            id TEXT PRIMARY KEY
         );
         INSERT INTO agent_workspace_review_monitors (conversation_id) VALUES ('review-1');
         INSERT INTO chat_conversations (id) VALUES ('chat-1');",
    )
    .expect("seed legacy tables");
    conn
}

#[test]
fn migration_adds_nullable_review_bypass_and_bound_agent_fields() {
    let conn = setup_test_db();
    v20260716202015_workspace_review_bypass_and_bound_agent::migrate(&conn).unwrap();

    let bypass: (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<i64>,
    ) = conn
        .query_row(
            "SELECT review_gate_bypassed_at,
                    review_gate_bypassed_target_scope,
                    review_gate_bypassed_diff_fingerprint,
                    review_gate_bypassed_artifact_id,
                    review_gate_bypassed_artifact_version
             FROM agent_workspace_review_monitors
             WHERE conversation_id = 'review-1'",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(bypass, (None, None, None, None, None));
    let bound_agent: Option<String> = conn
        .query_row(
            "SELECT bound_agent_name FROM chat_conversations WHERE id = 'chat-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(bound_agent, None);

    v20260716202015_workspace_review_bypass_and_bound_agent::migrate(&conn).unwrap();
}
