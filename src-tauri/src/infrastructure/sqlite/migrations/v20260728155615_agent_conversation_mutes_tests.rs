//! Tests for migration v20260728155615: agent conversation mutes

use rusqlite::Connection;

use super::helpers;
use super::v20260728155615_agent_conversation_mutes;

#[test]
fn migration_creates_agent_conversation_mutes_with_expected_columns() {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");

    v20260728155615_agent_conversation_mutes::migrate(&conn).unwrap();

    assert!(helpers::table_exists(&conn, "agent_conversation_mutes"));
    for column in ["conversation_id", "muted_at", "state_fingerprint"] {
        assert!(
            helpers::column_exists(&conn, "agent_conversation_mutes", column),
            "missing {column}"
        );
    }
}
