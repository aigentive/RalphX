use super::helpers::{index_exists, table_exists};
use super::v20260701143000_workspace_review_runtime_settings;
use rusqlite::Connection;

#[test]
fn test_workspace_review_runtime_settings_table_exists() {
    let conn = Connection::open_in_memory().unwrap();

    v20260701143000_workspace_review_runtime_settings::migrate(&conn).unwrap();

    assert!(table_exists(&conn, "workspace_review_runtime_settings"));
    assert!(index_exists(
        &conn,
        "idx_workspace_review_runtime_settings_scope_provider"
    ));
    assert!(index_exists(
        &conn,
        "idx_workspace_review_runtime_settings_scope"
    ));
}

#[test]
fn test_workspace_review_runtime_settings_accepts_scoped_provider_rows() {
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
         VALUES ('project', 'project-1', 'codex', 'gpt-5.4', 'high')",
        [],
    )
    .unwrap();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM workspace_review_runtime_settings",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);
}
