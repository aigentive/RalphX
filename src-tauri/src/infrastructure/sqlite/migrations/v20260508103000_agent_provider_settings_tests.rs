use super::helpers::{index_exists, table_exists};
use super::v20260508103000_agent_provider_settings;
use rusqlite::Connection;

#[test]
fn creates_agent_provider_settings_table_and_default_index() {
    let conn = Connection::open_in_memory().unwrap();

    v20260508103000_agent_provider_settings::migrate(&conn).unwrap();

    assert!(table_exists(&conn, "agent_provider_settings"));
    assert!(index_exists(&conn, "idx_agent_provider_settings_default"));
}

#[test]
fn permits_only_one_default_provider() {
    let conn = Connection::open_in_memory().unwrap();
    v20260508103000_agent_provider_settings::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO agent_provider_settings (provider, enabled, is_default)
         VALUES ('codex', 1, 1)",
        [],
    )
    .unwrap();
    let duplicate = conn.execute(
        "INSERT INTO agent_provider_settings (provider, enabled, is_default)
         VALUES ('claude', 1, 1)",
        [],
    );

    assert!(duplicate.is_err());
}
