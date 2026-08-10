use chrono::{DateTime, Utc};

use crate::domain::entities::ProjectId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRepositoryCapability {
    pub project_id: ProjectId,
    pub kind: String,
    pub fetch_url: Option<String>,
    pub push_url: Option<String>,
    pub message: Option<String>,
    pub inspected_at: DateTime<Utc>,
    pub working_directory: String,
}
