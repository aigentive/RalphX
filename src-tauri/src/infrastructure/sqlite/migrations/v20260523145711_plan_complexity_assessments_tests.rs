//! Tests for migration v20260523145711: plan complexity assessments

use rusqlite::Connection;

use super::helpers::table_exists;
use super::v20260523145711_plan_complexity_assessments;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute(
        "CREATE TABLE ideation_sessions (id TEXT PRIMARY KEY)",
        [],
    )
    .expect("create ideation_sessions table");
    conn.execute("CREATE TABLE artifacts (id TEXT PRIMARY KEY)", [])
        .expect("create artifacts table");
    conn
}

#[test]
fn test_migration_creates_plan_complexity_assessments_table() {
    let conn = setup_test_db();
    v20260523145711_plan_complexity_assessments::migrate(&conn).unwrap();

    assert!(table_exists(&conn, "plan_complexity_assessments"));
    conn.execute(
        "INSERT INTO ideation_sessions (id) VALUES ('session-1')",
        [],
    )
    .unwrap();
    conn.execute("INSERT INTO artifacts (id) VALUES ('artifact-1')", [])
        .unwrap();
    conn.execute(
        "INSERT INTO plan_complexity_assessments (
            id, session_id, artifact_id, artifact_version, level, score,
            recommended_action, confidence, reason_summary, signals_json,
            assessed_by, created_at, updated_at
        ) VALUES (
            'assessment-1', 'session-1', 'artifact-1', 2, 'complex', 78,
            'create_proposals', 0.86, 'Multiple dependent implementation areas.',
            '{\"areas\":3}', 'test', '2026-05-23T00:00:00Z',
            '2026-05-23T00:00:00Z'
        )",
        [],
    )
    .unwrap();
}

#[test]
fn test_migration_is_idempotent() {
    let conn = setup_test_db();

    v20260523145711_plan_complexity_assessments::migrate(&conn).unwrap();
    v20260523145711_plan_complexity_assessments::migrate(&conn).unwrap();

    assert!(table_exists(&conn, "plan_complexity_assessments"));
}
