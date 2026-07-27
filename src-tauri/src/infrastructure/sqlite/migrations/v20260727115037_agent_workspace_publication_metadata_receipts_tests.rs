//! Tests for migration v20260727115037: agent workspace publication metadata receipts

use rusqlite::Connection;

use super::helpers;
use super::v20260727115037_agent_workspace_publication_metadata_receipts;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE agent_conversation_workspaces (conversation_id TEXT PRIMARY KEY);
         CREATE TABLE agent_conversation_workspace_publication_events (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL,
            step TEXT NOT NULL,
            status TEXT NOT NULL,
            summary TEXT NOT NULL,
            classification TEXT,
            created_at TEXT NOT NULL
         );
         INSERT INTO agent_conversation_workspace_publication_events (
            id, conversation_id, step, status, summary, classification, created_at
         ) VALUES (
            'legacy-event', 'conversation-legacy', 'published', 'succeeded',
            'Legacy publication', NULL, '2026-07-27T11:00:00+00:00'
         );",
    )
    .unwrap();
    conn
}

#[test]
fn migration_adds_nullable_receipt_and_event_attempt_columns_idempotently() {
    let conn = setup_test_db();
    v20260727115037_agent_workspace_publication_metadata_receipts::migrate(&conn).unwrap();
    v20260727115037_agent_workspace_publication_metadata_receipts::migrate(&conn).unwrap();

    assert!(helpers::column_exists(
        &conn,
        "agent_conversation_workspaces",
        "publication_metadata_phase"
    ));
    assert!(helpers::column_exists(
        &conn,
        "agent_conversation_workspaces",
        "publication_metadata_state"
    ));
    assert!(helpers::column_exists(
        &conn,
        "agent_conversation_workspaces",
        "publication_metadata_attempt_id"
    ));
    for column in [
        "publication_metadata_target_pr_number",
        "publication_metadata_before_authority_sha256",
        "publication_metadata_before_title_sha256",
        "publication_metadata_before_editable_body_sha256",
        "publication_metadata_before_managed_suffix_sha256",
        "publication_metadata_intended_title_sha256",
        "publication_metadata_intended_editable_body_sha256",
        "publication_metadata_updated_at",
    ] {
        assert!(
            helpers::column_exists(&conn, "agent_conversation_workspaces", column),
            "missing receipt authority column {column}"
        );
    }
    assert!(helpers::column_exists(
        &conn,
        "agent_conversation_workspace_publication_events",
        "attempt_id"
    ));
    let legacy_attempt_id: Option<String> = conn
        .query_row(
            "SELECT attempt_id
             FROM agent_conversation_workspace_publication_events
             WHERE id = 'legacy-event'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(legacy_attempt_id, None);
}
