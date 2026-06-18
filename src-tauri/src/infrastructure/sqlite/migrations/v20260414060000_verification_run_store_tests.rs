use rusqlite::Connection;

use super::helpers;
use super::v20260414060000_verification_run_store;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE ideation_sessions (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL
        );",
    )
    .expect("Failed to create test schema");
    conn
}

#[test]
fn test_verification_run_store_tables_created() {
    let conn = setup_test_db();

    v20260414060000_verification_run_store::migrate(&conn).unwrap();

    assert!(helpers::table_exists(&conn, "verification_runs"));
    assert!(helpers::table_exists(&conn, "verification_rounds"));
    assert!(helpers::table_exists(&conn, "verification_round_gaps"));
    assert!(helpers::table_exists(
        &conn,
        "verification_run_current_gaps"
    ));
    assert!(helpers::column_exists(
        &conn,
        "ideation_sessions",
        "verification_current_round"
    ));
    assert!(helpers::column_exists(
        &conn,
        "ideation_sessions",
        "verification_max_rounds"
    ));
    assert!(helpers::column_exists(
        &conn,
        "ideation_sessions",
        "verification_gap_count"
    ));
    assert!(helpers::column_exists(
        &conn,
        "ideation_sessions",
        "verification_gap_score"
    ));
    assert!(helpers::column_exists(
        &conn,
        "ideation_sessions",
        "verification_convergence_reason"
    ));
}

#[test]
fn test_verification_run_store_unique_session_generation() {
    let conn = setup_test_db();
    v20260414060000_verification_run_store::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id) VALUES ('s1', 'p1')",
        [],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO verification_runs (id, session_id, generation, status, in_progress)
         VALUES ('r1', 's1', 3, 'reviewing', 1)",
        [],
    )
    .unwrap();

    let duplicate = conn.execute(
        "INSERT INTO verification_runs (id, session_id, generation, status, in_progress)
         VALUES ('r2', 's1', 3, 'needs_revision', 0)",
        [],
    );

    assert!(duplicate.is_err(), "session_id + generation must be unique");
}

#[test]
fn test_verification_run_store_idempotent() {
    let conn = setup_test_db();

    v20260414060000_verification_run_store::migrate(&conn).unwrap();
    v20260414060000_verification_run_store::migrate(&conn).unwrap();

    assert!(helpers::table_exists(&conn, "verification_runs"));
    assert!(helpers::table_exists(&conn, "verification_rounds"));
    assert!(helpers::table_exists(&conn, "verification_round_gaps"));
    assert!(helpers::table_exists(
        &conn,
        "verification_run_current_gaps"
    ));
    assert!(helpers::column_exists(
        &conn,
        "ideation_sessions",
        "verification_current_round"
    ));
    assert!(helpers::column_exists(
        &conn,
        "ideation_sessions",
        "verification_max_rounds"
    ));
    assert!(helpers::column_exists(
        &conn,
        "ideation_sessions",
        "verification_gap_count"
    ));
    assert!(helpers::column_exists(
        &conn,
        "ideation_sessions",
        "verification_gap_score"
    ));
    assert!(helpers::column_exists(
        &conn,
        "ideation_sessions",
        "verification_convergence_reason"
    ));
}
