use serde::{Deserialize, Serialize};

use super::ProjectId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectSkillSettings {
    pub project_id: ProjectId,
    pub export_enabled: bool,
}

impl ProjectSkillSettings {
    pub fn default_for_project(project_id: ProjectId) -> Self {
        Self {
            project_id,
            export_enabled: false,
        }
    }
}
