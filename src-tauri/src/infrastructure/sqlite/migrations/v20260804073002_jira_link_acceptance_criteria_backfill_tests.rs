//! Tests for migration v20260804073002: jira link acceptance criteria backfill

use rusqlite::{params, Connection};

use super::v20260804073002_jira_link_acceptance_criteria_backfill;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE agent_conversation_jira_issue_links (
            conversation_id TEXT PRIMARY KEY,
            acceptance_criteria_markdown TEXT,
            refresh_status TEXT NOT NULL
        );",
    )
    .expect("create Jira links table");
    conn
}

#[test]
fn resets_only_loaded_links_without_acceptance_criteria_and_is_idempotent() {
    let conn = setup_test_db();
    for (conversation_id, acceptance_criteria, refresh_status) in [
        ("loaded-null", None, "loaded"),
        ("loaded-blank", Some("   "), "loaded"),
        ("loaded-present", Some("- Visible"), "loaded"),
        ("error-null", None, "error"),
        ("not-loaded-null", None, "not_loaded"),
    ] {
        conn.execute(
            "INSERT INTO agent_conversation_jira_issue_links
                (conversation_id, acceptance_criteria_markdown, refresh_status)
             VALUES (?1, ?2, ?3)",
            params![conversation_id, acceptance_criteria, refresh_status],
        )
        .expect("seed Jira link");
    }

    v20260804073002_jira_link_acceptance_criteria_backfill::migrate(&conn)
        .expect("first migration run");
    v20260804073002_jira_link_acceptance_criteria_backfill::migrate(&conn)
        .expect("second migration run");

    let statuses = [
        "loaded-null",
        "loaded-blank",
        "loaded-present",
        "error-null",
        "not-loaded-null",
    ]
    .into_iter()
    .map(|conversation_id| {
        conn.query_row(
            "SELECT refresh_status FROM agent_conversation_jira_issue_links
             WHERE conversation_id = ?1",
            [conversation_id],
            |row| row.get::<_, String>(0),
        )
        .expect("read refresh status")
    })
    .collect::<Vec<_>>();

    assert_eq!(
        statuses,
        vec!["not_loaded", "not_loaded", "loaded", "error", "not_loaded"]
    );
}
