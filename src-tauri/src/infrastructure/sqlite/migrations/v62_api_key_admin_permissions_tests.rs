use rusqlite::Connection;

use crate::infrastructure::sqlite::migrations::v62_api_key_admin_permissions::migrate;

fn setup_api_keys_table(conn: &Connection) {
    conn.execute_batch(
        "CREATE TABLE api_keys (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            key_hash TEXT NOT NULL UNIQUE,
            key_prefix TEXT NOT NULL,
            permissions INTEGER NOT NULL DEFAULT 3,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
            revoked_at TEXT,
            last_used_at TEXT,
            grace_expires_at TEXT,
            metadata TEXT
        );",
    )
    .unwrap();
}

#[test]
fn upgrades_permissions_3_to_7() {
    let conn = Connection::open_in_memory().unwrap();
    setup_api_keys_table(&conn);

    conn.execute(
        "INSERT INTO api_keys (id, name, key_hash, key_prefix, permissions) VALUES ('k1', 'Key 1', 'hash1', 'rph_', 3)",
        [],
    )
    .unwrap();

    migrate(&conn).unwrap();

    let permissions: i64 = conn
        .query_row(
            "SELECT permissions FROM api_keys WHERE id = 'k1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(permissions, 7, "permissions=3 key should be upgraded to 7");
}

#[test]
fn does_not_upgrade_custom_permissions_1() {
    let conn = Connection::open_in_memory().unwrap();
    setup_api_keys_table(&conn);

    conn.execute(
        "INSERT INTO api_keys (id, name, key_hash, key_prefix, permissions) VALUES ('k1', 'Key 1', 'hash1', 'rph_', 1)",
        [],
    )
    .unwrap();

    migrate(&conn).unwrap();

    let permissions: i64 = conn
        .query_row(
            "SELECT permissions FROM api_keys WHERE id = 'k1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(permissions, 1, "permissions=1 key should not be changed");
}

#[test]
fn does_not_upgrade_custom_permissions_2() {
    let conn = Connection::open_in_memory().unwrap();
    setup_api_keys_table(&conn);

    conn.execute(
        "INSERT INTO api_keys (id, name, key_hash, key_prefix, permissions) VALUES ('k1', 'Key 1', 'hash1', 'rph_', 2)",
        [],
    )
    .unwrap();

    migrate(&conn).unwrap();

    let permissions: i64 = conn
        .query_row(
            "SELECT permissions FROM api_keys WHERE id = 'k1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(permissions, 2, "permissions=2 key should not be changed");
}

#[test]
fn does_not_upgrade_revoked_keys() {
    let conn = Connection::open_in_memory().unwrap();
    setup_api_keys_table(&conn);

    conn.execute(
        "INSERT INTO api_keys (id, name, key_hash, key_prefix, permissions, revoked_at)
         VALUES ('k1', 'Key 1', 'hash1', 'rph_', 3, '2024-01-01T00:00:00Z')",
        [],
    )
    .unwrap();

    migrate(&conn).unwrap();

    let permissions: i64 = conn
        .query_row(
            "SELECT permissions FROM api_keys WHERE id = 'k1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        permissions, 3,
        "revoked key with permissions=3 should not be upgraded"
    );
}

#[test]
fn upgrades_only_non_revoked_permissions_3_keys() {
    let conn = Connection::open_in_memory().unwrap();
    setup_api_keys_table(&conn);

    // Active key with old default — should be upgraded
    conn.execute(
        "INSERT INTO api_keys (id, name, key_hash, key_prefix, permissions) VALUES ('k1', 'Active', 'hash1', 'rph_', 3)",
        [],
    )
    .unwrap();
    // Revoked key with old default — should NOT be upgraded
    conn.execute(
        "INSERT INTO api_keys (id, name, key_hash, key_prefix, permissions, revoked_at)
         VALUES ('k2', 'Revoked', 'hash2', 'rph_', 3, '2024-01-01T00:00:00Z')",
        [],
    )
    .unwrap();
    // Custom permissions — should NOT be changed
    conn.execute(
        "INSERT INTO api_keys (id, name, key_hash, key_prefix, permissions) VALUES ('k3', 'Custom', 'hash3', 'rph_', 1)",
        [],
    )
    .unwrap();

    migrate(&conn).unwrap();

    let p1: i64 = conn
        .query_row(
            "SELECT permissions FROM api_keys WHERE id = 'k1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let p2: i64 = conn
        .query_row(
            "SELECT permissions FROM api_keys WHERE id = 'k2'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let p3: i64 = conn
        .query_row(
            "SELECT permissions FROM api_keys WHERE id = 'k3'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    assert_eq!(p1, 7, "active permissions=3 key should be upgraded to 7");
    assert_eq!(p2, 3, "revoked permissions=3 key should remain 3");
    assert_eq!(p3, 1, "custom permissions=1 key should remain 1");
}

#[test]
fn idempotent_on_empty_table() {
    let conn = Connection::open_in_memory().unwrap();
    setup_api_keys_table(&conn);

    migrate(&conn).unwrap();
    migrate(&conn).unwrap(); // Should not fail
}

#[test]
fn idempotent_on_already_upgraded_key() {
    let conn = Connection::open_in_memory().unwrap();
    setup_api_keys_table(&conn);

    conn.execute(
        "INSERT INTO api_keys (id, name, key_hash, key_prefix, permissions) VALUES ('k1', 'Key 1', 'hash1', 'rph_', 7)",
        [],
    )
    .unwrap();

    migrate(&conn).unwrap();
    migrate(&conn).unwrap(); // Should not fail or change value unexpectedly

    let permissions: i64 = conn
        .query_row(
            "SELECT permissions FROM api_keys WHERE id = 'k1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(permissions, 7, "already-upgraded key should remain 7");
}
