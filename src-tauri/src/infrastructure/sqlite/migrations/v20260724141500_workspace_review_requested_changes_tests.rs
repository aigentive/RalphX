use rusqlite::Connection;

use super::v20260724141500_workspace_review_requested_changes::migrate;

#[test]
fn migration_adds_requested_changes_artifact_lineage_without_manufacturing_legacy_content() {
    let conn = Connection::open_in_memory().expect("database should open");
    conn.execute_batch(
        "CREATE TABLE agent_workspace_review_monitors (
            conversation_id TEXT PRIMARY KEY,
            review_artifact_id TEXT NULL,
            review_artifact_version INTEGER NULL
        );
        INSERT INTO agent_workspace_review_monitors (
            conversation_id, review_artifact_id, review_artifact_version
        ) VALUES ('legacy-review', 'overview-1', 3);",
    )
    .expect("legacy table should be created");

    migrate(&conn).expect("migration should succeed");

    let columns = conn
        .prepare("PRAGMA table_info(agent_workspace_review_monitors)")
        .expect("table info should prepare")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("columns should query")
        .collect::<Result<Vec<_>, _>>()
        .expect("columns should decode");
    for expected in [
        "review_requested_changes_artifact_id",
        "review_requested_changes_artifact_version",
        "review_requested_changes_artifact_updated_at",
        "review_requested_changes_previous_version_id",
    ] {
        assert!(columns.iter().any(|column| column == expected));
    }

    let requested_changes: (Option<String>, Option<i64>) = conn
        .query_row(
            "SELECT review_requested_changes_artifact_id,
                    review_requested_changes_artifact_version
             FROM agent_workspace_review_monitors
             WHERE conversation_id = 'legacy-review'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("legacy row should remain readable");
    assert_eq!(requested_changes, (None, None));
}
