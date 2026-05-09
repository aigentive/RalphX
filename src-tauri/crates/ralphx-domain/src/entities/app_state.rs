use crate::entities::ProjectId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExecutionHaltMode {
    #[default]
    Running,
    Paused,
    Stopped,
}

#[derive(Debug, Clone, Default)]
pub struct AppSettings {
    pub active_project_id: Option<ProjectId>,
    pub execution_halt_mode: ExecutionHaltMode,
    pub last_seen_release_notes_version: Option<String>,
}
