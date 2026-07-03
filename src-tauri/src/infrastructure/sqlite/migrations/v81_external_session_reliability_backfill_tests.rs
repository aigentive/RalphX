//! Tests for migration v81: repair skipped external session reliability columns

use rusqlite::Connection;

use super::helpers;
use super::v81_external_session_reliability_backfill;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");

    conn.execute_batch(
        "CREATE TABLE ideation_sessions (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            title TEXT,
            status TEXT NOT NULL DEFAULT 'active',
            dependencies_acknowledged BOOLEAN NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
        );",
    )
    .expect("Failed to create test schema");

    conn
}

#[test]
fn test_backfill_columns_added() {
    let conn = setup_test_db();

    assert!(!helpers::column_exists(
        &conn,
        "ideation_sessions",
        "api_key_id"
    ));
    assert!(!helpers::column_exists(
        &conn,
        "ideation_sessions",
        "idempotency_key"
    ));
    assert!(!helpers::column_exists(
        &conn,
        "ideation_sessions",
        "external_activity_phase"
    ));
    assert!(!helpers::column_exists(
        &conn,
        "ideation_sessions",
        "external_last_read_message_id"
    ));

    v81_external_session_reliability_backfill::migrate(&conn).unwrap();

    assert!(helpers::column_exists(
        &conn,
        "ideation_sessions",
        "api_key_id"
    ));
    assert!(helpers::column_exists(
        &conn,
        "ideation_sessions",
        "idempotency_key"
    ));
    assert!(helpers::column_exists(
        &conn,
        "ideation_sessions",
        "external_activity_phase"
    ));
    assert!(helpers::column_exists(
        &conn,
        "ideation_sessions",
        "external_last_read_message_id"
    ));
    assert!(helpers::index_exists(
        &conn,
        "idx_ideation_sessions_idempotency"
    ));
}

#[test]
fn test_backfill_creates_partial_unique_index() {
    let conn = setup_test_db();
    v81_external_session_reliability_backfill::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id, api_key_id, idempotency_key)
         VALUES ('s1', 'p1', 'key1', 'dup')",
        [],
    )
    .unwrap();

    let err = conn
        .execute(
            "INSERT INTO ideation_sessions (id, project_id, api_key_id, idempotency_key)
             VALUES ('s2', 'p1', 'key1', 'dup')",
            [],
        )
        .expect_err("duplicate api_key_id + idempotency_key should fail");

    assert!(
        err.to_string().contains("UNIQUE"),
        "expected UNIQUE violation, got {err}"
    );
}

#[test]
fn test_backfill_idempotent() {
    let conn = setup_test_db();

    v81_external_session_reliability_backfill::migrate(&conn).unwrap();
    v81_external_session_reliability_backfill::migrate(&conn).unwrap();

    assert!(helpers::column_exists(
        &conn,
        "ideation_sessions",
        "api_key_id"
    ));
    assert!(helpers::index_exists(
        &conn,
        "idx_ideation_sessions_idempotency"
    ));
}
