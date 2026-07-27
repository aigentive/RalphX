//! Tests for migration v20260727191500: remote environments registry

use rusqlite::Connection;

use super::{helpers, v20260727191500_remote_environments};

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("in-memory database should open")
}

fn insert_environment(conn: &Connection, id: &str, environment_id: &str, status: &str) -> Result<usize, rusqlite::Error> {
    conn.execute(
        "INSERT INTO remote_environments (
            id, environment_id, name, base_url, candidate_urls,
            token_secret_ref, scopes, protocol_version, status
         ) VALUES (?1, ?2, 'Mac Studio', 'https://mac-studio.tailnet.ts.net', '[]',
                   ?3, '[\"ui:read\"]', 1, ?4)",
        rusqlite::params![id, environment_id, format!("remote-env:{id}:token"), status],
    )
}

#[test]
fn migration_creates_the_remote_environments_schema() {
    let conn = setup_test_db();

    v20260727191500_remote_environments::migrate(&conn)
        .expect("migration should create remote_environments");

    assert!(helpers::table_exists(&conn, "remote_environments"));
    for column in [
        "id",
        "environment_id",
        "name",
        "base_url",
        "candidate_urls",
        "token_secret_ref",
        "scopes",
        "protocol_version",
        "status",
        "created_at",
        "last_connected_at",
    ] {
        assert!(
            helpers::column_exists(&conn, "remote_environments", column),
            "remote_environments should contain {column}"
        );
    }
}

#[test]
fn migration_enforces_unique_host_identity() {
    let conn = setup_test_db();
    v20260727191500_remote_environments::migrate(&conn)
        .expect("migration should create remote_environments");

    insert_environment(&conn, "row-a", "env-1", "active")
        .expect("first row for a host identity should insert");
    assert!(
        insert_environment(&conn, "row-b", "env-1", "active").is_err(),
        "a second row for the same environment_id must violate UNIQUE"
    );
}

#[test]
fn migration_rejects_unknown_status_values() {
    let conn = setup_test_db();
    v20260727191500_remote_environments::migrate(&conn)
        .expect("migration should create remote_environments");

    for status in ["active", "pending_add", "pending_delete"] {
        insert_environment(&conn, &format!("row-{status}"), &format!("env-{status}"), status)
            .unwrap_or_else(|error| panic!("{status} should be accepted: {error}"));
    }
    assert!(
        insert_environment(&conn, "row-bad", "env-bad", "half_paired").is_err(),
        "statuses outside the reconciler set must be rejected"
    );
}

#[test]
fn migration_is_idempotent() {
    let conn = setup_test_db();

    v20260727191500_remote_environments::migrate(&conn).expect("first migration should succeed");
    v20260727191500_remote_environments::migrate(&conn)
        .expect("second migration should remain safe");

    let row_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM remote_environments", [], |row| {
            row.get(0)
        })
        .expect("table should be queryable");
    assert_eq!(row_count, 0, "migration must not seed environments");
}
