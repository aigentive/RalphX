use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::domain::agents::{AgentHarnessKind, AgentProviderSettings, STANDARD_AGENT_HARNESSES};
use crate::domain::repositories::AgentProviderSettingsRepository;
use crate::error::{AppError, AppResult};

use super::harness_runtime_registry::{refresh_harness_runtime_probe, HarnessRuntimeProbe};

#[derive(Debug, Clone)]
pub struct ProviderManagementEligibility {
    pub providers: Vec<AgentHarnessKind>,
    pub default_provider: Option<AgentHarnessKind>,
    pub probed_at: DateTime<Utc>,
}

impl ProviderManagementEligibility {
    pub fn ensure_ready(&self, provider: AgentHarnessKind) -> AppResult<()> {
        if self.providers.contains(&provider) {
            Ok(())
        } else {
            Err(AppError::Validation(format!(
                "provider_not_ready: {provider} must be enabled and runtime-available in Settings > Harness > Providers"
            )))
        }
    }
}

pub async fn resolve_provider_management_eligibility(
    repo: &Arc<dyn AgentProviderSettingsRepository>,
) -> AppResult<ProviderManagementEligibility> {
    let settings = repo.list().await.map_err(|error| {
        AppError::Infrastructure(format!("Failed to read provider settings: {error}"))
    })?;
    let probes = settings
        .iter()
        .filter(|row| row.enabled)
        .map(|row| {
            let probe = crate::application::managed_provider_cli::provider_runtime_probe(row)
                .unwrap_or_else(|| refresh_harness_runtime_probe(row.provider));
            (row.provider, probe)
        })
        .collect::<HashMap<_, _>>();
    Ok(resolve_provider_management_eligibility_with_probes(
        &settings,
        &probes,
        Utc::now(),
    ))
}

pub(crate) fn resolve_provider_management_eligibility_with_probes(
    settings: &[AgentProviderSettings],
    probes: &HashMap<AgentHarnessKind, HarnessRuntimeProbe>,
    probed_at: DateTime<Utc>,
) -> ProviderManagementEligibility {
    let providers = STANDARD_AGENT_HARNESSES
        .into_iter()
        .filter(|provider| {
            settings
                .iter()
                .find(|row| row.provider == *provider)
                .is_some_and(|row| row.enabled)
                && probes.get(provider).is_some_and(|probe| probe.available)
        })
        .collect::<Vec<_>>();
    let default_provider = settings
        .iter()
        .find(|row| row.enabled && row.is_default && providers.contains(&row.provider))
        .map(|row| row.provider);
    debug_assert!(providers
        .iter()
        .all(|provider| STANDARD_AGENT_HARNESSES.contains(provider)));
    ProviderManagementEligibility {
        providers,
        default_provider,
        probed_at,
    }
}
