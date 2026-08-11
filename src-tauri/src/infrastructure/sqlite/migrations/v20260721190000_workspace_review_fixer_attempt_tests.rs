use rusqlite::Connection;

use super::v20260721190000_workspace_review_fixer_attempt::migrate;

#[test]
fn migration_adds_backend_owned_fixer_attempt_identity() {
    let conn = Connection::open_in_memory().expect("database should open");
    conn.execute_batch(
        "CREATE TABLE agent_workspace_review_monitors (
            conversation_id TEXT PRIMARY KEY,
            review_fixer_status TEXT NULL,
            last_error TEXT NULL
        );
        INSERT INTO agent_workspace_review_monitors (
            conversation_id, review_fixer_status
        ) VALUES ('legacy-routing', 'routing');",
    )
    .expect("legacy table should be created");

    migrate(&conn).expect("migration should succeed");

    let mut statement = conn
        .prepare("PRAGMA table_info(agent_workspace_review_monitors)")
        .expect("table info should prepare");
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .expect("columns should query")
        .collect::<Result<Vec<_>, _>>()
        .expect("columns should decode");
    assert!(columns
        .iter()
        .any(|column| column == "review_fixer_attempt_id"));
    let (status, error): (String, String) = conn
        .query_row(
            "SELECT review_fixer_status, last_error
             FROM agent_workspace_review_monitors
             WHERE conversation_id = 'legacy-routing'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("legacy routing row should reconcile");
    assert_eq!(status, "failed");
    assert!(error.contains("predates durable attempt attribution"));
}
