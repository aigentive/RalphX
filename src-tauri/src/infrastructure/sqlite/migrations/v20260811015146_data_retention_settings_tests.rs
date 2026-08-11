//! Tests for migration v20260811015146: data retention settings

use rusqlite::Connection;

use super::v20260811015146_data_retention_settings::migrate;

fn column_names(conn: &Connection) -> Vec<String> {
    conn.prepare("PRAGMA table_info(data_retention_settings)")
        .expect("prepare table inspection")
        .query_map([], |row| row.get::<_, String>(1))
        .expect("read table columns")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect table columns")
}

fn insert_singleton(conn: &Connection, id: i64) -> rusqlite::Result<usize> {
    conn.execute(
        "INSERT INTO data_retention_settings (
            id, payload_retention_enabled, payload_retention_days,
            payload_retention_archived_days, payload_retention_batch_rows, updated_at
         ) VALUES (?1, 1, 90, 7, 500, '2026-08-11T00:00:00+00:00')",
        [id],
    )
}

#[test]
fn migration_is_idempotent_and_creates_every_policy_column() {
    let conn = Connection::open_in_memory().expect("open migration fixture");

    migrate(&conn).expect("first migration succeeds");
    migrate(&conn).expect("second migration succeeds");

    let columns = column_names(&conn);
    for expected in [
        "id",
        "payload_retention_enabled",
        "payload_retention_days",
        "payload_retention_archived_days",
        "payload_size_budget_bytes",
        "size_budget_confirmed_at",
        "payload_retention_batch_rows",
        "seeded_pristine",
        "size_budget_advised",
        "last_run_at",
        "last_run_pruned_rows",
        "last_run_payload_bytes",
        "last_run_payload_rows",
        "updated_at",
    ] {
        assert!(
            columns.iter().any(|column| column == expected),
            "missing column {expected}"
        );
    }
}

#[test]
fn migration_inserts_no_row_so_seeding_stays_repo_owned() {
    let conn = Connection::open_in_memory().expect("open migration fixture");
    migrate(&conn).expect("migration succeeds");

    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM data_retention_settings", [], |row| {
            row.get(0)
        })
        .expect("count rows");
    assert_eq!(rows, 0);
}

#[test]
fn table_is_single_row_by_construction() {
    let conn = Connection::open_in_memory().expect("open migration fixture");
    migrate(&conn).expect("migration succeeds");

    insert_singleton(&conn, 1).expect("insert singleton row");

    assert!(
        insert_singleton(&conn, 2).is_err(),
        "id must be constrained to 1"
    );
}

#[test]
fn size_budget_columns_default_to_null_so_shipped_state_is_time_window_only() {
    let conn = Connection::open_in_memory().expect("open migration fixture");
    migrate(&conn).expect("migration succeeds");

    insert_singleton(&conn, 1).expect("insert singleton row");

    let (budget, confirmed_at, pristine, advised): (Option<i64>, Option<String>, i64, i64) = conn
        .query_row(
            "SELECT payload_size_budget_bytes, size_budget_confirmed_at, seeded_pristine, size_budget_advised
             FROM data_retention_settings WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("read shipped defaults");

    assert_eq!(budget, None);
    assert_eq!(confirmed_at, None);
    assert_eq!(pristine, 1);
    assert_eq!(advised, 0);
}
