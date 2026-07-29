use rusqlite::Connection;

use crate::domain::agents::{
    AgentHarnessKind, AgentModelDefinition, AgentModelSource, LogicalEffort,
};
use crate::domain::repositories::AgentModelRegistryRepository;
use crate::infrastructure::sqlite::SqliteAgentModelRegistryRepository;
use crate::testing::SqliteTestDb;

fn setup_repo() -> (SqliteTestDb, SqliteAgentModelRegistryRepository) {
    let db = SqliteTestDb::new("sqlite_agent_model_registry_repo_tests");
    let repo = SqliteAgentModelRegistryRepository::from_shared(db.shared_conn());
    (db, repo)
}

#[tokio::test]
async fn upsert_and_list_custom_model() {
    let (_db, repo) = setup_repo();
    let model = AgentModelDefinition::custom(
        AgentHarnessKind::Codex,
        "gpt-5.6",
        "GPT-5.6",
        "GPT-5.6",
        Some("Future Codex model".to_string()),
        vec![
            LogicalEffort::Low,
            LogicalEffort::Medium,
            LogicalEffort::High,
            LogicalEffort::XHigh,
        ],
        LogicalEffort::XHigh,
        true,
    );

    let saved = repo.upsert_custom_model(&model).await.unwrap();
    let rows = repo.list_custom_models().await.unwrap();

    assert_eq!(saved.source, AgentModelSource::Custom);
    assert_eq!(saved.model_id, "gpt-5.6");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].default_effort, LogicalEffort::XHigh);
    assert_eq!(rows[0].supported_efforts.len(), 4);
    assert!(rows[0].created_at.is_some());
    assert!(rows[0].updated_at.is_some());
}

#[tokio::test]
async fn upsert_normalizes_default_effort_to_supported_efforts() {
    let (_db, repo) = setup_repo();
    let model = AgentModelDefinition::custom(
        AgentHarnessKind::Codex,
        "gpt-5.6-mini",
        "",
        "",
        None,
        vec![LogicalEffort::Low, LogicalEffort::Medium],
        LogicalEffort::XHigh,
        true,
    );

    let saved = repo.upsert_custom_model(&model).await.unwrap();

    assert_eq!(saved.label, "gpt-5.6-mini");
    assert_eq!(saved.menu_label, "gpt-5.6-mini");
    assert_eq!(saved.default_effort, LogicalEffort::Low);
}

#[tokio::test]
async fn delete_custom_model_removes_row() {
    let (_db, repo) = setup_repo();
    let model = AgentModelDefinition::custom(
        AgentHarnessKind::Claude,
        "claude-opus-4-7",
        "Claude Opus 4.7",
        "Claude Opus 4.7",
        None,
        vec![
            LogicalEffort::High,
            LogicalEffort::XHigh,
            LogicalEffort::Max,
        ],
        LogicalEffort::XHigh,
        true,
    );
    repo.upsert_custom_model(&model).await.unwrap();

    assert!(repo
        .delete_custom_model(AgentHarnessKind::Claude, "claude-opus-4-7")
        .await
        .unwrap());
    assert!(repo.list_custom_models().await.unwrap().is_empty());
}

#[tokio::test]
async fn upsert_updates_existing_row_and_preserves_created_timestamp() {
    let (_db, repo) = setup_repo();
    let initial = AgentModelDefinition::custom(
        AgentHarnessKind::Codex,
        "gpt-5.6",
        "GPT-5.6",
        "GPT-5.6",
        Some("Initial description".to_string()),
        vec![LogicalEffort::Low, LogicalEffort::Medium],
        LogicalEffort::Medium,
        true,
    );
    let saved = repo.upsert_custom_model(&initial).await.unwrap();

    let updated = AgentModelDefinition::custom(
        AgentHarnessKind::Codex,
        "gpt-5.6",
        "GPT-5.6 Preview",
        "GPT-5.6 Preview",
        Some("Updated description".to_string()),
        vec![LogicalEffort::High, LogicalEffort::XHigh],
        LogicalEffort::XHigh,
        false,
    );
    let saved_again = repo.upsert_custom_model(&updated).await.unwrap();

    assert_eq!(saved_again.model_id, "gpt-5.6");
    assert_eq!(saved_again.label, "GPT-5.6 Preview");
    assert_eq!(saved_again.description.as_deref(), Some("Updated description"));
    assert_eq!(
        saved_again.supported_efforts,
        vec![LogicalEffort::High, LogicalEffort::XHigh]
    );
    assert_eq!(saved_again.default_effort, LogicalEffort::XHigh);
    assert!(!saved_again.enabled);
    assert_eq!(saved_again.created_at, saved.created_at);
    assert!(saved_again.updated_at.is_some());

    let rows = repo.list_custom_models().await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].label, "GPT-5.6 Preview");
}

#[tokio::test]
async fn new_constructor_supports_existing_connections() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE agent_model_registry (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            provider TEXT NOT NULL,
            model_id TEXT NOT NULL,
            label TEXT NOT NULL,
            menu_label TEXT NOT NULL,
            description TEXT,
            default_effort TEXT NOT NULL,
            supported_efforts TEXT NOT NULL,
            source TEXT NOT NULL DEFAULT 'custom',
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(provider, model_id)
        );",
    )
    .unwrap();
    let repo = SqliteAgentModelRegistryRepository::new(conn);

    let model = AgentModelDefinition::custom(
        AgentHarnessKind::Claude,
        "claude-haiku-5",
        "Claude Haiku 5",
        "Claude Haiku 5",
        None,
        vec![LogicalEffort::Low],
        LogicalEffort::Low,
        true,
    );

    let saved = repo.upsert_custom_model(&model).await.unwrap();

    assert_eq!(saved.provider, AgentHarnessKind::Claude);
    assert_eq!(saved.source, AgentModelSource::Custom);
    assert!(!repo
        .delete_custom_model(AgentHarnessKind::Claude, "missing-model")
        .await
        .unwrap());
}

#[tokio::test]
async fn list_reports_invalid_persisted_provider() {
    let db = SqliteTestDb::new("sqlite_agent_model_registry_invalid_provider");
    {
        let conn = db.shared_conn();
        let conn = conn.lock().await;
        conn.execute(
            "INSERT INTO agent_model_registry (
                provider, model_id, label, menu_label, description, default_effort,
                supported_efforts, source, enabled
            ) VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, ?7, 1)",
            rusqlite::params![
                "openai",
                "gpt-invalid",
                "Invalid",
                "Invalid",
                "medium",
                "[\"medium\"]",
                "custom",
            ],
        )
        .unwrap();
    }
    let repo = SqliteAgentModelRegistryRepository::from_shared(db.shared_conn());

    let err = repo.list_custom_models().await.unwrap_err();

    assert!(err.to_string().contains("Invalid agent harness"));
}
