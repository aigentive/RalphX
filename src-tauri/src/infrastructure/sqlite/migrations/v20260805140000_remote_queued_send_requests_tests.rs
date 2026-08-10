use super::{helpers, v20260805140000_remote_queued_send_requests};

#[test]
fn migration_creates_remote_queued_send_requests_idempotently() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    v20260805140000_remote_queued_send_requests::migrate(&conn).unwrap();
    v20260805140000_remote_queued_send_requests::migrate(&conn).unwrap();
    for column in [
        "id",
        "conversation_id",
        "queued_message_id",
        "expected_active_run_id",
        "status",
        "error_code",
        "result_json",
        "claimed_at",
        "created_at",
        "updated_at",
    ] {
        assert!(helpers::column_exists(
            &conn,
            "remote_queued_send_requests",
            column
        ));
    }
}
