//! Tests for migration v20260611191722: agent workspace pr automation defaults

use super::helpers;
use crate::infrastructure::sqlite::{open_memory_connection, run_migrations};

#[test]
fn test_migration_adds_agent_workspace_pr_automation_default_columns() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();

    assert!(helpers::column_exists(
        &conn,
        "execution_settings",
        "agent_workspace_pr_autofix_default"
    ));
    assert!(helpers::column_exists(
        &conn,
        "execution_settings",
        "agent_workspace_pr_auto_merge_default"
    ));

    let values: (i64, i64) = conn
        .query_row(
            "SELECT agent_workspace_pr_autofix_default, agent_workspace_pr_auto_merge_default
             FROM execution_settings WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(values, (0, 0));
}
