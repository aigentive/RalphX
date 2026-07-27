//! Tests for migration v20260716214835: clickup strict git naming settings

use rusqlite::Connection;

use super::{
    v20260623074101_clickup_integration_settings,
    v20260716214835_clickup_strict_git_naming_settings,
};

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

#[test]
fn adds_non_null_strict_git_naming_defaults_to_existing_settings() {
    let conn = setup_test_db();
    v20260623074101_clickup_integration_settings::migrate(&conn).unwrap();
    conn.execute(
        "UPDATE clickup_integration_settings
            SET enabled = 1, workspace_id = 'workspace-1'
          WHERE id = 'default'",
        [],
    )
    .unwrap();

    v20260716214835_clickup_strict_git_naming_settings::migrate(&conn).unwrap();

    let stored = conn
        .query_row(
            "SELECT enabled, workspace_id, strict_git_naming_enabled,
                    branch_name_template, commit_subject_template, pr_title_template
               FROM clickup_integration_settings
              WHERE id = 'default'",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .unwrap();

    assert_eq!(stored.0, 1, "existing connection state is preserved");
    assert_eq!(stored.1, "workspace-1");
    assert_eq!(stored.2, 0, "strict naming remains opt-in");
    assert_eq!(stored.3, ":taskId:_:taskName:_:username:");
    assert_eq!(stored.4, ":taskId: - :taskName:");
    assert_eq!(stored.5, ":taskId: - :taskName:");
}

#[test]
fn migration_is_idempotent() {
    let conn = setup_test_db();
    v20260623074101_clickup_integration_settings::migrate(&conn).unwrap();

    v20260716214835_clickup_strict_git_naming_settings::migrate(&conn).unwrap();
    v20260716214835_clickup_strict_git_naming_settings::migrate(&conn).unwrap();

    let matching_columns = conn
        .prepare("PRAGMA table_info(clickup_integration_settings)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .into_iter()
        .filter(|column| {
            matches!(
                column.as_str(),
                "strict_git_naming_enabled"
                    | "branch_name_template"
                    | "commit_subject_template"
                    | "pr_title_template"
            )
        })
        .count();

    assert_eq!(matching_columns, 4);
}
