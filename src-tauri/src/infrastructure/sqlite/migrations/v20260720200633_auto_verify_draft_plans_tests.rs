//! Tests for migration v20260720200633: auto verify draft plans

use rusqlite::Connection;

use super::v20260720200633_auto_verify_draft_plans;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE ideation_settings (
            id INTEGER PRIMARY KEY,
            auto_verify_plans INTEGER NOT NULL DEFAULT 0
        );
        INSERT INTO ideation_settings (id, auto_verify_plans) VALUES (1, 0);",
    )
    .expect("create pre-migration settings row");
    conn
}

#[test]
fn migration_backfills_existing_settings_rows_enabled() {
    let conn = setup_test_db();
    v20260720200633_auto_verify_draft_plans::migrate(&conn).unwrap();

    let (completion_trigger, acceptance_fallback): (i64, i64) = conn
        .query_row(
            "SELECT auto_verify_draft_plans, auto_verify_plans FROM ideation_settings WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(completion_trigger, 1);
    assert_eq!(acceptance_fallback, 0);
}

#[test]
fn migration_default_enables_new_settings_rows() {
    let conn = setup_test_db();
    v20260720200633_auto_verify_draft_plans::migrate(&conn).unwrap();
    conn.execute("INSERT INTO ideation_settings (id) VALUES (2)", [])
        .unwrap();

    let enabled: i64 = conn
        .query_row(
            "SELECT auto_verify_draft_plans FROM ideation_settings WHERE id = 2",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(enabled, 1);
}

#[test]
fn migration_creates_exact_artifact_deferred_attention_marker() {
    let conn = setup_test_db();
    v20260720200633_auto_verify_draft_plans::migrate(&conn).unwrap();
    conn.execute(
        "INSERT INTO deferred_plan_approval_notifications (session_id, artifact_id)
         VALUES ('session-1', 'artifact-1')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO deferred_plan_approval_notifications (session_id, artifact_id)
         VALUES ('session-1', 'artifact-2')
         ON CONFLICT(session_id) DO UPDATE SET artifact_id = excluded.artifact_id",
        [],
    )
    .unwrap();

    let artifact_id: String = conn
        .query_row(
            "SELECT artifact_id FROM deferred_plan_approval_notifications WHERE session_id = 'session-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(artifact_id, "artifact-2");
}
