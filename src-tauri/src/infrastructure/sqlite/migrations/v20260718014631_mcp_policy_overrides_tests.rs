//! Tests for migration v20260718014631: mcp policy overrides

use rusqlite::Connection;

use super::v20260718014631_mcp_policy_overrides;

fn setup_test_db() -> Connection {
    Connection::open_in_memory().expect("Failed to create in-memory database")
}

#[test]
fn migration_creates_scoped_policy_table_with_required_server_guard() {
    let conn = setup_test_db();
    v20260718014631_mcp_policy_overrides::migrate(&conn).unwrap();
    conn.execute(
        "INSERT INTO mcp_policy_overrides
         (scope_type, scope_id, provider, server_id, server_state)
         VALUES ('global', '', 'claude', 'github', 'disabled')",
        [],
    )
    .unwrap();
    assert!(conn
        .execute(
            "INSERT INTO mcp_policy_overrides
             (scope_type, scope_id, provider, server_id, server_state)
             VALUES ('global', '', 'claude', 'ralphx', 'disabled')",
            [],
        )
        .is_err());
}
