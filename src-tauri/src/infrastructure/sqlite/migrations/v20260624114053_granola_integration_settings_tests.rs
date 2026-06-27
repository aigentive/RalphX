//! Tests for migration: granola integration settings

use crate::infrastructure::sqlite::migrations::v20260624114053_granola_integration_settings;
use crate::infrastructure::sqlite::open_memory_connection;

#[test]
fn creates_granola_integration_settings_seed_row() {
    let conn = open_memory_connection().unwrap();
    v20260624114053_granola_integration_settings::migrate(&conn).unwrap();

    let row = conn
        .query_row(
            "SELECT id, enabled, token_secret_ref, validation_status
             FROM granola_integration_settings
             WHERE id = 'default'",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .unwrap();

    assert_eq!(row.0, "default");
    assert_eq!(row.1, 0);
    assert_eq!(row.2, None);
    assert_eq!(row.3, "not_configured");
}

#[test]
fn migration_is_idempotent() {
    let conn = open_memory_connection().unwrap();
    v20260624114053_granola_integration_settings::migrate(&conn).unwrap();
    v20260624114053_granola_integration_settings::migrate(&conn).unwrap();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM granola_integration_settings",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(count, 1);
}

#[test]
fn migration_rejects_non_default_id() {
    let conn = open_memory_connection().unwrap();
    v20260624114053_granola_integration_settings::migrate(&conn).unwrap();

    // The singleton CHECK constraint must reject any id other than 'default'.
    let result = conn.execute(
        "INSERT INTO granola_integration_settings (id, validation_status) VALUES ('other', 'pending')",
        [],
    );

    assert!(
        result.is_err(),
        "non-default id should violate the singleton CHECK constraint"
    );
}
