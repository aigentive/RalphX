use rusqlite::Connection;

use super::v20260724130000_plan_blueprints;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("open migration test database");
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         CREATE TABLE artifacts (id TEXT PRIMARY KEY);
         CREATE TABLE ideation_sessions (
             id TEXT PRIMARY KEY,
             plan_artifact_id TEXT
         );
         CREATE TABLE plan_artifact_approvals (
             session_id TEXT PRIMARY KEY,
             artifact_id TEXT NOT NULL,
             artifact_version INTEGER NOT NULL
         );
         CREATE TABLE task_proposals (
             id TEXT PRIMARY KEY,
             plan_artifact_id TEXT,
             plan_version_at_creation INTEGER
         );
         CREATE TABLE tasks (
             id TEXT PRIMARY KEY,
             plan_artifact_id TEXT
         );
         CREATE TABLE deferred_plan_approval_notifications (
             session_id TEXT PRIMARY KEY,
             artifact_id TEXT NOT NULL
         );
         CREATE TABLE automation_runs (
             id TEXT PRIMARY KEY,
             plan_last_parked_artifact_id TEXT
         );
         CREATE TABLE plan_complexity_assessments (
             id TEXT PRIMARY KEY,
             session_id TEXT NOT NULL,
             artifact_id TEXT NOT NULL,
             artifact_version INTEGER NOT NULL
         );",
    )
    .expect("create legacy schema");
    conn
}

#[test]
fn existing_sessions_are_grandfathered_but_future_rows_default_to_v2() {
    let conn = setup_test_db();
    conn.execute(
        "INSERT INTO ideation_sessions (id, plan_artifact_id) VALUES ('legacy', 'overview-1')",
        [],
    )
    .unwrap();

    v20260724130000_plan_blueprints::migrate(&conn).unwrap();
    conn.execute(
        "INSERT INTO ideation_sessions (id, plan_artifact_id) VALUES ('future', 'overview-2')",
        [],
    )
    .unwrap();

    let legacy: i64 = conn
        .query_row(
            "SELECT plan_contract_version FROM ideation_sessions WHERE id = 'legacy'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let future: i64 = conn
        .query_row(
            "SELECT plan_contract_version FROM ideation_sessions WHERE id = 'future'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(legacy, 1);
    assert_eq!(future, 2);
}

#[test]
fn migration_adds_pair_lineage_and_backfills_notification_target() {
    let conn = setup_test_db();
    conn.execute(
        "INSERT INTO deferred_plan_approval_notifications (session_id, artifact_id)
         VALUES ('session-1', 'overview-1')",
        [],
    )
    .unwrap();

    v20260724130000_plan_blueprints::migrate(&conn).unwrap();

    let target: String = conn
        .query_row(
            "SELECT plan_target_id FROM deferred_plan_approval_notifications
             WHERE session_id = 'session-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(target, "overview-1");

    for (table, column) in [
        ("ideation_sessions", "plan_blueprint_artifact_id"),
        ("ideation_sessions", "inherited_plan_blueprint_artifact_id"),
        ("ideation_sessions", "verified_plan_blueprint_artifact_id"),
        ("ideation_sessions", "blueprint_version_last_read"),
        ("plan_artifact_approvals", "blueprint_artifact_id"),
        ("task_proposals", "blueprint_artifact_id"),
        ("task_proposals", "blueprint_version_at_creation"),
        ("tasks", "plan_blueprint_artifact_id"),
        ("automation_runs", "plan_last_parked_blueprint_artifact_id"),
        ("plan_complexity_assessments", "blueprint_artifact_id"),
        ("plan_complexity_assessments", "blueprint_artifact_version"),
    ] {
        assert!(
            super::helpers::column_exists(&conn, table, column),
            "missing {table}.{column}"
        );
    }
}

#[test]
fn complexity_pair_indexes_distinguish_legacy_and_v2_rows() {
    let conn = setup_test_db();
    v20260724130000_plan_blueprints::migrate(&conn).unwrap();

    assert!(super::helpers::index_exists(
        &conn,
        "idx_plan_complexity_assessments_legacy_unique"
    ));
    assert!(super::helpers::index_exists(
        &conn,
        "idx_plan_complexity_assessments_pair_unique"
    ));

    conn.execute(
        "INSERT INTO plan_complexity_assessments
         (id, session_id, artifact_id, artifact_version)
         VALUES ('legacy-1', 's1', 'o1', 1)",
        [],
    )
    .unwrap();
    assert!(conn
        .execute(
            "INSERT INTO plan_complexity_assessments
             (id, session_id, artifact_id, artifact_version)
             VALUES ('legacy-2', 's1', 'o1', 1)",
            [],
        )
        .is_err());

    conn.execute("INSERT INTO artifacts (id) VALUES ('b2')", [])
        .unwrap();
    conn.execute(
        "INSERT INTO plan_complexity_assessments
         (id, session_id, artifact_id, artifact_version, blueprint_artifact_id, blueprint_artifact_version)
         VALUES ('pair-1', 's2', 'o2', 1, 'b2', 1)",
        [],
    )
    .unwrap();
    assert!(conn
        .execute(
            "INSERT INTO plan_complexity_assessments
             (id, session_id, artifact_id, artifact_version, blueprint_artifact_id, blueprint_artifact_version)
             VALUES ('pair-2', 's2', 'o2', 1, 'b2', 1)",
            [],
        )
        .is_err());
}
