//! Tests for migration v20260801021420: delegation parks

use rusqlite::Connection;

use super::v20260801021420_delegation_parks;

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

#[test]
fn test_migration_runs() {
    let conn = setup_test_db();
    v20260801021420_delegation_parks::migrate(&conn).unwrap();

    for table in ["delegation_parks", "delegation_park_jobs"] {
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
                [table],
                |row| row.get(0),
            )
            .expect("query table existence");
        assert!(exists, "{table} should exist");
    }

    for index in [
        "idx_delegation_parks_state_deadline",
        "idx_delegation_parks_conversation_state",
        "idx_delegation_park_jobs_run",
    ] {
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'index' AND name = ?1)",
                [index],
                |row| row.get(0),
            )
            .expect("query index existence");
        assert!(exists, "{index} should exist");
    }
}
