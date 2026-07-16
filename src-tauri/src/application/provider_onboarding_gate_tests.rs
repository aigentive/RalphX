use std::sync::Arc;

use crate::domain::agents::{AgentHarnessKind, AgentProviderSettings};
use crate::domain::repositories::AgentProviderSettingsRepository;
use crate::infrastructure::memory::MemoryAgentProviderSettingsRepository;

use super::{
    ensure_provider_spawn_enabled, resolve_enabled_default_provider,
    resolve_enabled_provider_or_default,
};

#[tokio::test]
async fn default_provider_gate_requires_enabled_default_provider() {
    let repo: Arc<dyn AgentProviderSettingsRepository> =
        Arc::new(MemoryAgentProviderSettingsRepository::new());

    let error = resolve_enabled_default_provider(&repo, "send_agent_message")
        .await
        .expect_err("missing default provider should block spawns");

    assert!(error.contains("Settings > Harness > Providers"));
}

#[tokio::test]
async fn provider_gate_rejects_disabled_requested_provider() {
    let repo = Arc::new(MemoryAgentProviderSettingsRepository::new());
    let mut codex = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    codex.enabled = true;
    codex.is_default = true;
    repo.upsert(&codex).await.unwrap();

    let repo: Arc<dyn AgentProviderSettingsRepository> = repo;
    let error = ensure_provider_spawn_enabled(&repo, AgentHarnessKind::Claude, "PR describer")
        .await
        .expect_err("disabled requested provider should block spawns");

    assert!(error.contains("claude is not enabled"));
}

#[tokio::test]
async fn provider_gate_accepts_enabled_default_and_requested_provider() {
    let repo = Arc::new(MemoryAgentProviderSettingsRepository::new());
    let mut codex = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    codex.enabled = true;
    codex.is_default = true;
    repo.upsert(&codex).await.unwrap();

    let repo: Arc<dyn AgentProviderSettingsRepository> = repo;
    ensure_provider_spawn_enabled(&repo, AgentHarnessKind::Codex, "send_agent_message")
        .await
        .expect("enabled default provider should allow spawn");
}

#[tokio::test]
async fn provider_preference_falls_back_to_enabled_default_when_requested_provider_is_disabled() {
    let repo = Arc::new(MemoryAgentProviderSettingsRepository::new());
    let mut codex = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    codex.enabled = true;
    codex.is_default = true;
    repo.upsert(&codex).await.unwrap();
    repo.upsert(&AgentProviderSettings::disabled_defaults(
        AgentHarnessKind::Claude,
    ))
    .await
    .unwrap();

    let repo: Arc<dyn AgentProviderSettingsRepository> = repo;
    let selected = resolve_enabled_provider_or_default(
        &repo,
        Some(AgentHarnessKind::Claude),
        "workspace reviewer provider",
    )
    .await
    .expect("disabled preference should fall back to enabled default");

    assert_eq!(selected.provider, AgentHarnessKind::Codex);
}

#[tokio::test]
async fn provider_preference_keeps_enabled_non_default_provider() {
    let repo = Arc::new(
        MemoryAgentProviderSettingsRepository::with_all_providers_enabled(AgentHarnessKind::Codex),
    );
    let repo: Arc<dyn AgentProviderSettingsRepository> = repo;

    let selected = resolve_enabled_provider_or_default(
        &repo,
        Some(AgentHarnessKind::Claude),
        "workspace reviewer provider",
    )
    .await
    .expect("enabled preference should keep provider continuity");

    assert_eq!(selected.provider, AgentHarnessKind::Claude);
}
