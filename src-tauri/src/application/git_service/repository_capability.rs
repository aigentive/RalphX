use std::path::Path;

use crate::error::{AppError, AppResult};
use crate::infrastructure::git_auth::{inspect_repository_capability, RepositoryCapability};

use super::GitService;

impl GitService {
    /// Determine whether the repository can start a new GitHub PR workflow.
    ///
    /// Inspection failures remain errors so transition callers must fail closed
    /// instead of treating an unreadable origin as a local-only repository.
    pub async fn supports_github_prs(working_dir: &Path) -> AppResult<bool> {
        match inspect_repository_capability(working_dir).await {
            RepositoryCapability::Github { .. } => Ok(true),
            RepositoryCapability::LocalOnly | RepositoryCapability::OtherRemote { .. } => Ok(false),
            RepositoryCapability::InspectionFailed { message } => Err(AppError::Validation(
                format!("Cannot inspect repository capability: {message}"),
            )),
        }
    }
}
