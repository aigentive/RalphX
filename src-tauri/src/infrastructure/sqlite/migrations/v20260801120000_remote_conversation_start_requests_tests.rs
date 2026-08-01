//! Tests for migration v20260801120000: remote conversation start requests

use rusqlite::Connection;

use super::helpers;
use super::v20260801120000_remote_conversation_start_requests;

#[test]
fn migration_creates_remote_conversation_start_requests_with_expected_columns() {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");

    v20260801120000_remote_conversation_start_requests::migrate(&conn).unwrap();

    assert!(helpers::table_exists(
        &conn,
        "remote_conversation_start_requests"
    ));
    for column in [
        "id",
        "conversation_id",
        "project_id",
        "content",
        "provider",
        "model",
        "effort",
        "mode",
        "status",
        "error_code",
        "requested_by_device_id",
        "agent_run_id",
        "claimed_at",
        "created_at",
        "updated_at",
    ] {
        assert!(
            helpers::column_exists(&conn, "remote_conversation_start_requests", column),
            "missing {column}"
        );
    }
}
