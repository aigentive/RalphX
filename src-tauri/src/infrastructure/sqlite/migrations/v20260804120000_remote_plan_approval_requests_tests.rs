use super::{helpers, v20260804120000_remote_plan_approval_requests};
use rusqlite::Connection;

#[test]
fn remote_plan_approval_migration_has_columns_indexes_round_trip_and_reapplies() {
    let conn = Connection::open_in_memory().expect("db");
    v20260804120000_remote_plan_approval_requests::migrate(&conn).expect("migrate");
    assert!(helpers::table_exists(
        &conn,
        "remote_plan_approval_requests"
    ));
    for column in [
        "id",
        "session_id",
        "artifact_id",
        "blueprint_artifact_id",
        "blueprint_artifact_version",
        "status",
        "error_code",
        "result_json",
        "claimed_at",
        "created_at",
        "updated_at",
    ] {
        assert!(
            helpers::column_exists(&conn, "remote_plan_approval_requests", column),
            "missing {column}"
        );
    }
    let indexes:Vec<String>=conn.prepare("SELECT name FROM sqlite_master WHERE type='index' AND tbl_name='remote_plan_approval_requests'").unwrap().query_map([],|row|row.get(0)).unwrap().collect::<Result<_,_>>().unwrap();
    assert!(indexes
        .iter()
        .any(|v| v == "idx_remote_plan_approval_requests_pending"));
    assert!(indexes
        .iter()
        .any(|v| v == "idx_remote_plan_approval_requests_session"));
    conn.execute("INSERT INTO remote_plan_approval_requests(id,session_id,artifact_id,blueprint_artifact_id,blueprint_artifact_version,status,result_json,created_at,updated_at)VALUES('r','s','a','b',2,'completed','{\"ok\":true}','2026-08-04T00:00:00Z','2026-08-04T00:00:00Z')",[]).unwrap();
    let tuple:(String,Option<u32>,String)=conn.query_row("SELECT blueprint_artifact_id,blueprint_artifact_version,result_json FROM remote_plan_approval_requests WHERE id='r'",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    assert_eq!(tuple, ("b".into(), Some(2), "{\"ok\":true}".into()));
    v20260804120000_remote_plan_approval_requests::migrate(&conn).expect("idempotent");
}
