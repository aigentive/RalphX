use std::sync::Arc;

use crate::domain::agents::{AgentHarnessKind, AgentProviderSettings};
use crate::domain::repositories::AgentProviderSettingsRepository;
use crate::infrastructure::memory::MemoryAgentProviderSettingsRepository;

use super::{ensure_provider_spawn_enabled, resolve_enabled_default_provider};

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
    let error = ensure_provider_spawn_enabled(
        &repo,
        AgentHarnessKind::Claude,
        "PR describer",
    )
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
