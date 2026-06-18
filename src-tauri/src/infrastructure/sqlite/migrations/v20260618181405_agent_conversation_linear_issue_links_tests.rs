//! Tests for migration v20260618181405: agent conversation linear issue links

use rusqlite::{params, Connection};

use super::helpers::{index_exists, table_exists};
use super::v20260618181405_agent_conversation_linear_issue_links;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE chat_conversations (
            id TEXT PRIMARY KEY,
            context_type TEXT NOT NULL,
            context_id TEXT NOT NULL,
            agent_mode TEXT,
            created_at TEXT NOT NULL DEFAULT '2026-06-18T18:14:05Z',
            updated_at TEXT NOT NULL DEFAULT '2026-06-18T18:14:05Z'
        );
        CREATE TABLE chat_messages (
            id TEXT PRIMARY KEY,
            conversation_id TEXT REFERENCES chat_conversations(id) ON DELETE CASCADE,
            role TEXT NOT NULL,
            content TEXT NOT NULL DEFAULT '',
            metadata TEXT,
            created_at TEXT DEFAULT '2026-06-18T18:14:05Z'
        );
        CREATE TABLE agent_conversation_workspaces (
            conversation_id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL
        );",
    )
    .unwrap();
    conn
}

#[test]
fn creates_table_and_indexes() {
    let conn = setup_test_db();
    v20260618181405_agent_conversation_linear_issue_links::migrate(&conn).unwrap();

    assert!(table_exists(&conn, "agent_conversation_linear_issue_links"));
    assert!(index_exists(
        &conn,
        "idx_agent_conversation_linear_issue_links_project_id"
    ));
    assert!(index_exists(
        &conn,
        "idx_agent_conversation_linear_issue_links_project_key"
    ));
    assert!(index_exists(
        &conn,
        "idx_agent_conversation_linear_issue_links_conversation"
    ));
}

#[test]
fn backfills_earliest_structured_linear_reference() {
    let conn = setup_test_db();
    insert_agent_conversation(&conn, "conv-1", "project-1");
    insert_user_message(
        &conn,
        "conv-1",
        "msg-1",
        r#"{"composer_integration_references":[{"provider":"atlassian","kind":"jira","id":"RX-1","key":"RX-1"}]}"#,
        "2026-06-18T18:10:00Z",
    );
    insert_user_message(
        &conn,
        "conv-1",
        "msg-2",
        r#"{"composer_integration_references":[{"provider":"linear","kind":"linear","id":"539068e2-ae88-4d09-bd75-22eb4a59612f","key":"LIN-123","url":"https://linear.app/acme/issue/LIN-123/example"}]}"#,
        "2026-06-18T18:11:00Z",
    );

    v20260618181405_agent_conversation_linear_issue_links::migrate(&conn).unwrap();

    let row = conn
        .query_row(
            "SELECT project_id, provider, issue_id, issue_key, issue_url, refresh_status,
                    assigned_at, assigned_from_message_id, manually_assigned
             FROM agent_conversation_linear_issue_links WHERE conversation_id = 'conv-1'",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, i64>(8)?,
                ))
            },
        )
        .unwrap();

    assert_eq!(row.0, "project-1");
    assert_eq!(row.1, "linear");
    assert_eq!(row.2, "539068e2-ae88-4d09-bd75-22eb4a59612f");
    assert_eq!(row.3.as_deref(), Some("LIN-123"));
    assert_eq!(
        row.4.as_deref(),
        Some("https://linear.app/acme/issue/LIN-123/example")
    );
    assert_eq!(row.5, "not_loaded");
    assert_eq!(row.6, "2026-06-18T18:11:00Z");
    assert_eq!(row.7.as_deref(), Some("msg-2"));
    assert_eq!(row.8, 0);
}

#[test]
fn ignores_invalid_linear_reference_ids() {
    let conn = setup_test_db();
    insert_agent_conversation(&conn, "conv-1", "project-1");
    insert_user_message(
        &conn,
        "conv-1",
        "msg-1",
        "{\"composer_integration_references\":[{\"provider\":\"linear\",\"kind\":\"linear\",\"id\":\"bad\\nvalue\"}]}",
        "2026-06-18T18:11:00Z",
    );

    v20260618181405_agent_conversation_linear_issue_links::migrate(&conn).unwrap();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM agent_conversation_linear_issue_links",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}

fn insert_agent_conversation(conn: &Connection, id: &str, project_id: &str) {
    conn.execute(
        "INSERT INTO chat_conversations (id, context_type, context_id, agent_mode, created_at, updated_at)
         VALUES (?1, 'project', ?2, 'edit', '2026-06-18T18:14:05Z', '2026-06-18T18:14:05Z')",
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
