use serde::{Deserialize, Serialize};

use super::ProjectId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectMemorySettings {
    pub project_id: ProjectId,
    pub enabled: bool,
    pub maintenance_categories: Vec<String>,
    pub capture_categories: Vec<String>,
}

impl ProjectMemorySettings {
    pub fn default_for_project(project_id: ProjectId) -> Self {
        Self {
            project_id,
            enabled: true,
            maintenance_categories: vec![
                "execution".to_string(),
                "review".to_string(),
                "merge".to_string(),
            ],
            capture_categories: vec![
                "planning".to_string(),
                "execution".to_string(),
                "review".to_string(),
            ],
        }
    }
}
