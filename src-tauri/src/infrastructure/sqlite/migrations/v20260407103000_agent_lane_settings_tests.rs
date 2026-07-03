use crate::infrastructure::sqlite::{open_memory_connection, run_migrations};

#[test]
fn test_agent_lane_settings_table_exists() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();

    let count: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='agent_lane_settings'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn test_agent_lane_settings_unique_scope_lane_index_exists() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();

    let count: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_agent_lane_settings_scope_lane'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn test_agent_lane_settings_accepts_global_and_project_rows() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();

    conn.execute(
        "INSERT INTO agent_lane_settings (
            scope_type, scope_id, lane, harness, model, effort,
            approval_policy, sandbox_mode
         ) VALUES (
            'global', NULL, 'ideation_primary', 'codex', 'gpt-5.4', 'xhigh',
            'on-request', 'workspace-write'
         )",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO agent_lane_settings (
            scope_type, scope_id, lane, harness, model, effort
         ) VALUES (
            'project', 'project-1', 'ideation_verifier', 'codex', 'gpt-5.4-mini', 'medium'
         )",
        [],
    )
    .unwrap();

    let count: i32 = conn
        .query_row("SELECT COUNT(*) FROM agent_lane_settings", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(count, 2);
}
