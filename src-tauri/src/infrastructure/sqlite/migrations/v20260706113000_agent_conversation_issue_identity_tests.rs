//! Tests for migration v20260706113000: Agent conversation issue identity and occurrences

use rusqlite::Connection;

use super::helpers;
use super::v20260625153000_agent_conversation_issues;
use super::v20260706113000_agent_conversation_issue_identity;

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
         VALUES (1, '2026-07-06T11:30:00+00:00')",
        [],
    )
    .expect("review settings row should be seeded");
    v20260625153000_agent_conversation_issues::migrate(&conn)
        .expect("base issue migration should run");
    conn
}

#[test]
fn migration_adds_identity_columns_occurrence_table_and_indexes() {
    let conn = setup_test_db();
    v20260706113000_agent_conversation_issue_identity::migrate(&conn).unwrap();

    for column in [
        "canonical_fingerprint",
        "canonical_scope_kind",
        "canonical_scope_subject",
        "canonical_family",
        "superseded_by_issue_id",
    ] {
        assert!(helpers::column_exists(
            &conn,
            "agent_conversation_issues",
            column
        ));
    }
    assert!(helpers::table_exists(
        &conn,
        "agent_conversation_issue_occurrences"
    ));
    assert!(helpers::index_exists(
        &conn,
        "idx_agent_conversation_issues_canonical"
    ));
    assert!(helpers::index_exists(
        &conn,
        "idx_agent_conversation_issues_identity_candidates"
    ));
    assert!(helpers::index_exists(
        &conn,
        "idx_agent_conversation_issue_occurrences_issue"
    ));
}

#[test]
fn migration_is_idempotent_and_backfills_legacy_fingerprint() {
    let conn = setup_test_db();
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
    .expect("legacy issue insert should succeed");

    v20260706113000_agent_conversation_issue_identity::migrate(&conn).unwrap();
    v20260706113000_agent_conversation_issue_identity::migrate(&conn).unwrap();

    let canonical: String = conn
        .query_row(
            "SELECT canonical_fingerprint FROM agent_conversation_issues WHERE id = 'issue-1'",
            [],
            |row| row.get(0),
        )
        .expect("canonical fingerprint should be readable");
    assert_eq!(canonical, "scope-drift:task-1:file");

    conn.execute(
        "INSERT INTO agent_conversation_issue_occurrences (
            id, issue_id, project_id, conversation_id, issue_kind, severity,
            blocking_scope, title, summary, raw_blocker_fingerprint,
            canonical_fingerprint, dedupe_decision
        ) VALUES (
            'occ-1', 'issue-1', 'project-1', 'conversation-1', 'plan_drift',
            'high', 'followup_only', 'Plan drift', 'Summary',
            'scope-drift:task-1:file', 'scope-drift:task-1:file', 'created'
        )",
        [],
    )
    .expect("occurrence insert should succeed");
}
