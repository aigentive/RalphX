//! Tests for migration v20260804105225: remote resume requests

use rusqlite::Connection;

use super::helpers;
use super::v20260804105225_remote_resume_requests;

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

#[test]
fn test_migration_runs() {
    let conn = setup_test_db();
    v20260804105225_remote_resume_requests::migrate(&conn).unwrap();
    assert!(helpers::table_exists(&conn, "remote_resume_requests"));
    for column in [
        "id",
        "family",
        "action",
        "task_id",
        "project_id",
        "group_kind",
        "group_id",
        "force_restart",
        "note",
        "status",
        "error_code",
        "result_json",
        "claimed_at",
        "created_at",
        "updated_at",
    ] {
        assert!(
            helpers::column_exists(&conn, "remote_resume_requests", column),
            "missing {column}"
        );
    }
}
