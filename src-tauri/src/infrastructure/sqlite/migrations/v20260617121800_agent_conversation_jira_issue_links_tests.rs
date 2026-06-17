//! Tests for migration v20260617121800: agent conversation jira issue links

use rusqlite::{params, Connection};

use super::helpers::{index_exists, table_exists};
use super::v20260617121800_agent_conversation_jira_issue_links;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE chat_conversations (
            id TEXT PRIMARY KEY,
            context_type TEXT NOT NULL,
            context_id TEXT NOT NULL,
            agent_mode TEXT,
            created_at TEXT NOT NULL DEFAULT '2026-06-17T12:00:00Z',
            updated_at TEXT NOT NULL DEFAULT '2026-06-17T12:00:00Z'
        );
        CREATE TABLE chat_messages (
            id TEXT PRIMARY KEY,
            conversation_id TEXT REFERENCES chat_conversations(id) ON DELETE CASCADE,
            role TEXT NOT NULL,
            content TEXT NOT NULL DEFAULT '',
            metadata TEXT,
            created_at TEXT DEFAULT '2026-06-17T12:00:00Z'
        );
        CREATE TABLE agent_conversation_workspaces (
            conversation_id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL
        );",
    )
    .unwrap();
    conn
}

fn insert_agent_conversation(conn: &Connection, id: &str, project_id: &str) {
    conn.execute(
        "INSERT INTO chat_conversations (id, context_type, context_id, agent_mode, created_at, updated_at)
         VALUES (?1, 'project', ?2, 'edit', '2026-06-17T12:00:00Z', '2026-06-17T12:00:00Z')",
        params![id, project_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO agent_conversation_workspaces (conversation_id, project_id)
         VALUES (?1, ?2)",
        params![id, project_id],
    )
    .unwrap();
}

fn insert_user_message(
    conn: &Connection,
    conversation_id: &str,
    id: &str,
    metadata: &str,
    at: &str,
) {
    conn.execute(
        "INSERT INTO chat_messages (id, conversation_id, role, metadata, created_at)
         VALUES (?1, ?2, 'user', ?3, ?4)",
        params![id, conversation_id, metadata, at],
    )
    .unwrap();
}

#[test]
fn creates_table_and_indexes() {
    let conn = setup_test_db();

    v20260617121800_agent_conversation_jira_issue_links::migrate(&conn).unwrap();

    assert!(table_exists(&conn, "agent_conversation_jira_issue_links"));
    assert!(index_exists(
        &conn,
        "idx_agent_conversation_jira_issue_links_project_key"
    ));
}

#[test]
fn backfills_earliest_structured_jira_reference() {
    let conn = setup_test_db();
    insert_agent_conversation(&conn, "conv-1", "project-1");
    insert_user_message(
        &conn,
        "conv-1",
        "msg-later",
        r#"{"composer_integration_references":[{"provider":"atlassian","kind":"jira","id":"RX-77"}]}"#,
        "2026-06-17T12:02:00Z",
    );
    insert_user_message(
        &conn,
        "conv-1",
        "msg-first",
        r#"{"composer_integration_references":[{"provider":"atlassian","kind":"jira","id":"rx-42","title":"Fix composer","url":"https://jira.test/browse/RX-42"}]}"#,
        "2026-06-17T12:01:00Z",
    );

    v20260617121800_agent_conversation_jira_issue_links::migrate(&conn).unwrap();

    let row: (String, String, String, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT issue_key, assigned_from_message_id, assigned_at, title, issue_url
             FROM agent_conversation_jira_issue_links WHERE conversation_id = 'conv-1'",
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
    assert_eq!(
        row,
        (
            "RX-42".to_string(),
            "msg-first".to_string(),
            "2026-06-17T12:01:00Z".to_string(),
            Some("Fix composer".to_string()),
            Some("https://jira.test/browse/RX-42".to_string())
        )
    );
}

#[test]
fn backfill_uses_first_jira_reference_inside_metadata() {
    let conn = setup_test_db();
    insert_agent_conversation(&conn, "conv-1", "project-1");
    insert_user_message(
        &conn,
        "conv-1",
        "msg-first",
        r#"{"composer_integration_references":[{"provider":"atlassian","kind":"confluence","id":"abc"},{"provider":"atlassian","kind":"jira","id":"RX-42"},{"provider":"atlassian","kind":"jira","id":"RX-77"}]}"#,
        "2026-06-17T12:01:00Z",
    );

    v20260617121800_agent_conversation_jira_issue_links::migrate(&conn).unwrap();

    let issue_key: String = conn
        .query_row(
            "SELECT issue_key FROM agent_conversation_jira_issue_links WHERE conversation_id = 'conv-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(issue_key, "RX-42");
}

#[test]
fn backfill_is_idempotent_and_does_not_overwrite_existing_link() {
    let conn = setup_test_db();
    insert_agent_conversation(&conn, "conv-1", "project-1");
    insert_user_message(
        &conn,
        "conv-1",
        "msg-first",
        r#"{"composer_integration_references":[{"provider":"atlassian","kind":"jira","id":"RX-42"}]}"#,
        "2026-06-17T12:01:00Z",
    );

    v20260617121800_agent_conversation_jira_issue_links::migrate(&conn).unwrap();
    conn.execute(
        "UPDATE agent_conversation_jira_issue_links
         SET issue_key = 'RX-99', manually_assigned = 1
         WHERE conversation_id = 'conv-1'",
        [],
    )
    .unwrap();
    v20260617121800_agent_conversation_jira_issue_links::migrate(&conn).unwrap();

    let row: (String, i64) = conn
        .query_row(
            "SELECT issue_key, manually_assigned
             FROM agent_conversation_jira_issue_links WHERE conversation_id = 'conv-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(row, ("RX-99".to_string(), 1));
}

#[test]
fn non_agent_conversation_is_not_backfilled() {
    let conn = setup_test_db();
    conn.execute(
        "INSERT INTO chat_conversations (id, context_type, context_id, agent_mode)
         VALUES ('conv-1', 'project', 'project-1', NULL)",
        [],
    )
    .unwrap();
    insert_user_message(
        &conn,
        "conv-1",
        "msg-first",
        r#"{"composer_integration_references":[{"provider":"atlassian","kind":"jira","id":"RX-42"}]}"#,
        "2026-06-17T12:01:00Z",
    );

    v20260617121800_agent_conversation_jira_issue_links::migrate(&conn).unwrap();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM agent_conversation_jira_issue_links",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}
