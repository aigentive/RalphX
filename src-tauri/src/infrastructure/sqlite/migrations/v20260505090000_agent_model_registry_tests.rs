use super::helpers::{index_exists, table_exists};
use super::v20260505090000_agent_model_registry;
use crate::infrastructure::sqlite::connection::open_memory_connection;

#[test]
fn test_agent_model_registry_table_exists() {
    let conn = open_memory_connection().unwrap();

    v20260505090000_agent_model_registry::migrate(&conn).unwrap();

    assert!(table_exists(&conn, "agent_model_registry"));
    assert!(index_exists(&conn, "idx_agent_model_registry_provider"));
    assert!(index_exists(&conn, "idx_agent_model_registry_enabled"));
}

#[test]
fn test_agent_model_registry_enforces_provider_model_uniqueness() {
    let conn = open_memory_connection().unwrap();

    v20260505090000_agent_model_registry::migrate(&conn).unwrap();

    conn.execute(
        "INSERT INTO agent_model_registry (
            provider, model_id, label, menu_label, default_effort, supported_efforts
        ) VALUES ('codex', 'gpt-5.6', 'GPT-5.6', 'GPT-5.6', 'xhigh', '[\"low\",\"medium\",\"high\",\"xhigh\"]')",
        [],
    )
    .unwrap();

    let duplicate = conn.execute(
        "INSERT INTO agent_model_registry (
            provider, model_id, label, menu_label, default_effort, supported_efforts
        ) VALUES ('codex', 'gpt-5.6', 'GPT-5.6 other', 'GPT-5.6 other', 'high', '[\"high\"]')",
        [],
    );

    assert!(duplicate.is_err());
}
