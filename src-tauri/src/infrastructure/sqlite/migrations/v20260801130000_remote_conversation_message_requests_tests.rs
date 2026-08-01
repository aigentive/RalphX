//! Tests for migration v20260801130000: remote conversation message (continuation) requests

use rusqlite::Connection;

use super::helpers;
use super::v20260801130000_remote_conversation_message_requests;

#[test]
fn migration_creates_remote_conversation_message_requests_with_expected_columns() {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");

    v20260801130000_remote_conversation_message_requests::migrate(&conn).unwrap();

    assert!(helpers::table_exists(
        &conn,
        "remote_conversation_message_requests"
    ));
    for column in [
        "id",
        "conversation_id",
        "project_id",
        "content",
        "provider",
        "model_override",
        "logical_effort",
        "status",
        "error_code",
        "requested_by_device_id",
        "agent_run_id",
        "claimed_at",
        "created_at",
        "updated_at",
    ] {
        assert!(
            helpers::column_exists(&conn, "remote_conversation_message_requests", column),
            "missing {column}"
        );
    }
}

/// Forward-only and idempotent: re-running must not wedge a host that already migrated.
#[test]
fn migration_is_idempotent() {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    v20260801130000_remote_conversation_message_requests::migrate(&conn).unwrap();
    v20260801130000_remote_conversation_message_requests::migrate(&conn).unwrap();
    assert!(helpers::table_exists(
        &conn,
        "remote_conversation_message_requests"
    ));
}
