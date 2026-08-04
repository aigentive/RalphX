use super::{helpers, v20260805120000_remote_finalize_decision_requests};
use rusqlite::Connection;

#[test]
fn remote_finalize_decision_migration_has_columns_indexes_round_trip_and_reapplies() {
    let conn = Connection::open_in_memory().expect("db");
    v20260805120000_remote_finalize_decision_requests::migrate(&conn).expect("migrate");
    assert!(helpers::table_exists(
        &conn,
        "remote_finalize_decision_requests"
    ));
    for column in [
        "id",
        "session_id",
        "decision",
        "status",
        "error_code",
        "result_json",
        "claimed_at",
        "created_at",
        "updated_at",
    ] {
        assert!(
            helpers::column_exists(&conn, "remote_finalize_decision_requests", column),
            "missing {column}"
        );
    }
    let indexes: Vec<String> = conn.prepare("SELECT name FROM sqlite_master WHERE type='index' AND tbl_name='remote_finalize_decision_requests'").expect("prepare").query_map([], |row| row.get(0)).expect("query").collect::<Result<_, _>>().expect("collect");
    assert!(indexes
        .iter()
        .any(|value| value == "idx_remote_finalize_decision_requests_pending"));
    assert!(indexes
        .iter()
        .any(|value| value == "idx_remote_finalize_decision_requests_session"));
    conn.execute("INSERT INTO remote_finalize_decision_requests(id,session_id,decision,status,result_json,created_at,updated_at) VALUES('r','s','accept','completed','{\"ok\":true}','2026-08-05T00:00:00Z','2026-08-05T00:00:00Z')", []).expect("insert");
    let tuple: (String, String) = conn
        .query_row(
            "SELECT decision,result_json FROM remote_finalize_decision_requests WHERE id='r'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read");
    assert_eq!(tuple, ("accept".into(), "{\"ok\":true}".into()));
    v20260805120000_remote_finalize_decision_requests::migrate(&conn).expect("idempotent");
}
