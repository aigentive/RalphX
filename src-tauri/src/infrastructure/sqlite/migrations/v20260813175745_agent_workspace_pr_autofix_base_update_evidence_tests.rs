//! Tests for migration v20260813175745: agent workspace pr autofix base update evidence

use rusqlite::Connection;

use super::v20260813175745_agent_workspace_pr_autofix_base_update_evidence::migrate;

fn seeded_connection() -> Connection {
    let conn = Connection::open_in_memory().expect("open migration fixture");
    conn.execute_batch("CREATE TABLE agent_workspace_repair_attempts (id TEXT PRIMARY KEY);")
        .expect("seed repair attempts table");
    conn
}

#[test]
fn migration_adds_idempotent_nullable_base_update_evidence_columns() {
    let conn = seeded_connection();

    migrate(&conn).expect("first migration succeeds");
    migrate(&conn).expect("second migration succeeds");

    let column_names = conn
        .prepare("PRAGMA table_info(agent_workspace_repair_attempts)")
        .expect("prepare table inspection")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("read table columns")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect table columns");
    assert!(column_names
        .iter()
        .any(|column| column == "pr_autofix_issue_kind"));
    assert!(column_names
        .iter()
        .any(|column| column == "base_update_head_commit"));
}

#[test]
fn migrated_columns_are_nullable_and_round_trip_values() {
    let conn = seeded_connection();
    migrate(&conn).expect("migration succeeds");

    conn.execute(
        "INSERT INTO agent_workspace_repair_attempts (id) VALUES ('legacy-row')",
        [],
    )
    .expect("legacy row inserts without the new columns");
    conn.execute(
        "INSERT INTO agent_workspace_repair_attempts \
         (id, pr_autofix_issue_kind, base_update_head_commit) VALUES ('new-row', ?1, ?2)",
        rusqlite::params!["mergeability", "abc123"],
    )
    .expect("new row inserts with evidence columns");

    let legacy: (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT pr_autofix_issue_kind, base_update_head_commit \
             FROM agent_workspace_repair_attempts WHERE id = 'legacy-row'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read legacy row");
    assert_eq!(legacy, (None, None));

    let stored: (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT pr_autofix_issue_kind, base_update_head_commit \
             FROM agent_workspace_repair_attempts WHERE id = 'new-row'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read new row");
    assert_eq!(
        stored,
        (Some("mergeability".to_string()), Some("abc123".to_string()))
    );
}
