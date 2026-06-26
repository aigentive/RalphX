//! Tests for migration v20260622162352: agent workspace followup provenance

use rusqlite::Connection;

use super::helpers;
use super::v20260622162352_agent_workspace_followup_provenance;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute(
        "CREATE TABLE agent_conversation_workspaces (
            conversation_id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'active',
            updated_at TEXT NOT NULL
        )",
        [],
    )
    .expect("test table should be created");
    conn
}

#[test]
fn migration_adds_followup_provenance_columns_and_index() {
    let conn = setup_test_db();
    v20260622162352_agent_workspace_followup_provenance::migrate(&conn).unwrap();

    for column in [
        "followup_origin_conversation_id",
        "followup_source_task_id",
        "followup_source_context_type",
        "followup_source_context_id",
        "followup_source_agent_name",
        "followup_spawn_reason",
        "followup_blocker_fingerprint",
    ] {
        assert!(
            helpers::column_exists(&conn, "agent_conversation_workspaces", column),
            "missing column {column}"
        );
    }

    assert!(helpers::index_exists(
        &conn,
        "idx_agent_workspaces_followup_blocker"
    ));
}

#[test]
fn migration_is_idempotent_and_supports_lookup_key() {
    let conn = setup_test_db();
    v20260622162352_agent_workspace_followup_provenance::migrate(&conn).unwrap();
    v20260622162352_agent_workspace_followup_provenance::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO agent_conversation_workspaces (
            conversation_id, project_id, status, updated_at,
            followup_origin_conversation_id, followup_source_task_id,
            followup_blocker_fingerprint
        ) VALUES (
            'followup-1', 'project-1', 'active', '2026-06-22T16:00:00+00:00',
            'origin-1', 'task-1', 'scope-drift:task-1:file'
        )",
        [],
    )
    .expect("insert with followup provenance should succeed");

    let found: String = conn
        .query_row(
            "SELECT conversation_id FROM agent_conversation_workspaces
             WHERE followup_origin_conversation_id = ?1
               AND followup_source_task_id = ?2
               AND followup_blocker_fingerprint = ?3
               AND status = 'active'",
            ["origin-1", "task-1", "scope-drift:task-1:file"],
            |row| row.get(0),
        )
        .expect("lookup key should find row");
    assert_eq!(found, "followup-1");
}
