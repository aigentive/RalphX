//! Tests for migration v20260701174810: workspace review hunk annotations

use rusqlite::Connection;

use super::v20260701174810_workspace_review_hunk_annotations;

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

#[test]
fn workspace_review_hunk_annotations_table_and_indexes_are_added() {
    let conn = setup_test_db();
    v20260701174810_workspace_review_hunk_annotations::migrate(&conn).unwrap();

    let table_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name = 'agent_workspace_review_hunk_annotations'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(table_count, 1);

    let columns = [
        "id",
        "conversation_id",
        "project_id",
        "artifact_id",
        "artifact_version",
        "target_scope",
        "head_sha",
        "diff_fingerprint",
        "path",
        "diff_source",
        "hunk_header",
        "old_start",
        "old_lines",
        "new_start",
        "new_lines",
        "title",
        "message",
        "level",
        "created_by_run_id",
        "created_at",
    ];
    for column in columns {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('agent_workspace_review_hunk_annotations')
                 WHERE name = ?1",
                [column],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "missing column {column}");
    }

    for index in [
        "idx_agent_workspace_review_hunk_annotations_artifact",
        "idx_agent_workspace_review_hunk_annotations_current",
        "idx_agent_workspace_review_hunk_annotations_path",
    ] {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'index' AND name = ?1",
                [index],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "missing index {index}");
    }
}

#[test]
fn workspace_review_hunk_annotations_migration_is_idempotent() {
    let conn = setup_test_db();
    v20260701174810_workspace_review_hunk_annotations::migrate(&conn).unwrap();
    v20260701174810_workspace_review_hunk_annotations::migrate(&conn).unwrap();

    let table_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name = 'agent_workspace_review_hunk_annotations'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(table_count, 1);
}
