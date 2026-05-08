use crate::domain::agents::{AgentHarnessKind, AgentProviderSettings};
use crate::domain::repositories::AgentProviderSettingsRepository;
use crate::testing::SqliteTestDb;

use super::SqliteAgentProviderSettingsRepository;

fn setup_repo() -> (SqliteTestDb, SqliteAgentProviderSettingsRepository) {
    let db = SqliteTestDb::new("sqlite-agent-provider-settings-repo");
    let repo = SqliteAgentProviderSettingsRepository::from_shared(db.shared_conn());
    (db, repo)
}

#[tokio::test]
async fn upsert_and_get_provider_settings() {
    let (_db, repo) = setup_repo();
    let mut settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    settings.enabled = true;
    settings.is_default = true;

    repo.upsert(&settings).await.unwrap();
    let row = repo
        .get(AgentHarnessKind::Codex)
        .await
        .unwrap()
        .expect("codex settings");

    assert!(row.enabled);
    assert!(row.is_default);
    assert_eq!(row.model.as_deref(), Some("gpt-5.5"));
    assert_eq!(row.sandbox_mode.as_deref(), Some("danger-full-access"));
}

#[tokio::test]
async fn upsert_clears_prior_default_provider() {
    let (_db, repo) = setup_repo();
    let mut claude = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    claude.enabled = true;
    claude.is_default = true;
    repo.upsert(&claude).await.unwrap();

    let mut codex = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    codex.enabled = true;
    codex.is_default = true;
    repo.upsert(&codex).await.unwrap();

    let default = repo.get_default().await.unwrap().expect("default provider");
    let claude = repo.get(AgentHarnessKind::Claude).await.unwrap().unwrap();

    assert_eq!(default.provider, AgentHarnessKind::Codex);
    assert!(!claude.is_default);
}
