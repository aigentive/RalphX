use rusqlite::Connection;

use super::helpers;
use super::v20260522090000_agent_workspace_state_history;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("create in-memory database");
    conn.execute_batch(
        "CREATE TABLE agent_conversation_workspaces (
            conversation_id TEXT PRIMARY KEY,
            publication_pr_status TEXT NULL,
            publication_push_status TEXT NULL,
            pr_supervision_status TEXT NULL,
            pr_supervision_updated_at TEXT NULL,
            status TEXT NOT NULL DEFAULT 'active',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE agent_conversation_workspace_publication_events (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL,
            step TEXT NOT NULL,
            status TEXT NOT NULL,
            summary TEXT NOT NULL,
            classification TEXT NULL,
            created_at TEXT NOT NULL
        );

        INSERT INTO agent_conversation_workspaces (
            conversation_id, publication_pr_status, publication_push_status,
            pr_supervision_status, pr_supervision_updated_at, status, created_at, updated_at
        )
        VALUES (
            'conversation-1', 'open', 'pushed', 'monitoring',
            '2026-05-20T10:00:00+00:00', 'active',
            '2026-05-20T09:00:00+00:00', '2026-05-20T10:00:00+00:00'
        );

        INSERT INTO agent_conversation_workspace_publication_events (
            id, conversation_id, step, status, summary, classification, created_at
        )
        VALUES
            ('event-merged', 'conversation-1', 'pr_merged', 'succeeded', 'merged', NULL, '2026-05-20T12:00:00+00:00'),
            ('event-autofix', 'conversation-1', 'pr_autofix', 'needs_agent', 'fix needed', 'agent_fixable', '2026-05-20T11:00:00+00:00');",
    )
    .expect("create test schema");
    conn
}

fn history_count(conn: &Connection, family: &str, to_state: &str, source: &str) -> i64 {
    conn.query_row(
        "SELECT COUNT(*)
         FROM agent_conversation_workspace_state_history
         WHERE state_family = ?1 AND to_state = ?2 AND source = ?3",
        rusqlite::params![family, to_state, source],
        |row| row.get(0),
    )
    .unwrap()
}

#[test]
fn workspace_state_history_table_indexes_and_backfill_are_added() {
    let conn = setup_test_db();

    v20260522090000_agent_workspace_state_history::migrate(&conn).unwrap();

    assert!(helpers::table_exists(
        &conn,
        "agent_conversation_workspace_state_history"
    ));
    assert!(helpers::index_exists(
        &conn,
        "idx_agent_workspace_state_history_conversation"
    ));
    assert!(helpers::index_exists(
        &conn,
        "idx_agent_workspace_state_history_family_state"
    ));

    assert_eq!(
        history_count(
            &conn,
            "workspace_status",
            "active",
            "workspace_snapshot_backfill"
        ),
        1
    );
    assert_eq!(
        history_count(
            &conn,
            "publication_pr_status",
            "merged",
            "publication_event_backfill"
        ),
        1
    );
    assert_eq!(
        history_count(
            &conn,
            "pr_supervision_status",
            "fixing",
            "publication_event_backfill"
        ),
        1
    );
}

#[test]
fn workspace_state_history_triggers_capture_future_events_and_snapshot_updates() {
    let conn = setup_test_db();
    v20260522090000_agent_workspace_state_history::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO agent_conversation_workspace_publication_events (
            id, conversation_id, step, status, summary, classification, created_at
         )
         VALUES ('event-published', 'conversation-1', 'published', 'succeeded', 'published', NULL, '2026-05-20T13:00:00+00:00')",
        [],
    )
    .unwrap();
    conn.execute(
        "UPDATE agent_conversation_workspaces
         SET publication_push_status = 'needs_agent',
             updated_at = '2026-05-20T14:00:00+00:00'
         WHERE conversation_id = 'conversation-1'",
        [],
    )
    .unwrap();

    assert_eq!(
        history_count(
            &conn,
            "publication_push_status",
            "pushed",
            "publication_event"
        ),
        1
    );
    assert_eq!(
        history_count(
            &conn,
            "publication_push_status",
            "needs_agent",
            "workspace_snapshot"
        ),
        1
    );
}

#[test]
fn workspace_state_history_migration_is_idempotent() {
    let conn = setup_test_db();

    v20260522090000_agent_workspace_state_history::migrate(&conn).unwrap();
    v20260522090000_agent_workspace_state_history::migrate(&conn).unwrap();

    assert!(helpers::table_exists(
        &conn,
        "agent_conversation_workspace_state_history"
    ));
    assert_eq!(
        history_count(
            &conn,
            "publication_pr_status",
            "merged",
            "publication_event_backfill"
        ),
        1
    );
}
