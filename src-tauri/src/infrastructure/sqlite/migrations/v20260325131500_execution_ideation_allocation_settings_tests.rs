//! Tests for migration v20260325131500: execution ideation allocation settings

use super::helpers;
use crate::infrastructure::sqlite::{open_memory_connection, run_migrations};

#[test]
fn test_migration_adds_project_ideation_max_to_execution_settings() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();

    assert!(helpers::column_exists(
        &conn,
        "execution_settings",
        "project_ideation_max"
    ));

    let value: i64 = conn
        .query_row(
            "SELECT project_ideation_max FROM execution_settings WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(value, 5);
}

#[test]
fn test_migration_adds_global_ideation_allocation_columns() {
    let conn = open_memory_connection().unwrap();
    run_migrations(&conn).unwrap();

    assert!(helpers::column_exists(
        &conn,
        "global_execution_settings",
        "global_ideation_max"
    ));
    assert!(helpers::column_exists(
        &conn,
        "global_execution_settings",
        "allow_ideation_borrow_idle_execution"
    ));

    let values: (i64, i64) = conn
        .query_row(
            "SELECT global_ideation_max, allow_ideation_borrow_idle_execution
             FROM global_execution_settings WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(values.0, 10);
    assert_eq!(values.1, 0);
}
