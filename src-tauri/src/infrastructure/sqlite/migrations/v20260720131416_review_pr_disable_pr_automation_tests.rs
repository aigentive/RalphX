//! Tests for migration v20260720131416: disable Review PR owned-PR automation

use rusqlite::{params, Connection};

use super::v20260720131416_review_pr_disable_pr_automation;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("create in-memory database");
    conn.execute_batch(
        "CREATE TABLE agent_conversation_workspaces (
            conversation_id TEXT PRIMARY KEY,
            mode TEXT NOT NULL,
            source_pr_number INTEGER NULL,
            source_pr_url TEXT NULL,
            auto_publish_enabled INTEGER NOT NULL DEFAULT 1,
            auto_publish_initial_pr_enabled INTEGER NOT NULL DEFAULT 0,
            auto_publish_paused_pr_autofix_enabled INTEGER NULL,
            auto_publish_paused_pr_auto_merge_desired INTEGER NULL,
            pr_autofix_enabled INTEGER NOT NULL DEFAULT 0,
            pr_auto_merge_desired INTEGER NOT NULL DEFAULT 0,
            pr_auto_merge_current INTEGER NULL,
            publication_push_status TEXT NULL,
            pr_supervision_status TEXT NULL,
            pr_supervision_summary TEXT NULL,
            pr_supervision_updated_at TEXT NULL
        );
        CREATE TABLE agent_workspace_pr_review_monitors (
            conversation_id TEXT PRIMARY KEY,
            monitor_enabled INTEGER NOT NULL,
            status TEXT NOT NULL,
            last_reviewed_head_sha TEXT NULL
        );",
    )
    .expect("create prior schema");
    conn
}

fn seed_workspace(conn: &Connection, conversation_id: &str, mode: &str) {
    conn.execute(
        "INSERT INTO agent_conversation_workspaces (
            conversation_id, mode, source_pr_number, source_pr_url,
            auto_publish_enabled, auto_publish_initial_pr_enabled,
            auto_publish_paused_pr_autofix_enabled,
            auto_publish_paused_pr_auto_merge_desired,
            pr_autofix_enabled, pr_auto_merge_desired, pr_auto_merge_current,
            publication_push_status, pr_supervision_status,
            pr_supervision_summary, pr_supervision_updated_at
         ) VALUES (?1, ?2, 779, 'https://github.com/owner/repo/pull/779',
            0, 1, 1, 1, 1, 1, 1, 'needs_agent', 'fixing',
            'Fixer was dispatched', '2026-07-20T12:00:00+00:00')",
        params![conversation_id, mode],
    )
    .expect("seed workspace");
}

#[test]
fn migration_cleans_only_review_pr_automation_and_preserves_monitor_state() {
    let conn = setup_test_db();
    seed_workspace(&conn, "review", "review_pr");
    seed_workspace(&conn, "edit", "edit");
    conn.execute(
        "INSERT INTO agent_workspace_pr_review_monitors
         (conversation_id, monitor_enabled, status, last_reviewed_head_sha)
         VALUES ('review', 0, 'paused', 'review-head')",
        [],
    )
    .expect("seed paused monitor");

    v20260720131416_review_pr_disable_pr_automation::migrate(&conn).expect("run cleanup migration");
    v20260720131416_review_pr_disable_pr_automation::migrate(&conn)
        .expect("migration should be idempotent");

    let review: (
        i64,
        i64,
        Option<i64>,
        i64,
        Option<i64>,
        Option<i64>,
        i64,
        i64,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    ) = conn
        .query_row(
            "SELECT pr_autofix_enabled, pr_auto_merge_desired, pr_auto_merge_current,
                    auto_publish_enabled, auto_publish_paused_pr_autofix_enabled,
                    auto_publish_paused_pr_auto_merge_desired,
                    auto_publish_initial_pr_enabled, source_pr_number,
                    source_pr_url, publication_push_status, pr_supervision_status,
                    pr_supervision_summary, pr_supervision_updated_at,
                    (SELECT status FROM agent_workspace_pr_review_monitors
                     WHERE conversation_id = 'review')
             FROM agent_conversation_workspaces WHERE conversation_id = 'review'",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                    row.get(10)?,
                    row.get(11)?,
                    row.get(12)?,
                    row.get(13)?,
                ))
            },
        )
        .expect("read cleaned Review PR workspace");
    let (
        pr_autofix_enabled,
        pr_auto_merge_desired,
        pr_auto_merge_current,
        auto_publish_enabled,
        paused_pr_autofix_enabled,
        paused_pr_auto_merge_desired,
        auto_publish_initial_pr_enabled,
        source_pr_number,
        source_pr_url,
        publication_push_status,
        pr_supervision_status,
        pr_supervision_summary,
        pr_supervision_updated_at,
        monitor_status,
    ) = review;
    assert_eq!(pr_autofix_enabled, 0);
    assert_eq!(pr_auto_merge_desired, 0);
    assert_eq!(pr_auto_merge_current, None);
    assert_eq!(auto_publish_enabled, 1);
    assert_eq!(paused_pr_autofix_enabled, None);
    assert_eq!(paused_pr_auto_merge_desired, None);
    assert_eq!(auto_publish_initial_pr_enabled, 0);
    assert_eq!(source_pr_number, 779);
    assert_eq!(
        source_pr_url.as_deref(),
        Some("https://github.com/owner/repo/pull/779")
    );
    assert_eq!(publication_push_status, None);
    assert_eq!(pr_supervision_status, None);
    assert_eq!(pr_supervision_summary, None);
    assert_eq!(pr_supervision_updated_at, None);
    assert_eq!(monitor_status.as_deref(), Some("paused"));

    let edit: (i64, i64, Option<i64>, i64, Option<String>) = conn
        .query_row(
            "SELECT pr_autofix_enabled, pr_auto_merge_desired, pr_auto_merge_current,
                    auto_publish_enabled, publication_push_status
             FROM agent_conversation_workspaces WHERE conversation_id = 'edit'",
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
        .expect("read untouched Edit workspace");
    assert_eq!(edit, (1, 1, Some(1), 0, Some("needs_agent".to_string())));
}
