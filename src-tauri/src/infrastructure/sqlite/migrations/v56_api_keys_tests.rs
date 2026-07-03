//! Tests for migration v56: api_keys, api_key_projects, external_events, api_audit_log

use rusqlite::Connection;

use super::helpers;
use super::v56_api_keys;

// ---------------------------------------------------------------------------
// Setup helper
// ---------------------------------------------------------------------------

/// Set up a minimal in-memory database for v56 testing.
/// Includes the `projects` table that `api_key_projects` references via FK.
fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");

    conn.execute_batch(
        "PRAGMA foreign_keys = ON;

        CREATE TABLE projects (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            working_directory TEXT NOT NULL DEFAULT '/tmp'
        );",
    )
    .expect("Failed to create test schema");

    conn
}

// ---------------------------------------------------------------------------
// Table creation tests
// ---------------------------------------------------------------------------

#[test]
fn test_api_keys_table_created() {
    let conn = setup_test_db();

    assert!(
        !helpers::table_exists(&conn, "api_keys"),
        "api_keys should not exist before migration"
    );

    v56_api_keys::migrate(&conn).unwrap();

    assert!(
        helpers::table_exists(&conn, "api_keys"),
        "api_keys table should exist after migration"
    );
}

#[test]
fn test_api_key_projects_table_created() {
    let conn = setup_test_db();

    v56_api_keys::migrate(&conn).unwrap();

    assert!(
        helpers::table_exists(&conn, "api_key_projects"),
        "api_key_projects table should exist after migration"
    );
}

#[test]
fn test_external_events_table_created() {
    let conn = setup_test_db();

    v56_api_keys::migrate(&conn).unwrap();

    assert!(
        helpers::table_exists(&conn, "external_events"),
        "external_events table should exist after migration"
    );
}

#[test]
fn test_api_audit_log_table_created() {
    let conn = setup_test_db();

    v56_api_keys::migrate(&conn).unwrap();

    assert!(
        helpers::table_exists(&conn, "api_audit_log"),
        "api_audit_log table should exist after migration"
    );
}

// ---------------------------------------------------------------------------
// Index creation tests
// ---------------------------------------------------------------------------

#[test]
fn test_indexes_created() {
    let conn = setup_test_db();

    v56_api_keys::migrate(&conn).unwrap();

    assert!(
        helpers::index_exists(&conn, "idx_api_keys_hash"),
        "idx_api_keys_hash should exist"
    );
    assert!(
        helpers::index_exists(&conn, "idx_api_key_projects_project"),
        "idx_api_key_projects_project should exist"
    );
    assert!(
        helpers::index_exists(&conn, "idx_external_events_project"),
        "idx_external_events_project should exist"
    );
    assert!(
        helpers::index_exists(&conn, "idx_external_events_created"),
        "idx_external_events_created should exist"
    );
    assert!(
        helpers::index_exists(&conn, "idx_audit_log_key"),
        "idx_audit_log_key should exist"
    );
}

// ---------------------------------------------------------------------------
// Insert / default values tests
// ---------------------------------------------------------------------------

#[test]
fn test_api_key_insert_with_defaults() {
    let conn = setup_test_db();
    v56_api_keys::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO api_keys (id, name, key_hash, key_prefix) VALUES ('k1', 'My Key', 'hash1', 'rph_')",
        [],
    )
    .unwrap();

    let (permissions, revoked_at, last_used_at): (i64, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT permissions, revoked_at, last_used_at FROM api_keys WHERE id = 'k1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();

    assert_eq!(permissions, 3, "default permissions should be 3");
    assert!(revoked_at.is_none(), "revoked_at should default to NULL");
    assert!(
        last_used_at.is_none(),
        "last_used_at should default to NULL"
    );
}

#[test]
fn test_api_key_created_at_defaults_to_now() {
    let conn = setup_test_db();
    v56_api_keys::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO api_keys (id, name, key_hash, key_prefix) VALUES ('k1', 'My Key', 'hash1', 'rph_')",
        [],
    )
    .unwrap();

    let created_at: String = conn
        .query_row(
            "SELECT created_at FROM api_keys WHERE id = 'k1'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert!(
        !created_at.is_empty(),
        "created_at should be populated by default"
    );
}

// ---------------------------------------------------------------------------
// Unique constraint on key_hash
// ---------------------------------------------------------------------------

#[test]
fn test_key_hash_unique_constraint() {
    let conn = setup_test_db();
    v56_api_keys::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO api_keys (id, name, key_hash, key_prefix) VALUES ('k1', 'Key 1', 'same_hash', 'rph_')",
        [],
    )
    .unwrap();

    let result = conn.execute(
        "INSERT INTO api_keys (id, name, key_hash, key_prefix) VALUES ('k2', 'Key 2', 'same_hash', 'rph_')",
        [],
    );

    assert!(
        result.is_err(),
        "inserting duplicate key_hash should fail due to UNIQUE constraint"
    );
}

// ---------------------------------------------------------------------------
// Cascade delete tests
// ---------------------------------------------------------------------------

