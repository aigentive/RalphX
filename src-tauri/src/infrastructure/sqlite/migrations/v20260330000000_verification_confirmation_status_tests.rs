use rusqlite::Connection;

use super::helpers;
use super::v20260330000000_verification_confirmation_status;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    conn.execute_batch(
        "CREATE TABLE ideation_sessions (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL
        );",
    )
    .expect("Failed to create test schema");
    conn
}

#[test]
fn test_verification_confirmation_status_column_added() {
    let conn = setup_test_db();
    assert!(
        !helpers::column_exists(
            &conn,
            "ideation_sessions",
            "verification_confirmation_status"
        ),
        "verification_confirmation_status should not exist before migration"
    );

    v20260330000000_verification_confirmation_status::migrate(&conn).unwrap();

    assert!(
        helpers::column_exists(
            &conn,
            "ideation_sessions",
            "verification_confirmation_status"
        ),
        "verification_confirmation_status should exist after migration"
    );
}

#[test]
fn test_existing_sessions_default_to_null() {
    let conn = setup_test_db();
    conn.execute(
        "INSERT INTO ideation_sessions (id, title) VALUES ('s1', 'Pre-migration Session')",
        [],
    )
    .unwrap();

    v20260330000000_verification_confirmation_status::migrate(&conn).unwrap();

    let status: Option<String> = conn
        .query_row(
            "SELECT verification_confirmation_status FROM ideation_sessions WHERE id = 's1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(status.is_none());
}

#[test]
fn test_sessions_can_store_all_states() {
    let conn = setup_test_db();
    v20260330000000_verification_confirmation_status::migrate(&conn).unwrap();

    for (id, state) in [("s1", "pending"), ("s2", "accepted"), ("s3", "rejected")] {
        conn.execute(
            "INSERT INTO ideation_sessions (id, title, verification_confirmation_status) VALUES (?1, ?2, ?3)",
            rusqlite::params![id, format!("Session {id}"), state],
        )
        .unwrap();

        let stored: Option<String> = conn
            .query_row(
                "SELECT verification_confirmation_status FROM ideation_sessions WHERE id = ?1",
                rusqlite::params![id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored.as_deref(), Some(state));
    }
}

#[test]
fn test_migration_is_idempotent() {
    let conn = setup_test_db();
    v20260330000000_verification_confirmation_status::migrate(&conn).unwrap();
    // Running migration again should not fail
    v20260330000000_verification_confirmation_status::migrate(&conn).unwrap();
}
