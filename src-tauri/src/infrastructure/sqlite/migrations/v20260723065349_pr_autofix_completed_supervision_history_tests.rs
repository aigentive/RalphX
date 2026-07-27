//! Tests for migration v20260723065349: pr autofix completed supervision history
//!
//! Since PR #854, `pr_autofix_completed` events are appended in the same
//! transaction that moves `pr_supervision_status` to `reviewing`, `publishing`,
//! or `paused` (never directly to `monitoring`). The v20260522090000
//! publication-event trigger still derived a `monitoring` supervision row from
//! that step, producing contradictory state history alongside the correct
//! `workspace_snapshot` row. This migration recreates the trigger without that
//! arm; all other step mappings must survive unchanged.

use rusqlite::Connection;

use super::v20260522090000_agent_workspace_state_history;
use super::v20260723065349_pr_autofix_completed_supervision_history;

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
            'conversation-1', 'open', 'needs_agent', 'fixing',
            '2026-07-22T10:00:00+00:00', 'active',
            '2026-07-22T09:00:00+00:00', '2026-07-22T10:00:00+00:00'
        );",
    )
    .expect("create test schema");
    v20260522090000_agent_workspace_state_history::migrate(&conn)
        .expect("run state history migration");
    conn
}

fn insert_event(conn: &Connection, id: &str, step: &str, status: &str) {
    conn.execute(
        "INSERT INTO agent_conversation_workspace_publication_events (
            id, conversation_id, step, status, summary, classification, created_at
         )
         VALUES (?1, 'conversation-1', ?2, ?3, 'summary', NULL, '2026-07-22T12:00:00+00:00')",
        rusqlite::params![id, step, status],
    )
    .unwrap();
}

fn supervision_event_history_count(conn: &Connection, to_state: &str) -> i64 {
    conn.query_row(
        "SELECT COUNT(*)
         FROM agent_conversation_workspace_state_history
         WHERE state_family = 'pr_supervision_status'
           AND to_state = ?1
           AND source = 'publication_event'",
        rusqlite::params![to_state],
        |row| row.get(0),
    )
    .unwrap()
}

#[test]
fn pr_autofix_completed_no_longer_derives_monitoring_supervision_history() {
    let conn = setup_test_db();
    v20260723065349_pr_autofix_completed_supervision_history::migrate(&conn).unwrap();

    insert_event(
        &conn,
        "event-completed",
        "pr_autofix_completed",
        "succeeded",
    );

    assert_eq!(supervision_event_history_count(&conn, "monitoring"), 0);
}

#[test]
fn other_supervision_step_mappings_survive_trigger_recreation() {
    let conn = setup_test_db();
    v20260723065349_pr_autofix_completed_supervision_history::migrate(&conn).unwrap();

    insert_event(
        &conn,
        "event-recovered",
        "pr_supervision_recovered",
        "succeeded",
    );
    insert_event(&conn, "event-repair", "repair_completed", "succeeded");
    insert_event(&conn, "event-requested", "repair_requested", "started");
    insert_event(&conn, "event-sent-failed", "repair_sent", "failed");

    assert_eq!(supervision_event_history_count(&conn, "monitoring"), 2);
    assert_eq!(supervision_event_history_count(&conn, "fixing"), 1);
    assert_eq!(supervision_event_history_count(&conn, "blocked"), 1);
}

#[test]
fn pr_and_push_status_event_mappings_survive_trigger_recreation() {
    let conn = setup_test_db();
    v20260723065349_pr_autofix_completed_supervision_history::migrate(&conn).unwrap();

    insert_event(&conn, "event-merged", "pr_merged", "succeeded");
    insert_event(&conn, "event-published", "published", "succeeded");

    let pr_merged: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM agent_conversation_workspace_state_history
             WHERE state_family = 'publication_pr_status'
               AND to_state = 'merged' AND source = 'publication_event'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let pushed: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM agent_conversation_workspace_state_history
             WHERE state_family = 'publication_push_status'
               AND to_state = 'pushed' AND source = 'publication_event'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(pr_merged, 1);
    assert_eq!(pushed, 1);
}

#[test]
fn workspace_snapshot_triggers_remain_untouched() {
    let conn = setup_test_db();
    v20260723065349_pr_autofix_completed_supervision_history::migrate(&conn).unwrap();

    conn.execute(
        "UPDATE agent_conversation_workspaces
         SET pr_supervision_status = 'reviewing',
             pr_supervision_updated_at = '2026-07-22T13:00:00+00:00',
             updated_at = '2026-07-22T13:00:00+00:00'
         WHERE conversation_id = 'conversation-1'",
        [],
    )
    .unwrap();

    let snapshot_reviewing: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM agent_conversation_workspace_state_history
             WHERE state_family = 'pr_supervision_status'
               AND to_state = 'reviewing' AND source = 'workspace_snapshot'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(snapshot_reviewing, 1);
}

#[test]
fn migration_is_idempotent() {
    let conn = setup_test_db();

    v20260723065349_pr_autofix_completed_supervision_history::migrate(&conn).unwrap();
    v20260723065349_pr_autofix_completed_supervision_history::migrate(&conn).unwrap();

    insert_event(
        &conn,
        "event-completed",
        "pr_autofix_completed",
        "succeeded",
    );
    insert_event(
        &conn,
        "event-recovered",
        "pr_supervision_recovered",
        "succeeded",
    );

    assert_eq!(supervision_event_history_count(&conn, "monitoring"), 1);
}