#[test]
fn test_api_key_projects_cascade_on_api_key_delete() {
    let conn = setup_test_db();
    v56_api_keys::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO projects (id, name) VALUES ('p1', 'Project 1')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO api_keys (id, name, key_hash, key_prefix) VALUES ('k1', 'Key 1', 'hash1', 'rph_')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO api_key_projects (api_key_id, project_id) VALUES ('k1', 'p1')",
        [],
    )
    .unwrap();

    // Verify row exists
    let before: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM api_key_projects WHERE api_key_id = 'k1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(before, 1);

    // Delete the api_key — should cascade
    conn.execute("DELETE FROM api_keys WHERE id = 'k1'", [])
        .unwrap();

    let after: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM api_key_projects WHERE api_key_id = 'k1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        after, 0,
        "api_key_projects rows should be deleted when api_key is deleted (CASCADE)"
    );
}

#[test]
fn test_api_key_projects_cascade_on_project_delete() {
    let conn = setup_test_db();
    v56_api_keys::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO projects (id, name) VALUES ('p1', 'Project 1')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO api_keys (id, name, key_hash, key_prefix) VALUES ('k1', 'Key 1', 'hash1', 'rph_')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO api_key_projects (api_key_id, project_id) VALUES ('k1', 'p1')",
        [],
    )
    .unwrap();

    // Delete the project — should cascade
    conn.execute("DELETE FROM projects WHERE id = 'p1'", [])
        .unwrap();

    let after: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM api_key_projects WHERE project_id = 'p1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        after, 0,
        "api_key_projects rows should be deleted when project is deleted (CASCADE)"
    );
}

// ---------------------------------------------------------------------------
// external_events insert
// ---------------------------------------------------------------------------

#[test]
fn test_external_events_insert() {
    let conn = setup_test_db();
    v56_api_keys::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO external_events (event_type, project_id, payload) VALUES ('task.created', 'p1', '{\"id\":\"t1\"}')",
        [],
    )
    .unwrap();

    let count: i32 = conn
        .query_row("SELECT COUNT(*) FROM external_events", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1, "external_events row should be inserted");
}

#[test]
fn test_external_events_autoincrement_id() {
    let conn = setup_test_db();
    v56_api_keys::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO external_events (event_type, project_id, payload) VALUES ('ev1', 'p1', '{}')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO external_events (event_type, project_id, payload) VALUES ('ev2', 'p2', '{}')",
        [],
    )
    .unwrap();

    let ids: Vec<i64> = {
        let mut stmt = conn
            .prepare("SELECT id FROM external_events ORDER BY id")
            .unwrap();
        stmt.query_map([], |row| row.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };

    assert_eq!(ids, vec![1, 2], "external_events.id should autoincrement");
}

// ---------------------------------------------------------------------------
// api_audit_log insert
// ---------------------------------------------------------------------------

#[test]
fn test_api_audit_log_insert() {
    let conn = setup_test_db();
    v56_api_keys::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO api_keys (id, name, key_hash, key_prefix) VALUES ('k1', 'Key 1', 'hash1', 'rph_')",
        [],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO api_audit_log (api_key_id, tool_name) VALUES ('k1', 'list_tasks')",
        [],
    )
    .unwrap();

    let (success, project_id, latency_ms): (i64, Option<String>, Option<i64>) = conn
        .query_row(
            "SELECT success, project_id, latency_ms FROM api_audit_log WHERE api_key_id = 'k1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();

    assert_eq!(success, 1, "success should default to 1");
    assert!(project_id.is_none(), "project_id should default to NULL");
    assert!(latency_ms.is_none(), "latency_ms should default to NULL");
}

// ---------------------------------------------------------------------------
// api_key_projects primary key uniqueness
// ---------------------------------------------------------------------------

#[test]
fn test_api_key_projects_primary_key_uniqueness() {
    let conn = setup_test_db();
    v56_api_keys::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO projects (id, name) VALUES ('p1', 'Project 1')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO api_keys (id, name, key_hash, key_prefix) VALUES ('k1', 'Key 1', 'hash1', 'rph_')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO api_key_projects (api_key_id, project_id) VALUES ('k1', 'p1')",
        [],
    )
    .unwrap();

    let result = conn.execute(
        "INSERT INTO api_key_projects (api_key_id, project_id) VALUES ('k1', 'p1')",
        [],
    );

    assert!(
        result.is_err(),
        "duplicate (api_key_id, project_id) should violate primary key constraint"
    );
}

// ---------------------------------------------------------------------------
// Idempotency
// ---------------------------------------------------------------------------

#[test]
fn test_migration_idempotent() {
    let conn = setup_test_db();

    // Run twice — should not fail because of IF NOT EXISTS
    v56_api_keys::migrate(&conn).unwrap();
    v56_api_keys::migrate(&conn).unwrap();

    assert!(helpers::table_exists(&conn, "api_keys"));
    assert!(helpers::table_exists(&conn, "api_key_projects"));
    assert!(helpers::table_exists(&conn, "external_events"));
    assert!(helpers::table_exists(&conn, "api_audit_log"));
}
