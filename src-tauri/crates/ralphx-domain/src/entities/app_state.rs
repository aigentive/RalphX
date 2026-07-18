use crate::entities::ProjectId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExecutionHaltMode {
    #[default]
    Running,
    Paused,
    Stopped,
}

#[derive(Debug, Clone)]
pub struct AppSettings {
    pub active_project_id: Option<ProjectId>,
    pub execution_halt_mode: ExecutionHaltMode,
    pub last_seen_release_notes_version: Option<String>,
    pub remove_inherited_github_cli_tokens: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            active_project_id: None,
            execution_halt_mode: ExecutionHaltMode::Running,
            last_seen_release_notes_version: None,
            remove_inherited_github_cli_tokens: true,
        }
    }
}
