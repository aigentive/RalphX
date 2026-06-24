//! Tests for migration: clickup integration settings

use crate::infrastructure::sqlite::migrations::v20260623074101_clickup_integration_settings;
use crate::infrastructure::sqlite::open_memory_connection;

#[test]
fn creates_clickup_integration_settings_seed_row() {
    let conn = open_memory_connection().unwrap();
    v20260623074101_clickup_integration_settings::migrate(&conn).unwrap();

    let row = conn
        .query_row(
            "SELECT id, enabled, token_secret_ref, workspace_id, validation_status, task_search_available
             FROM clickup_integration_settings
             WHERE id = 'default'",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            },
        )
        .unwrap();

    assert_eq!(row.0, "default");
    assert_eq!(row.1, 0);
    assert_eq!(row.2, None);
    assert_eq!(row.3, None);
    assert_eq!(row.4, "not_configured");
    assert_eq!(row.5, 0);
}

#[test]
fn migration_is_idempotent() {
    let conn = open_memory_connection().unwrap();
    v20260623074101_clickup_integration_settings::migrate(&conn).unwrap();
    v20260623074101_clickup_integration_settings::migrate(&conn).unwrap();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM clickup_integration_settings",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(count, 1);
}
