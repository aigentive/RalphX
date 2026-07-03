use serde::{Deserialize, Serialize};

pub const DEFAULT_WORKSPACE_MAX_CONCURRENT: u32 = 10;

/// Execution settings for task scheduling and automation
/// Can be stored per-project (with project_id) or as global defaults (project_id = None)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecutionSettings {
    /// Maximum number of concurrent tasks that can execute simultaneously (per-project)
    pub max_concurrent_tasks: u32,
    /// Maximum number of ideation/verification sessions this project may run concurrently.
    pub project_ideation_max: u32,
    /// Whether to auto-commit changes after successful task completion
    pub auto_commit: bool,
    /// Whether to pause execution when a task fails
    pub pause_on_failure: bool,
    /// Default Autofix CI & Reviews setting for newly created agent workspaces.
    #[serde(default)]
    pub agent_workspace_pr_autofix_default: bool,
    /// Default GitHub auto-merge setting for newly created agent workspaces.
    #[serde(default)]
    pub agent_workspace_pr_auto_merge_default: bool,
}

impl Default for ExecutionSettings {
    fn default() -> Self {
        // Last-resort fallback only. Keep aligned with ralphx.yaml execution_defaults.project.
        Self {
            max_concurrent_tasks: 10,
            project_ideation_max: 5,
            auto_commit: true,
            pause_on_failure: true,
            agent_workspace_pr_autofix_default: false,
            agent_workspace_pr_auto_merge_default: false,
        }
    }
}

/// Global execution settings that apply across all projects
/// Phase 82: Global concurrency cap to prevent system overload
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GlobalExecutionSettings {
    /// Maximum total concurrent tasks across ALL projects (hard cap)
    /// Default: 20, UI max: 50
    pub global_max_concurrent: u32,
    /// Maximum number of workspace main-agent conversations allowed globally.
    #[serde(default = "default_workspace_max_concurrent")]
    pub workspace_max_concurrent: u32,
    /// Maximum number of ideation/verification sessions allowed globally.
    pub global_ideation_max: u32,
    /// When true, ideation may borrow idle execution capacity if no runnable execution work waits.
    pub allow_ideation_borrow_idle_execution: bool,
}

fn default_workspace_max_concurrent() -> u32 {
    DEFAULT_WORKSPACE_MAX_CONCURRENT
}

impl Default for GlobalExecutionSettings {
    fn default() -> Self {
        // Last-resort fallback only. Keep aligned with ralphx.yaml execution_defaults.global.
        Self {
            global_max_concurrent: 20,
            workspace_max_concurrent: DEFAULT_WORKSPACE_MAX_CONCURRENT,
            global_ideation_max: 10,
            allow_ideation_borrow_idle_execution: false,
        }
    }
}

impl GlobalExecutionSettings {
    /// Maximum allowed value for global_max_concurrent (UI enforced)
    pub const MAX_ALLOWED: u32 = 50;

    /// Validate and clamp global_max_concurrent to allowed range
    pub fn validate(&self) -> Self {
        Self {
            global_max_concurrent: self.global_max_concurrent.clamp(1, Self::MAX_ALLOWED),
            workspace_max_concurrent: self.workspace_max_concurrent.clamp(1, Self::MAX_ALLOWED),
            global_ideation_max: self.global_ideation_max.clamp(1, Self::MAX_ALLOWED),
            allow_ideation_borrow_idle_execution: self.allow_ideation_borrow_idle_execution,
        }
    }
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
