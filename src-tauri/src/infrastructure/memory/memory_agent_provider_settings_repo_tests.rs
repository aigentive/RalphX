use crate::domain::agents::{AgentHarnessKind, AgentProviderSettings};
use crate::domain::repositories::AgentProviderSettingsRepository;

use super::MemoryAgentProviderSettingsRepository;

#[tokio::test]
async fn upsert_tracks_one_default_provider() {
    let repo = MemoryAgentProviderSettingsRepository::new();
    let mut claude = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    claude.enabled = true;
    claude.is_default = true;
    repo.upsert(&claude).await.expect("upsert claude");

    let mut codex = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    codex.enabled = true;
    codex.is_default = true;
    repo.upsert(&codex).await.expect("upsert codex");

    let default = repo
        .get_default()
        .await
        .expect("get default")
        .expect("default provider");
    assert_eq!(default.provider, AgentHarnessKind::Codex);
    assert!(
        !repo
            .get(AgentHarnessKind::Claude)
            .await
            .expect("get claude")
            .expect("claude row")
            .is_default
    );
}

#[tokio::test]
async fn test_constructor_can_seed_enabled_test_providers() {
    let repo =
        MemoryAgentProviderSettingsRepository::with_all_providers_enabled(AgentHarnessKind::Codex);

    let providers = repo.list().await.unwrap();
    assert!(providers.iter().all(|provider| provider.enabled));
    assert_eq!(
        repo.get_default().await.unwrap().map(|row| row.provider),
        Some(AgentHarnessKind::Codex)
    );
}
