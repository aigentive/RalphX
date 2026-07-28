use rusqlite::Connection;

use super::v20260728183000_workspace_review_plan_context::migrate;

#[test]
fn migration_adds_nullable_workspace_review_plan_context_authority() {
    let conn = Connection::open_in_memory().expect("database should open");
    conn.execute_batch(
        "CREATE TABLE agent_workspace_review_monitors (
            conversation_id TEXT PRIMARY KEY,
            reviewed_diff_fingerprint TEXT NULL
        );
        INSERT INTO agent_workspace_review_monitors (
            conversation_id, reviewed_diff_fingerprint
        ) VALUES ('legacy-review', 'diff-1');",
    )
    .expect("legacy table should be created");

    migrate(&conn).expect("migration should succeed");

    let fingerprints: (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT current_plan_context_fingerprint,
                    reviewed_plan_context_fingerprint
             FROM agent_workspace_review_monitors
             WHERE conversation_id = 'legacy-review'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("legacy row should remain readable");
    assert_eq!(fingerprints, (None, None));
}
