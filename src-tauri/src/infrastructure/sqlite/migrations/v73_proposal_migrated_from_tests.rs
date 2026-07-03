//! Tests for migration v73: migrated_from traceability columns on task_proposals

use rusqlite::Connection;

use super::helpers;
use super::v73_proposal_migrated_from;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");

    conn.execute_batch(
        "CREATE TABLE task_proposals (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL
        );",
    )
    .expect("Failed to create test schema");

    conn
}

#[test]
fn test_migrated_from_session_id_column_added() {
    let conn = setup_test_db();

    assert!(
        !helpers::column_exists(&conn, "task_proposals", "migrated_from_session_id"),
        "migrated_from_session_id should not exist before migration"
    );

    v73_proposal_migrated_from::migrate(&conn).unwrap();

    assert!(
        helpers::column_exists(&conn, "task_proposals", "migrated_from_session_id"),
        "migrated_from_session_id should exist after migration"
    );
}

#[test]
fn test_migrated_from_proposal_id_column_added() {
    let conn = setup_test_db();

    assert!(
        !helpers::column_exists(&conn, "task_proposals", "migrated_from_proposal_id"),
        "migrated_from_proposal_id should not exist before migration"
    );

    v73_proposal_migrated_from::migrate(&conn).unwrap();

    assert!(
        helpers::column_exists(&conn, "task_proposals", "migrated_from_proposal_id"),
        "migrated_from_proposal_id should exist after migration"
    );
}

#[test]
fn test_existing_proposals_have_null_migrated_from_fields() {
    let conn = setup_test_db();

    conn.execute(
        "INSERT INTO task_proposals (id, title) VALUES ('p1', 'Pre-migration Proposal')",
        [],
    )
    .unwrap();

    v73_proposal_migrated_from::migrate(&conn).unwrap();

    let (session_val, proposal_val): (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT migrated_from_session_id, migrated_from_proposal_id FROM task_proposals WHERE id = 'p1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();

    assert!(
        session_val.is_none(),
        "existing proposals should have NULL migrated_from_session_id"
    );
    assert!(
        proposal_val.is_none(),
        "existing proposals should have NULL migrated_from_proposal_id"
    );
}

#[test]
fn test_new_proposal_can_set_migrated_from_fields() {
    let conn = setup_test_db();
    v73_proposal_migrated_from::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO task_proposals (id, title, migrated_from_session_id, migrated_from_proposal_id) \
         VALUES ('p2', 'Migrated Proposal', 'session-abc', 'proposal-xyz')",
        [],
    )
    .unwrap();

    let (session_val, proposal_val): (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT migrated_from_session_id, migrated_from_proposal_id FROM task_proposals WHERE id = 'p2'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();

    assert_eq!(session_val.as_deref(), Some("session-abc"));
    assert_eq!(proposal_val.as_deref(), Some("proposal-xyz"));
}

#[test]
fn test_migration_idempotent() {
    let conn = setup_test_db();

    v73_proposal_migrated_from::migrate(&conn).unwrap();
    v73_proposal_migrated_from::migrate(&conn).unwrap();

    assert!(helpers::column_exists(
        &conn,
        "task_proposals",
        "migrated_from_session_id"
    ));
    assert!(helpers::column_exists(
        &conn,
        "task_proposals",
        "migrated_from_proposal_id"
    ));
}
