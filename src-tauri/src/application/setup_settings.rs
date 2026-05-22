use std::sync::Arc;

use tracing::{info, warn};

use crate::AppState;
use crate::application::harness_runtime_registry::{
    default_agent_harness_settings_config, default_execution_settings_config,
};
use crate::application::ideation_effort_bootstrap::seed_ideation_effort_defaults;
use crate::application::ideation_model_bootstrap::seed_ideation_model_settings;
use crate::application::{
    load_or_seed_agent_lane_settings_defaults, load_or_seed_execution_settings_defaults,
};
use crate::commands::ExecutionState;
use crate::domain::agents::AgentHarnessKind;
use crate::infrastructure::agents::claude::apply_claude_provider_permission_settings;

pub(crate) fn initialize_settings_defaults(
    app_state: &AppState,
    init_execution_state: Arc<ExecutionState>,
) {
    // Load execution settings from database and apply to ExecutionState
    // This must happen before HTTP server starts to ensure consistent configuration
    let init_settings_repo = Arc::clone(&app_state.execution_settings_repo);
    let init_global_settings_repo = Arc::clone(&app_state.global_execution_settings_repo);
    let init_agent_lane_settings_repo = Arc::clone(&app_state.agent_lane_settings_repo);
    let init_agent_provider_settings_repo = Arc::clone(&app_state.agent_provider_settings_repo);
    let execution_defaults = default_execution_settings_config();
    let agent_harness_defaults = default_agent_harness_settings_config();
    tauri::async_runtime::block_on(async move {
        match load_or_seed_execution_settings_defaults(
            init_settings_repo,
            init_global_settings_repo,
            &execution_defaults.project,
            &execution_defaults.global,
        )
        .await
        {
            Ok(result) => {
                init_execution_state
                    .set_max_concurrent(result.project_defaults.max_concurrent_tasks);
                init_execution_state
                    .set_global_max_concurrent(result.global_defaults.global_max_concurrent);
                init_execution_state
                    .set_global_ideation_max(result.global_defaults.global_ideation_max);
                init_execution_state.set_allow_ideation_borrow_idle_execution(
                    result.global_defaults.allow_ideation_borrow_idle_execution,
                );
                info!(
                    seeded_project_defaults = result.seeded_project_defaults,
                    seeded_global_defaults = result.seeded_global_defaults,
                    max_concurrent = result.project_defaults.max_concurrent_tasks,
                    project_ideation_max = result.project_defaults.project_ideation_max,
                    global_max_concurrent = result.global_defaults.global_max_concurrent,
                    global_ideation_max = result.global_defaults.global_ideation_max,
                    allow_ideation_borrow_idle_execution =
                        result.global_defaults.allow_ideation_borrow_idle_execution,
                    "Initialized execution settings from DB/YAML defaults"
                );
            }
            Err(e) => {
                warn!(
                    "Failed to load/seed execution settings from database, using defaults: {}",
                    e
                );
            }
        }

        match load_or_seed_agent_lane_settings_defaults(
            init_agent_lane_settings_repo,
            &agent_harness_defaults,
        )
        .await
        {
            Ok(result) => {
                info!(
                    seeded_global_lane_count = result.seeded_global_lanes.len(),
                    seeded_global_lanes = ?result
                        .seeded_global_lanes
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>(),
                    configured_global_lane_count = result.global_defaults.len(),
                    "Initialized agent harness defaults from DB/YAML defaults"
                );
            }
            Err(e) => {
                warn!(
                    "Failed to load/seed agent harness defaults from database, using runtime fallbacks: {}",
                    e
                );
            }
        }

        match init_agent_provider_settings_repo.get_default().await {
            Ok(Some(settings)) if settings.enabled => {}
            Ok(_) => {
                init_execution_state.pause();
                info!("Paused execution because no enabled default provider is configured");
            }
            Err(e) => {
                init_execution_state.pause();
                warn!(
                    "Paused execution because provider settings could not be read: {}",
                    e
                );
            }
        }

        match init_agent_provider_settings_repo
            .get(AgentHarnessKind::Claude)
            .await
        {
            Ok(Some(settings)) => {
                apply_claude_provider_permission_settings(&settings);
                info!("Initialized Claude permission defaults from provider settings");
            }
            Ok(None) => {}
            Err(e) => {
                warn!(
                    "Failed to load Claude provider settings for permission defaults: {}",
                    e
                );
            }
        }
    });

    // Seed ideation effort defaults (idempotent — only seeds when no global row exists)
    let init_effort_repo = Arc::clone(&app_state.ideation_effort_settings_repo);
    tauri::async_runtime::block_on(async move {
        match seed_ideation_effort_defaults(init_effort_repo).await {
            Ok(result) => {
                if result.seeded_global {
                    tracing::info!("Seeded global ideation effort defaults (inherit/inherit)");
                }
            }
            Err(e) => tracing::warn!("Failed to seed ideation effort defaults: {}", e),
        }
    });

    // Seed ideation model defaults (idempotent — only seeds when no global row exists)
    let init_model_repo = Arc::clone(&app_state.ideation_model_settings_repo);
    tauri::async_runtime::block_on(async move {
        match seed_ideation_model_settings(init_model_repo).await {
            Ok(_) => {
                tracing::debug!("Ideation model settings seeded (or already existed)");
            }
            Err(e) => tracing::warn!("Failed to seed ideation model settings: {}", e),
        }
    });
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::application::AppState;
    use crate::domain::agents::DEFAULT_AGENT_HARNESS;
    use crate::infrastructure::memory::MemoryAgentProviderSettingsRepository;

    #[test]
    fn initialize_settings_defaults_pauses_execution_without_default_provider() {
        let mut app_state = AppState::new_test();
        app_state.agent_provider_settings_repo =
            Arc::new(MemoryAgentProviderSettingsRepository::new());
        let execution_state = Arc::new(ExecutionState::new());

        initialize_settings_defaults(&app_state, Arc::clone(&execution_state));

        assert!(execution_state.is_paused());
    }

    #[test]
    fn initialize_settings_defaults_keeps_execution_running_with_default_provider() {
        let mut app_state = AppState::new_test();
        app_state.agent_provider_settings_repo =
            Arc::new(MemoryAgentProviderSettingsRepository::with_all_providers_enabled(
                DEFAULT_AGENT_HARNESS,
            ));
        let execution_state = Arc::new(ExecutionState::new());

        initialize_settings_defaults(&app_state, Arc::clone(&execution_state));

        assert!(!execution_state.is_paused());
    }
}
