use crate::domain::execution::{ExecutionSettings, GlobalExecutionSettings};
use crate::domain::repositories::{
    ExecutionSettingsRepository, GlobalExecutionSettingsRepository,
};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionSettingsBootstrapResult {
    pub project_defaults: ExecutionSettings,
    pub global_defaults: GlobalExecutionSettings,
    pub seeded_project_defaults: bool,
    pub seeded_global_defaults: bool,
}

pub async fn load_or_seed_execution_settings_defaults(
    execution_settings_repo: Arc<dyn ExecutionSettingsRepository>,
    global_execution_settings_repo: Arc<dyn GlobalExecutionSettingsRepository>,
    desired_project_defaults: &ExecutionSettings,
    desired_global_defaults: &GlobalExecutionSettings,
) -> Result<ExecutionSettingsBootstrapResult, String> {
    let built_in_project_defaults = ExecutionSettings::default();
    let built_in_global_defaults = GlobalExecutionSettings::default();

    let mut project_defaults = execution_settings_repo
        .get_settings(None)
        .await
        .map_err(|e| e.to_string())?;
    let mut global_defaults = global_execution_settings_repo
        .get_settings()
        .await
        .map_err(|e| e.to_string())?;

    let mut seeded_project_defaults = false;
    let mut seeded_global_defaults = false;

    if project_defaults == built_in_project_defaults
        && project_defaults != *desired_project_defaults
    {
        project_defaults = execution_settings_repo
            .update_settings(None, desired_project_defaults)
            .await
            .map_err(|e| e.to_string())?;
        seeded_project_defaults = true;
    }

    if global_defaults == built_in_global_defaults && global_defaults != *desired_global_defaults {
        global_defaults = global_execution_settings_repo
            .update_settings(desired_global_defaults)
            .await
            .map_err(|e| e.to_string())?;
        seeded_global_defaults = true;
    }

    Ok(ExecutionSettingsBootstrapResult {
        project_defaults,
        global_defaults,
        seeded_project_defaults,
        seeded_global_defaults,
    })
}

#[cfg(test)]
#[path = "execution_settings_bootstrap_tests.rs"]
mod tests;
