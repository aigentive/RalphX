//! Tests for migration v20260520150000: atlassian oauth settings

use rusqlite::Connection;

use super::{v20260520125526_atlassian_integrations, v20260520150000_atlassian_oauth};

#[test]
fn test_migration_adds_oauth_columns() {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");
    v20260520125526_atlassian_integrations::migrate(&conn).unwrap();
    v20260520150000_atlassian_oauth::migrate(&conn).unwrap();

    let row: (String, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT auth_method, oauth_client_id, oauth_cloud_id
               FROM atlassian_integration_settings
              WHERE id = 'default'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();

    assert_eq!(row, ("api_token".to_string(), None, None));
}
