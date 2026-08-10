use super::*;

// ExecutionState/ActiveProjectState now live in the application layer; these
// re-exports keep the existing `crate::commands::...` command-layer paths working.
pub use crate::application::execution_state::{
    ActiveProjectState, ExecutionState, AGENT_ACTIVE_STATUSES, AUTO_TRANSITION_STATES,
};

pub use crate::domain::execution::{ExecutionCommandResponse, ExecutionStatusResponse};

/// Response for execution settings queries
#[derive(Debug, Serialize)]
pub struct ExecutionSettingsResponse {
    /// Maximum number of concurrent tasks
    pub max_concurrent_tasks: u32,
    /// Maximum number of concurrent ideation sessions for this project
    pub project_ideation_max: u32,
    /// Whether to auto-commit changes after successful task completion
    pub auto_commit: bool,
    /// Whether to pause execution when a task fails
    pub pause_on_failure: bool,
    /// Default Autofix CI & Reviews setting for new agent workspaces
    pub agent_workspace_pr_autofix_default: bool,
    /// Default GitHub auto-merge setting for new agent workspaces
    pub agent_workspace_pr_auto_merge_default: bool,
}

impl From<ExecutionSettings> for ExecutionSettingsResponse {
    fn from(settings: ExecutionSettings) -> Self {
        Self {
            max_concurrent_tasks: settings.max_concurrent_tasks,
            project_ideation_max: settings.project_ideation_max,
            auto_commit: settings.auto_commit,
            pause_on_failure: settings.pause_on_failure,
            agent_workspace_pr_autofix_default: settings.agent_workspace_pr_autofix_default,
            agent_workspace_pr_auto_merge_default: settings.agent_workspace_pr_auto_merge_default,
        }
    }
}

/// Input for updating execution settings
#[derive(Debug, Deserialize)]
pub struct UpdateExecutionSettingsInput {
    /// Maximum number of concurrent tasks
    pub max_concurrent_tasks: u32,
    /// Maximum number of concurrent ideation sessions for this project
    pub project_ideation_max: u32,
    /// Whether to auto-commit changes after successful task completion
    pub auto_commit: bool,
    /// Whether to pause execution when a task fails
    pub pause_on_failure: bool,
    /// Default Autofix CI & Reviews setting for new agent workspaces
    pub agent_workspace_pr_autofix_default: bool,
    /// Default GitHub auto-merge setting for new agent workspaces
    pub agent_workspace_pr_auto_merge_default: bool,
}

/// Response for global execution settings queries
/// Phase 82: Global concurrency cap across all projects
#[derive(Debug, Serialize)]
pub struct GlobalExecutionSettingsResponse {
    /// Maximum total concurrent tasks across ALL projects
    pub global_max_concurrent: u32,
    /// Maximum concurrent workspace main agents across all projects
    pub workspace_max_concurrent: u32,
    /// Maximum total concurrent ideation sessions across all projects
    pub global_ideation_max: u32,
    /// Whether ideation may borrow idle execution capacity
    pub allow_ideation_borrow_idle_execution: bool,
}

impl From<crate::domain::execution::GlobalExecutionSettings> for GlobalExecutionSettingsResponse {
    fn from(settings: crate::domain::execution::GlobalExecutionSettings) -> Self {
        Self {
            global_max_concurrent: settings.global_max_concurrent,
            workspace_max_concurrent: settings.workspace_max_concurrent,
            global_ideation_max: settings.global_ideation_max,
            allow_ideation_borrow_idle_execution: settings.allow_ideation_borrow_idle_execution,
        }
    }
}

/// Input for updating global execution settings
#[derive(Debug, Deserialize)]
pub struct UpdateGlobalExecutionSettingsInput {
    /// Maximum total concurrent tasks across ALL projects (max: 50)
    pub global_max_concurrent: u32,
    /// Maximum concurrent workspace main agents across ALL projects (max: 50)
    #[serde(default = "default_workspace_update_max_concurrent")]
    pub workspace_max_concurrent: u32,
    /// Maximum total concurrent ideation sessions across ALL projects (max: 50)
    pub global_ideation_max: u32,
    /// Whether ideation may borrow idle execution capacity
    pub allow_ideation_borrow_idle_execution: bool,
}

fn default_workspace_update_max_concurrent() -> u32 {
    DEFAULT_WORKSPACE_MAX_CONCURRENT
}

// ========================================
// Quota Sync Helper
// ========================================

/// Result of syncing project quota
/// Contains the resolved project ID and the max concurrent value that was applied
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ProjectQuotaSync {
    /// The resolved project ID (None if global/no project)
    pub project_id: Option<ProjectId>,
    /// The max concurrent tasks value that was synced to execution_state
    pub max_concurrent: u32,
}

/// Syncs runtime ExecutionState max_concurrent with persisted project settings.
/// Returns the resolved project ID and the effective max_concurrent value.
///
/// Resolution order:
/// 1. Explicit project_id parameter
/// 2. Active project from active_project_state
/// 3. None (uses global default settings)
///
/// This helper ensures the runtime quota always reflects the active project's
/// persisted settings, preventing drift when switching projects or querying status.
pub(super) async fn sync_quota_from_project(
    project_id: Option<ProjectId>,
    active_project_state: &Arc<ActiveProjectState>,
    execution_state: &Arc<ExecutionState>,
    app_state: &AppState,
) -> Result<(Option<ProjectId>, u32), String> {
    // Determine effective project_id: explicit param > active project > none
    let effective_project_id = match project_id {
        Some(id) => Some(id),
        None => active_project_state.get().await,
    };

    // Load execution settings for the effective project
    let settings = app_state
        .execution_settings_repo
        .get_settings(effective_project_id.as_ref())
        .await
        .map_err(|e| e.to_string())?;

    // Sync runtime ExecutionState with persisted project settings
    execution_state.set_max_concurrent(settings.max_concurrent_tasks);
    execution_state.set_project_ideation_max(settings.project_ideation_max);

    Ok((effective_project_id, settings.max_concurrent_tasks))
}

/// Wrapper that returns a `ProjectQuotaSync` struct instead of a tuple.
/// Delegates to `sync_quota_from_project` for the actual logic.
#[allow(dead_code)]
pub(super) async fn sync_project_quota(
    explicit_project_id: Option<ProjectId>,
    active_project_state: &Arc<ActiveProjectState>,
    execution_state: &Arc<ExecutionState>,
    app_state: &AppState,
) -> Result<ProjectQuotaSync, String> {
    let (project_id, max_concurrent) = sync_quota_from_project(
        explicit_project_id,
        active_project_state,
        execution_state,
        app_state,
    )
    .await?;

    Ok(ProjectQuotaSync {
        project_id,
        max_concurrent,
    })
}
