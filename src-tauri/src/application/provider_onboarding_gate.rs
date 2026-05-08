use std::sync::Arc;

use crate::domain::agents::{AgentHarnessKind, AgentProviderSettings};
use crate::domain::repositories::AgentProviderSettingsRepository;

const PROVIDER_SETUP_REQUIRED_MESSAGE: &str =
    "Choose and enable a default provider in Settings > Harness > Providers before running agents.";

pub(crate) fn provider_setup_required_message(surface_name: &str) -> String {
    format!("{surface_name}: {PROVIDER_SETUP_REQUIRED_MESSAGE}")
}

pub(crate) async fn resolve_enabled_default_provider(
    repo: &Arc<dyn AgentProviderSettingsRepository>,
    surface_name: &str,
) -> Result<AgentProviderSettings, String> {
    let default = repo
        .get_default()
        .await
        .map_err(|error| format!("Failed to read provider settings: {error}"))?;
    match default {
        Some(settings) if settings.enabled => Ok(settings),
        _ => Err(provider_setup_required_message(surface_name)),
    }
}

pub(crate) async fn ensure_provider_spawn_enabled(
    repo: &Arc<dyn AgentProviderSettingsRepository>,
    harness: AgentHarnessKind,
    surface_name: &str,
) -> Result<(), String> {
    let _default = resolve_enabled_default_provider(repo, surface_name).await?;
    let provider = repo
        .get(harness)
        .await
        .map_err(|error| format!("Failed to read provider settings: {error}"))?;
    match provider {
        Some(settings) if settings.enabled => Ok(()),
        _ => Err(format!(
            "{surface_name}: {harness} is not enabled in Settings > Harness > Providers."
        )),
    }
}

#[cfg(test)]
#[path = "provider_onboarding_gate_tests.rs"]
mod tests;
