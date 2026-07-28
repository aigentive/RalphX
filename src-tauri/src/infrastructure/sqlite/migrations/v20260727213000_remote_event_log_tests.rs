//! Tests for migration v20260727213000: durable remote event log + seq high-water

use rusqlite::Connection;

use super::{helpers, v20260727161131_remote_host_settings, v20260727213000_remote_event_log};

fn migrated_db() -> Connection {
    let conn = Connection::open_in_memory().expect("in-memory database should open");
    v20260727161131_remote_host_settings::migrate(&conn)
        .expect("remote host settings must exist before the event log migration");
    v20260727213000_remote_event_log::migrate(&conn).expect("event log migration should apply");
    conn
}

#[test]
fn migration_creates_the_event_log_and_high_water_column() {
    let conn = migrated_db();

    assert!(helpers::table_exists(&conn, "remote_event_log"));
    for column in ["seq", "epoch", "name", "payload", "created_at"] {
        assert!(
            helpers::column_exists(&conn, "remote_event_log", column),
            "remote_event_log should contain {column}"
        );
    }
    assert!(helpers::column_exists(
        &conn,
        "remote_host_settings",
        "event_seq_high_water"
    ));
}

#[test]
fn seq_is_not_autoincrement_so_the_sequencer_stays_the_only_assigner() {
    let conn = migrated_db();

    let table_sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'remote_event_log'",
            [],
            |row| row.get(0),
        )
        .expect("table DDL should be readable");
    assert!(
        !table_sql.to_ascii_uppercase().contains("AUTOINCREMENT"),
        "seq must be sequencer-authored, not SQLite-authored: {table_sql}"
    );

    // A sequencer-authored seq is accepted verbatim, including a gap left by a prune.
    conn.execute(
        "INSERT INTO remote_event_log (seq, epoch, name, payload) VALUES (9, 'e1', 'task:created', '{}')",
        [],
    )
    .expect("explicit seq should be accepted");
    let stored: i64 = conn
        .query_row("SELECT seq FROM remote_event_log", [], |row| row.get(0))
        .expect("row should be readable");
    assert_eq!(stored, 9);
}

#[test]
fn duplicate_seq_is_rejected_so_a_seq_can_never_be_reused() {
    let conn = migrated_db();

    conn.execute(
        "INSERT INTO remote_event_log (seq, epoch, name, payload) VALUES (1, 'e1', 'task:created', '{}')",
        [],
    )
    .expect("first row should insert");
    assert!(
        conn.execute(
            "INSERT INTO remote_event_log (seq, epoch, name, payload) VALUES (1, 'e2', 'task:created', '{}')",
            [],
        )
        .is_err(),
        "reusing a seq must be refused by the primary key"
    );
}

#[test]
fn high_water_defaults_to_zero_on_an_existing_settings_row() {
    let conn = Connection::open_in_memory().expect("in-memory database should open");
    v20260727161131_remote_host_settings::migrate(&conn).expect("settings migration should apply");
    conn.execute(
        "INSERT INTO remote_host_settings (id, enabled, exposure_mode, port, environment_id)
         VALUES (1, 0, 'serve', 3849, '8d3d6a07-8e85-4e91-97ce-915fc038fdb2')",
        [],
    )
    .expect("pre-existing settings row should insert");

    v20260727213000_remote_event_log::migrate(&conn).expect("event log migration should apply");

    let high_water: i64 = conn
        .query_row(
            "SELECT event_seq_high_water FROM remote_host_settings WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .expect("high water should be readable");
    assert_eq!(high_water, 0);
}

#[test]
fn migration_is_idempotent() {
    let conn = migrated_db();
    v20260727213000_remote_event_log::migrate(&conn).expect("second migration should remain safe");
    assert!(helpers::column_exists(
        &conn,
        "remote_host_settings",
        "event_seq_high_water"
    ));
}
