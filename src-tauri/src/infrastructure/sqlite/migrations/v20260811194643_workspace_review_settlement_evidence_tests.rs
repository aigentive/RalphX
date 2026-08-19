//! Tests for migration v20260811194643: durable Workspace Review settlement evidence.

use rusqlite::Connection;

use super::helpers;
use super::v20260811194643_workspace_review_settlement_evidence;

const MONITOR_COLUMNS: &[&str] = &[
    "review_artifact_recorded_outcome",
    "review_artifact_recorded_outcome_run_id",
    "review_artifact_recorded_blocking_summary",
    "review_settlement_source",
    "annotation_run_id",
    "previous_review_artifact_id",
    "previous_review_requested_changes_artifact_id",
    "previous_review_artifact_version",
    "previous_review_diff_fingerprint",
    "previous_review_head_sha",
    "previous_review_outcome",
];

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("create migration test database");
    conn.execute_batch(
        "CREATE TABLE agent_workspace_review_monitors (conversation_id TEXT PRIMARY KEY);
         CREATE TABLE agent_workspace_review_hunk_annotations (
             id TEXT PRIMARY KEY,
             conversation_id TEXT NOT NULL,
             artifact_id TEXT NOT NULL
         );",
    )
    .expect("seed legacy-shaped migration database");
    conn
}

#[test]
fn migration_adds_settlement_evidence_columns() {
    let conn = setup_test_db();

    v20260811194643_workspace_review_settlement_evidence::migrate(&conn)
        .expect("migration should succeed");

    for column in MONITOR_COLUMNS {
        assert!(
            helpers::column_exists(&conn, "agent_workspace_review_monitors", column),
            "expected monitor column {column}"
        );
    }
    assert!(helpers::column_exists(
        &conn,
        "agent_workspace_review_hunk_annotations",
        "file_patch_hash"
    ));
}

/// Every new column is nullable: existing monitors must survive the upgrade untouched, with no
/// recorded outcome (so they can never authorize a degraded settlement).
#[test]
fn migration_leaves_legacy_rows_without_recorded_evidence() {
    let conn = setup_test_db();
    conn.execute(
        "INSERT INTO agent_workspace_review_monitors (conversation_id) VALUES (?1)",
        ["legacy-monitor"],
    )
    .expect("legacy monitor should insert");

    v20260811194643_workspace_review_settlement_evidence::migrate(&conn)
        .expect("migration should succeed");

    let recorded: Option<String> = conn
        .query_row(
            "SELECT review_artifact_recorded_outcome
             FROM agent_workspace_review_monitors
             WHERE conversation_id = 'legacy-monitor'",
            [],
            |row| row.get(0),
        )
        .expect("legacy monitor should still be readable");
    assert!(recorded.is_none());
}

#[test]
fn migration_is_idempotent() {
    let conn = setup_test_db();

    v20260811194643_workspace_review_settlement_evidence::migrate(&conn)
        .expect("first migration should succeed");
    v20260811194643_workspace_review_settlement_evidence::migrate(&conn)
        .expect("second migration should succeed");

    assert!(helpers::column_exists(
        &conn,
        "agent_workspace_review_monitors",
        "annotation_run_id"
    ));
}
