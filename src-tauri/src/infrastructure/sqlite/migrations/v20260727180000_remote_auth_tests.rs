//! Tests for migration v20260727180000: remote access auth tables

use rusqlite::Connection;

use super::{helpers, v20260727180000_remote_auth};

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("in-memory database should open")
}

#[test]
fn migration_creates_the_five_remote_auth_tables() {
    let conn = setup_test_db();

    v20260727180000_remote_auth::migrate(&conn)
        .expect("migration should create remote auth tables");

    for table in [
        "remote_devices",
        "remote_pairing_codes",
        "remote_sessions",
        "remote_ws_tickets",
        "remote_audit_log",
    ] {
        assert!(helpers::table_exists(&conn, table), "{table} should exist");
    }
}

#[test]
fn remote_device_and_pairing_columns_match_the_spec_shape() {
    let conn = setup_test_db();
    v20260727180000_remote_auth::migrate(&conn)
        .expect("migration should create remote auth tables");

    for column in [
        "id",
        "name",
        "token_hash",
        "token_prefix",
        "scopes",
        "created_at",
        "last_seen_at",
        "revoked_at",
    ] {
        assert!(
            helpers::column_exists(&conn, "remote_devices", column),
            "remote_devices should contain {column}"
        );
    }
    for column in [
        "id",
        "code_hash",
        "scopes",
        "created_at",
        "expires_at",
        "consumed_at",
    ] {
        assert!(
            helpers::column_exists(&conn, "remote_pairing_codes", column),
            "remote_pairing_codes should contain {column}"
        );
    }
    for column in ["ticket_hash", "device_id", "expires_at", "consumed_at"] {
        assert!(
            helpers::column_exists(&conn, "remote_ws_tickets", column),
            "remote_ws_tickets should contain {column}"
        );
    }
}

/// A-9: nothing in the schema invites storing a plaintext credential — the only credential
/// columns are hashes, and they are unique so a hash collision cannot silently pair twice.
#[test]
fn credential_columns_are_hash_only_and_unique() {
    let conn = setup_test_db();
    v20260727180000_remote_auth::migrate(&conn)
        .expect("migration should create remote auth tables");

    conn.execute(
        "INSERT INTO remote_devices (id, name, token_hash, token_prefix, scopes, created_at)
         VALUES ('d1', 'laptop', 'hash-a', 'rxd_live_aaaa', '[\"ui:read\"]', '2026-07-27T00:00:00Z')",
        [],
    )
    .expect("first device inserts");
    assert!(
        conn.execute(
            "INSERT INTO remote_devices (id, name, token_hash, token_prefix, scopes, created_at)
             VALUES ('d2', 'phone', 'hash-a', 'rxd_live_bbbb', '[\"ui:read\"]', '2026-07-27T00:00:00Z')",
            [],
        )
        .is_err(),
        "a duplicate token hash must be refused"
    );

    conn.execute(
        "INSERT INTO remote_pairing_codes (id, code_hash, scopes, created_at, expires_at)
         VALUES ('c1', 'code-a', '[\"ui:read\"]', '2026-07-27T00:00:00Z', '2026-07-27T00:10:00Z')",
        [],
    )
    .expect("first pairing code inserts");
    assert!(
        conn.execute(
            "INSERT INTO remote_pairing_codes (id, code_hash, scopes, created_at, expires_at)
             VALUES ('c2', 'code-a', '[\"ui:read\"]', '2026-07-27T00:00:00Z', '2026-07-27T00:10:00Z')",
            [],
        )
        .is_err(),
        "a duplicate pairing-code hash must be refused"
    );

    assert!(
        !helpers::column_exists(&conn, "remote_devices", "token"),
        "no plaintext token column may exist"
    );
    assert!(
        !helpers::column_exists(&conn, "remote_pairing_codes", "code"),
        "no plaintext pairing-code column may exist"
    );
}

#[test]
fn migration_is_idempotent_and_seeds_no_devices() {
    let conn = setup_test_db();

    v20260727180000_remote_auth::migrate(&conn).expect("first migration should succeed");
    v20260727180000_remote_auth::migrate(&conn).expect("second migration should remain safe");

    // A-2: a freshly migrated host has zero paired devices and grants nothing by default.
    let device_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM remote_devices", [], |row| row.get(0))
        .expect("devices should be queryable");
    let code_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM remote_pairing_codes", [], |row| {
            row.get(0)
        })
        .expect("pairing codes should be queryable");
    assert_eq!(device_count, 0);
    assert_eq!(code_count, 0);
}
