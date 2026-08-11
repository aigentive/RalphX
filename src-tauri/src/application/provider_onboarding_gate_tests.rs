use std::{error::Error, io, sync::Arc};

use async_trait::async_trait;

use crate::domain::agents::{AgentHarnessKind, AgentProviderSettings};
use crate::domain::repositories::AgentProviderSettingsRepository;
use crate::infrastructure::memory::MemoryAgentProviderSettingsRepository;

use super::{ensure_provider_spawn_enabled, resolve_enabled_default_provider};

struct FailingAgentProviderSettingsRepository;

fn provider_repo_error() -> Box<dyn Error> {
    Box::new(io::Error::other("provider repo failed"))
}

#[async_trait]
impl AgentProviderSettingsRepository for FailingAgentProviderSettingsRepository {
    async fn get(
        &self,
        _provider: AgentHarnessKind,
    ) -> Result<Option<AgentProviderSettings>, Box<dyn Error>> {
        Err(provider_repo_error())
    }

    async fn list(&self) -> Result<Vec<AgentProviderSettings>, Box<dyn Error>> {
        Err(provider_repo_error())
    }

    async fn get_default(&self) -> Result<Option<AgentProviderSettings>, Box<dyn Error>> {
        Err(provider_repo_error())
    }

    async fn upsert(
        &self,
        _settings: &AgentProviderSettings,
    ) -> Result<AgentProviderSettings, Box<dyn Error>> {
        Err(provider_repo_error())
    }
}

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
async fn default_provider_gate_rejects_disabled_default_provider() {
    let repo = Arc::new(MemoryAgentProviderSettingsRepository::new());
    let mut claude = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    claude.is_default = true;
    repo.upsert(&claude).await.unwrap();

    let repo: Arc<dyn AgentProviderSettingsRepository> = repo;
    let error = resolve_enabled_default_provider(&repo, "send_agent_message")
        .await
        .expect_err("disabled default provider should block spawns");

    assert!(error.contains("Settings > Harness > Providers"));
}

#[tokio::test]
async fn default_provider_gate_rejects_enabled_provider_without_a_default() {
    let repo = Arc::new(MemoryAgentProviderSettingsRepository::new());
    let mut codex = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    codex.enabled = true;
    repo.upsert(&codex).await.unwrap();

    let repo: Arc<dyn AgentProviderSettingsRepository> = repo;
    let error = resolve_enabled_default_provider(&repo, "send_agent_message")
        .await
        .expect_err("provider without default selection should block spawns");

    assert!(error.contains("Settings > Harness > Providers"));
}

#[tokio::test]
async fn default_provider_gate_fails_closed_when_settings_cannot_be_read() {
    let repo: Arc<dyn AgentProviderSettingsRepository> =
        Arc::new(FailingAgentProviderSettingsRepository);

    let error = resolve_enabled_default_provider(&repo, "send_agent_message")
        .await
        .expect_err("provider read failure should block spawns");

    assert!(error.contains("Failed to read provider settings"));
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
