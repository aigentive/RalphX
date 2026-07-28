//! Tests for migration v20260728120000: remote request dedup + attachment metadata

use rusqlite::Connection;

use super::{helpers, v20260728120000_remote_request_dedup, MIGRATIONS};

fn migrated_db() -> Connection {
    let conn = Connection::open_in_memory().expect("in-memory database should open");
    v20260728120000_remote_request_dedup::migrate(&conn).expect("dedup migration should apply");
    conn
}

#[test]
fn migration_creates_both_tables_with_their_columns() {
    let conn = migrated_db();

    assert!(helpers::table_exists(&conn, "remote_request_dedup"));
    for column in [
        "device_id",
        "request_id",
        "args_hash",
        "outcome",
        "response",
        "created_at",
        "expires_at",
    ] {
        assert!(
            helpers::column_exists(&conn, "remote_request_dedup", column),
            "remote_request_dedup should contain {column}"
        );
    }

    assert!(helpers::table_exists(&conn, "remote_attachments"));
    for column in [
        "id",
        "device_id",
        "display_name",
        "mime",
        "size",
        "created_at",
    ] {
        assert!(
            helpers::column_exists(&conn, "remote_attachments", column),
            "remote_attachments should contain {column}"
        );
    }
}

#[test]
fn dedup_primary_key_is_device_scoped_so_two_devices_may_reuse_one_request_id() {
    let conn = migrated_db();

    conn.execute(
        "INSERT INTO remote_request_dedup
             (device_id, request_id, args_hash, outcome, response, created_at, expires_at)
         VALUES ('device-a', 'req-1', 'hash-a', 'ok', '{}', 'now', 'later')",
        [],
    )
    .expect("first device row should insert");

    // Same request id, different device: must be accepted — ids are minted client-side.
    conn.execute(
        "INSERT INTO remote_request_dedup
             (device_id, request_id, args_hash, outcome, response, created_at, expires_at)
         VALUES ('device-b', 'req-1', 'hash-b', 'ok', '{}', 'now', 'later')",
        [],
    )
    .expect("a second device must be able to use the same request id");

    // Same device and id again: refused by the composite primary key.
    assert!(
        conn.execute(
            "INSERT INTO remote_request_dedup
                 (device_id, request_id, args_hash, outcome, response, created_at, expires_at)
             VALUES ('device-a', 'req-1', 'hash-c', 'ok', '{}', 'now', 'later')",
            [],
        )
        .is_err(),
        "the same (device_id, request_id) must not insert twice"
    );
}

#[test]
fn attachment_size_is_stored_as_an_integer() {
    let conn = migrated_db();
    conn.execute(
        "INSERT INTO remote_attachments (id, device_id, display_name, mime, size, created_at)
         VALUES ('att-1', 'device-a', 'notes.txt', 'text/plain', 4096, 'now')",
        [],
    )
    .expect("attachment row should insert");

    let (size, kind): (i64, String) = conn
        .query_row(
            "SELECT size, typeof(size) FROM remote_attachments WHERE id = 'att-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("attachment row should be readable");
    assert_eq!(size, 4096);
    assert_eq!(kind, "integer", "quota arithmetic must never be float");
}

#[test]
fn migration_is_idempotent() {
    let conn = migrated_db();
    v20260728120000_remote_request_dedup::migrate(&conn)
        .expect("second migration should remain safe");
    assert!(helpers::table_exists(&conn, "remote_attachments"));
}

#[test]
fn stamp_sorts_after_every_previously_registered_migration() {
    let position = MIGRATIONS
        .iter()
        .position(|migration| migration.name == "remote_request_dedup")
        .expect("the dedup migration must be registered");

    assert_eq!(
        position,
        MIGRATIONS.len() - 1,
        "forward-only: the new migration must be registered last"
    );
    let stamp = MIGRATIONS[position].version;
    for migration in &MIGRATIONS[..position] {
        assert!(
            migration.version < stamp,
            "{} ({}) must sort before the new stamp {stamp}",
            migration.name,
            migration.version
        );
    }
}
