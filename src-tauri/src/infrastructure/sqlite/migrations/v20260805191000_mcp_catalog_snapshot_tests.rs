use rusqlite::Connection;

use super::v20260805191000_mcp_catalog_snapshot::migrate;

#[test]
fn migration_creates_snapshot_columns_and_is_idempotent() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn).unwrap();
    migrate(&conn).unwrap();

    let columns = conn
        .prepare("PRAGMA table_info(mcp_catalog_snapshot)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        columns,
        [
            "scope_project_id",
            "provider",
            "response_json",
            "captured_at"
        ]
    );
}

#[test]
fn migration_keeps_global_and_project_scope_keys_distinct() {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn).unwrap();
    let response = r#"{"eligible_providers":["codex"],"eligible_default_provider":"codex","probed_at":"2026-08-05T18:00:00Z","probe_stale":false,"provider_diagnostics":{},"policy_diagnostics":[],"servers":[]}"#;

    conn.execute(
        "INSERT INTO mcp_catalog_snapshot VALUES (NULL, 'codex', ?1, 'global-time')",
        [response],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO mcp_catalog_snapshot VALUES ('project-1', 'codex', ?1, 'project-time')",
        [response],
    )
    .unwrap();

    let rows = conn
        .query_row("SELECT COUNT(*) FROM mcp_catalog_snapshot", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap();
    assert_eq!(rows, 2);
}
