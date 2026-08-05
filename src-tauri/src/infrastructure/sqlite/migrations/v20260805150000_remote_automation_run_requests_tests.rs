use super::{helpers, v20260805150000_remote_automation_run_requests};
use rusqlite::Connection;

#[test]
fn creates_remote_automation_run_requests_schema_idempotently() {
    let conn = Connection::open_in_memory().expect("open");
    v20260805150000_remote_automation_run_requests::migrate(&conn).expect("migrate");
    for column in [
        "id",
        "automation_id",
        "kind",
        "expected_run_id",
        "status",
        "error_code",
        "result_json",
        "claimed_at",
        "created_at",
        "updated_at",
    ] {
        assert!(
            helpers::column_exists(&conn, "remote_automation_run_requests", column),
            "missing {column}"
        );
    }
    assert!(helpers::index_exists(
        &conn,
        "idx_remote_automation_run_requests_pending"
    ));
    assert!(helpers::index_exists(
        &conn,
        "idx_remote_automation_run_requests_automation_kind"
    ));
    conn.execute("INSERT INTO remote_automation_run_requests(id,automation_id,kind,expected_run_id,status,error_code,result_json,claimed_at,created_at,updated_at) VALUES('r','a','retryJudge','run-7','completed',NULL,'{\"scheduled\":false}','2026-08-05T00:01:00Z','2026-08-05T00:00:00Z','2026-08-05T00:02:00Z')", []).expect("insert");
    assert_eq!(
        conn.query_row(
            "SELECT automation_id || '|' || kind || '|' || expected_run_id || '|' || status || '|' || result_json || '|' || claimed_at || '|' || created_at || '|' || updated_at FROM remote_automation_run_requests WHERE id='r'",
            [],
            |row| row.get::<_, String>(0)
        )
        .expect("query"),
        "a|retryJudge|run-7|completed|{\"scheduled\":false}|2026-08-05T00:01:00Z|2026-08-05T00:00:00Z|2026-08-05T00:02:00Z"
    );
    v20260805150000_remote_automation_run_requests::migrate(&conn).expect("idempotent");
}
