//! Tests for migration v20260723170404: agent workspace publication pushed sha.

use super::v20260723170404_agent_workspace_publication_pushed_sha;

#[test]
fn migration_adds_nullable_publication_sha_and_preserves_legacy_rows() {
    let conn = rusqlite::Connection::open_in_memory().expect("create memory database");
    conn.execute_batch(
        "CREATE TABLE agent_conversation_workspaces (
            conversation_id TEXT PRIMARY KEY,
            publication_push_status TEXT
        );
        INSERT INTO agent_conversation_workspaces (
            conversation_id, publication_push_status
        ) VALUES ('conversation-1', 'pushed');",
    )
    .expect("seed legacy workspace");

    v20260723170404_agent_workspace_publication_pushed_sha::migrate(&conn)
        .expect("add publication SHA");

    let row: (String, Option<String>) = conn
        .query_row(
            "SELECT publication_push_status, publication_pushed_sha
             FROM agent_conversation_workspaces
             WHERE conversation_id = 'conversation-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read migrated workspace");
    assert_eq!(row, ("pushed".to_string(), None));

    v20260723170404_agent_workspace_publication_pushed_sha::migrate(&conn)
        .expect("migration is idempotent");
}
