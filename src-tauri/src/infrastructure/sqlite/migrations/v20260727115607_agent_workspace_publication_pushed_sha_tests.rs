//! Tests for migration v20260727115607: agent workspace publication pushed sha

use rusqlite::Connection;

use super::helpers;
use super::v20260727115607_agent_workspace_publication_pushed_sha;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE agent_conversation_workspaces (
            conversation_id TEXT PRIMARY KEY,
            publication_push_status TEXT
        );
        INSERT INTO agent_conversation_workspaces (
            conversation_id, publication_push_status
        ) VALUES (
            'conversation-existing', 'pushed'
        );",
    )
    .unwrap();
    conn
}

#[test]
fn migration_adds_nullable_publication_pushed_sha_without_rewriting_publication_state() {
    let conn = setup_test_db();
    v20260727115607_agent_workspace_publication_pushed_sha::migrate(&conn).unwrap();

    assert!(helpers::column_exists(
        &conn,
        "agent_conversation_workspaces",
        "publication_pushed_sha"
    ));
    let row: (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT publication_push_status, publication_pushed_sha
             FROM agent_conversation_workspaces
             WHERE conversation_id = 'conversation-existing'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(row, (Some("pushed".to_string()), None));

    v20260727115607_agent_workspace_publication_pushed_sha::migrate(&conn).unwrap();
}
