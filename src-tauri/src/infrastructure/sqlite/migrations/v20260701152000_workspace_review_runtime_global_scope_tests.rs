use super::helpers::index_exists;
use super::{
    v20260701143000_workspace_review_runtime_settings,
    v20260701152000_workspace_review_runtime_global_scope,
};
use rusqlite::Connection;

#[test]
fn test_workspace_review_runtime_global_scope_repair_dedupes_null_rows() {
    let conn = Connection::open_in_memory().unwrap();

    v20260701143000_workspace_review_runtime_settings::migrate(&conn).unwrap();
    conn.execute(
        "INSERT INTO workspace_review_runtime_settings
            (scope_type, scope_id, provider, model, effort)
         VALUES ('global', NULL, 'codex', 'gpt-5.4-mini', 'medium')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO workspace_review_runtime_settings
            (scope_type, scope_id, provider, model, effort)
         VALUES ('global', NULL, 'codex', 'gpt-5.4', 'high')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO workspace_review_runtime_settings
            (scope_type, scope_id, provider, model, effort)
         VALUES ('project', 'project-1', 'codex', 'gpt-5.4', 'high')",
        [],
    )
    .unwrap();

    v20260701152000_workspace_review_runtime_global_scope::migrate(&conn).unwrap();

    assert!(index_exists(
        &conn,
        "idx_workspace_review_runtime_settings_global_provider"
    ));
    let global_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM workspace_review_runtime_settings
             WHERE scope_type = 'global' AND provider = 'codex'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(global_count, 1);
    let global_scope_id: String = conn
        .query_row(
            "SELECT scope_id FROM workspace_review_runtime_settings
             WHERE scope_type = 'global' AND provider = 'codex'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(global_scope_id, "");
    let duplicate_global = conn.execute(
        "INSERT INTO workspace_review_runtime_settings
            (scope_type, scope_id, provider, model, effort)
         VALUES ('global', NULL, 'codex', 'gpt-5.5', 'max')",
        [],
    );
    assert!(duplicate_global.is_err());
}
