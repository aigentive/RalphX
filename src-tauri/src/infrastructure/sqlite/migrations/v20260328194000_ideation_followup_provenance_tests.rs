//! Tests for migration v20260328194000: ideation follow-up provenance

use rusqlite::Connection;

use super::v20260328194000_ideation_followup_provenance;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE ideation_sessions (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'active'
        )",
    )
    .expect("Failed to create ideation_sessions table");
    conn
}

#[test]
fn test_migration_adds_provenance_columns() {
    let conn = setup_test_db();
    v20260328194000_ideation_followup_provenance::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO ideation_sessions (
            id, project_id, source_task_id, source_context_type, source_context_id, spawn_reason
        ) VALUES ('s1', 'p1', 'task-1', 'task_execution', 'task-1', 'out_of_scope_failure')",
        [],
    )
    .expect("Insert with provenance fields should succeed");

    let row: (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    ) = conn
        .query_row(
            "SELECT source_task_id, source_context_type, source_context_id, spawn_reason
             FROM ideation_sessions WHERE id = 's1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("Query should succeed");

    assert_eq!(
        row,
        (
            Some("task-1".to_string()),
            Some("task_execution".to_string()),
            Some("task-1".to_string()),
            Some("out_of_scope_failure".to_string())
        )
    );
}

#[test]
fn test_migration_is_idempotent() {
    let conn = setup_test_db();
    v20260328194000_ideation_followup_provenance::migrate(&conn).unwrap();
    v20260328194000_ideation_followup_provenance::migrate(&conn).unwrap();
}
