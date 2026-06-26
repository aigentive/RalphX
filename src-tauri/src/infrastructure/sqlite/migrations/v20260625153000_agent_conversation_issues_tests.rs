//! Tests for migration v20260625153000: Agent conversation issues and autonomy policy

use rusqlite::Connection;

use super::helpers;
use super::v20260625153000_agent_conversation_issues;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute(
        "CREATE TABLE review_settings (
            id INTEGER PRIMARY KEY DEFAULT 1 CHECK (id = 1),
            updated_at TEXT NOT NULL
        )",
        [],
    )
    .expect("review settings table should be created");
    conn.execute(
        "INSERT INTO review_settings (id, updated_at)
         VALUES (1, '2026-06-25T15:30:00+00:00')",
        [],
    )
    .expect("review settings row should be seeded");
    conn
}

#[test]
fn migration_adds_issue_table_indexes_and_autonomy_setting() {
    let conn = setup_test_db();
    v20260625153000_agent_conversation_issues::migrate(&conn).unwrap();

    assert!(helpers::table_exists(&conn, "agent_conversation_issues"));
    assert!(helpers::column_exists(
        &conn,
        "review_settings",
        "auto_create_followup_agent_conversation"
    ));
    assert!(helpers::index_exists(
        &conn,
        "idx_agent_conversation_issues_conversation_status"
    ));
    assert!(helpers::index_exists(
        &conn,
        "idx_agent_conversation_issues_project_status"
    ));
    assert!(helpers::index_exists(
        &conn,
        "idx_agent_conversation_issues_fingerprint"
    ));

    let enabled: i64 = conn
        .query_row(
            "SELECT auto_create_followup_agent_conversation FROM review_settings WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .expect("autonomy setting should be readable");
    assert_eq!(enabled, 1);
}

#[test]
fn migration_is_idempotent_and_supports_issue_lookup() {
    let conn = setup_test_db();
    v20260625153000_agent_conversation_issues::migrate(&conn).unwrap();
    v20260625153000_agent_conversation_issues::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO agent_conversation_issues (
            id, project_id, conversation_id, source_task_id, issue_kind,
            severity, status, blocking_scope, title, summary,
            blocker_fingerprint, auto_followup_eligible
        ) VALUES (
            'issue-1', 'project-1', 'conversation-1', 'task-1',
            'plan_drift', 'high', 'open', 'followup_only',
            'Plan drift', 'A task found work outside the accepted plan.',
            'scope-drift:task-1:file', 1
        )",
        [],
    )
    .expect("issue insert should succeed");

    let found: String = conn
        .query_row(
            "SELECT id FROM agent_conversation_issues
             WHERE conversation_id = ?1
               AND source_task_id = ?2
               AND issue_kind = ?3
               AND blocker_fingerprint = ?4
               AND status = 'open'",
            [
                "conversation-1",
                "task-1",
                "plan_drift",
                "scope-drift:task-1:file",
            ],
            |row| row.get(0),
        )
        .expect("fingerprint lookup should find issue");
    assert_eq!(found, "issue-1");
}
