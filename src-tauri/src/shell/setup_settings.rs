use std::sync::Arc;

use tracing::{info, warn};

use crate::application::harness_runtime_registry::{
    default_agent_harness_settings_config, default_execution_settings_config,
};
use crate::application::ideation_effort_bootstrap::seed_ideation_effort_defaults;
use crate::application::ideation_model_bootstrap::seed_ideation_model_settings;
use crate::application::{
    agent_capability_gate::AgentCapabilities, load_or_seed_agent_lane_settings_defaults,
    load_or_seed_execution_settings_defaults,
};
use crate::application::execution_state::ExecutionState;
use crate::domain::agents::AgentHarnessKind;
use crate::infrastructure::agents::claude::apply_claude_provider_permission_settings;
use crate::infrastructure::agents::set_agent_personas_override;
use crate::AppState;

pub(crate) async fn initialize_settings_defaults(
    app_state: &AppState,
    init_execution_state: Arc<ExecutionState>,
) {
    let feature_flag_overrides_repo = Arc::clone(&app_state.ui_feature_flag_overrides_repo);
    let agent_capability_gate = Arc::clone(&app_state.agent_capability_gate);
    match feature_flag_overrides_repo.get().await {
        Ok(overrides) => {
            set_agent_personas_override(overrides.agent_personas);
            agent_capability_gate.replace(AgentCapabilities {
                team: overrides.agent_conversation_team,
                workflows: overrides.agent_conversation_workflows,
                autopilot: overrides.agent_conversation_autopilot,
            });
        }
        Err(error) => {
            agent_capability_gate.replace(AgentCapabilities::default());
            warn!(%error, "Failed to load UI feature flag overrides; Agent capabilities remain disabled");
        }
    }

    // Load execution settings from database and apply to ExecutionState
    // This must happen before HTTP server starts to ensure consistent configuration
    let init_settings_repo = Arc::clone(&app_state.execution_settings_repo);
    let init_global_settings_repo = Arc::clone(&app_state.global_execution_settings_repo);
    let init_agent_lane_settings_repo = Arc::clone(&app_state.agent_lane_settings_repo);
    let init_agent_provider_settings_repo = Arc::clone(&app_state.agent_provider_settings_repo);
    let execution_defaults = default_execution_settings_config();
    let agent_harness_defaults = default_agent_harness_settings_config();
    match load_or_seed_execution_settings_defaults(
        init_settings_repo,
        init_global_settings_repo,
        &execution_defaults.project,
        &execution_defaults.global,
    )
    .await
    {
        Ok(result) => {
            init_execution_state.set_max_concurrent(result.project_defaults.max_concurrent_tasks);
            init_execution_state
                .set_global_max_concurrent(result.global_defaults.global_max_concurrent);
            init_execution_state
                .set_workspace_max_concurrent(result.global_defaults.workspace_max_concurrent);
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
                workspace_max_concurrent = result.global_defaults.workspace_max_concurrent,
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
            info!("Provider onboarding required because no enabled default is configured");
        }
        Err(e) => {
            warn!(
                "Provider onboarding state unavailable because settings could not be read: {}",
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

    // Seed ideation effort defaults (idempotent — only seeds when no global row exists)
    let init_effort_repo = Arc::clone(&app_state.ideation_effort_settings_repo);
    match seed_ideation_effort_defaults(init_effort_repo).await {
        Ok(result) => {
            if result.seeded_global {
                tracing::info!("Seeded global ideation effort defaults (inherit/inherit)");
            }
        }
        Err(e) => tracing::warn!("Failed to seed ideation effort defaults: {}", e),
    }

    // Seed ideation model defaults (idempotent — only seeds when no global row exists)
    let init_model_repo = Arc::clone(&app_state.ideation_model_settings_repo);
    match seed_ideation_model_settings(init_model_repo).await {
        Ok(_) => {
            tracing::debug!("Ideation model settings seeded (or already existed)");
        }
        Err(e) => tracing::warn!("Failed to seed ideation model settings: {}", e),
    }
}
