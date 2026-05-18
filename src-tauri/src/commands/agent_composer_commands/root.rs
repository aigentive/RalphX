use std::path::{Path, PathBuf};

use crate::application::AppState;
use crate::domain::entities::{ChatConversationId, Project};
use crate::utils::path_safety::validate_absolute_non_root_path;

pub(super) async fn resolve_composer_root(
    project: &Project,
    conversation_id: Option<&str>,
    state: &AppState,
) -> Result<PathBuf, String> {
    if let Some(conversation_id) = conversation_id.filter(|value| !value.trim().is_empty()) {
        let conversation_id = ChatConversationId::from_string(conversation_id.to_string());
        if let Some(workspace) = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .map_err(|error| error.to_string())?
            .filter(|workspace| workspace.project_id == project.id)
        {
            return validate_composer_root(Path::new(&workspace.worktree_path));
        }
    }
    validate_composer_root(Path::new(&project.working_directory))
}

pub(super) fn validate_composer_root(path: &Path) -> Result<PathBuf, String> {
    let safe = validate_absolute_non_root_path(path, "agent composer root")
        .map_err(|error| error.to_string())?;
    let canonical = safe
        .canonicalize()
        .map_err(|error| format!("Failed to resolve agent composer root: {error}"))?;
    if !canonical.is_dir() {
        return Err(format!(
            "Agent composer root is not a directory: {}",
            canonical.display()
        ));
    }
    Ok(canonical)
}
