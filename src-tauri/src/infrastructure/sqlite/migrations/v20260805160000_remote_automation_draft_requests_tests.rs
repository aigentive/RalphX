use super::{helpers, v20260805160000_remote_automation_draft_requests};

#[test]
fn creates_remote_automation_draft_requests_schema_idempotently() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    v20260805160000_remote_automation_draft_requests::migrate(&conn).unwrap();
    for column in [
        "id",
        "project_id",
        "automation_id",
        "name",
        "authoring_mode",
        "base_ref_kind",
        "base_branch_mode",
        "base_branch",
        "status",
        "error_code",
        "result_json",
        "claimed_at",
        "created_at",
        "updated_at",
    ] {
        assert!(helpers::column_exists(
            &conn,
            "remote_automation_draft_requests",
            column
        ));
    }
    conn.execute("INSERT INTO remote_automation_draft_requests(id,project_id,automation_id,name,authoring_mode,base_ref_kind,base_branch_mode,base_branch,status,created_at,updated_at) VALUES('r','p','a','Nightly','reviewed','project_default','isolated',NULL,'pending','2026-08-05T00:00:00Z','2026-08-05T00:00:00Z')", []).unwrap();
    v20260805160000_remote_automation_draft_requests::migrate(&conn).unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT automation_id FROM remote_automation_draft_requests WHERE id='r'",
            [],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        "a"
    );
}
