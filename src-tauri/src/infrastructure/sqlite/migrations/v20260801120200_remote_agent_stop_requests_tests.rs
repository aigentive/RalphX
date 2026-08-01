//! Tests for migration v20260801120200: remote agent stop requests

use rusqlite::Connection;

use super::helpers;
use super::v20260801120200_remote_agent_stop_requests;

#[test]
fn migration_creates_remote_agent_stop_requests_with_expected_columns() {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");

    v20260801120200_remote_agent_stop_requests::migrate(&conn).unwrap();

    assert!(helpers::table_exists(&conn, "remote_agent_stop_requests"));
    for column in [
        "id",
        "conversation_id",
        "status",
        "error_code",
        "requested_by_device_id",
        "claimed_at",
        "created_at",
        "updated_at",
    ] {
        assert!(
            helpers::column_exists(&conn, "remote_agent_stop_requests", column),
            "missing {column}"
        );
    }
}

/// Forward-only re-run must be a no-op, not an error: migrations replay on every boot.
#[test]
fn migration_is_idempotent() {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");

    v20260801120200_remote_agent_stop_requests::migrate(&conn).unwrap();
    v20260801120200_remote_agent_stop_requests::migrate(&conn).unwrap();

    assert!(helpers::table_exists(&conn, "remote_agent_stop_requests"));
}
