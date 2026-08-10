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

    let indexes: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'index' AND tbl_name = 'remote_resume_requests'")
        .expect("prepare index query")
        .query_map([], |row| row.get(0))
        .expect("query indexes")
        .collect::<Result<_, _>>()
        .expect("collect indexes");
    assert!(indexes
        .iter()
        .any(|name| name == "idx_remote_resume_requests_pending"));
    assert!(indexes
        .iter()
        .any(|name| name == "idx_remote_resume_requests_task"));

    conn.execute(
        "INSERT INTO remote_resume_requests
         (id, family, project_id, status, created_at, updated_at)
         VALUES (?1, 'execution', ?2, 'pending', ?3, ?3)",
        rusqlite::params!["request-1", "project-1", "2026-08-04T10:52:25Z"],
    )
    .expect("insert request through migrated schema");
    let stored: (String, String) = conn
        .query_row(
            "SELECT family, status FROM remote_resume_requests WHERE id = 'request-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read request");
    assert_eq!(stored, ("execution".into(), "pending".into()));

    v20260804105225_remote_resume_requests::migrate(&conn).expect("migration must be idempotent");
}
