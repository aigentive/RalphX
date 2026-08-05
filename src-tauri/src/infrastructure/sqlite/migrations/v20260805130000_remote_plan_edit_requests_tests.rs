use super::{helpers, v20260805130000_remote_plan_edit_requests};
use rusqlite::Connection;

#[test]
fn remote_plan_edit_migration_has_columns_indexes_round_trip_and_reapplies() {
    let conn = Connection::open_in_memory().expect("db");
    v20260805130000_remote_plan_edit_requests::migrate(&conn).expect("migrate");
    assert!(helpers::table_exists(&conn, "remote_plan_edit_requests"));
    for column in [
        "id",
        "artifact_id",
        "content",
        "expected_version",
        "status",
        "error_code",
        "result_json",
        "claimed_at",
        "created_at",
        "updated_at",
    ] {
        assert!(
            helpers::column_exists(&conn, "remote_plan_edit_requests", column),
            "missing {column}"
        );
    }
    let indexes:Vec<String>=conn.prepare("SELECT name FROM sqlite_master WHERE type='index' AND tbl_name='remote_plan_edit_requests'").expect("prepare").query_map([],|row|row.get(0)).expect("query").collect::<Result<_,_>>().expect("collect");
    assert!(indexes
        .iter()
        .any(|v| v == "idx_remote_plan_edit_requests_dispatch"));
    assert!(indexes
        .iter()
        .any(|v| v == "idx_remote_plan_edit_requests_artifact"));
    conn.execute("INSERT INTO remote_plan_edit_requests(id,artifact_id,content,expected_version,status,result_json,created_at,updated_at) VALUES('r','a','new',4,'completed','{\"ok\":true}','2026-08-05T00:00:00Z','2026-08-05T00:00:00Z')",[]).expect("insert");
    let tuple:(String,i64,String)=conn.query_row("SELECT content,expected_version,result_json FROM remote_plan_edit_requests WHERE id='r'",[],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).expect("read");
    assert_eq!(tuple, ("new".into(), 4, "{\"ok\":true}".into()));
    v20260805130000_remote_plan_edit_requests::migrate(&conn).expect("idempotent");
}
