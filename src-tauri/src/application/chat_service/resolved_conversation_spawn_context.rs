use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::domain::entities::{AgentConversationWorkspaceMode, ChatContextType, ChatConversation};
use crate::domain::repositories::{ConversationFolderReferenceRepository, ProjectRepository};

use super::chat_service_context::{
    build_mcp_runtime_context, folder_references_skip_reason, mcp_lineage_parent_conversation_id,
    resolve_mcp_filesystem_read_roots, resolve_mcp_filesystem_read_roots_with_folder_references,
    FOLDER_REFS_SKIPPED_CONTEXT_UNAVAILABLE,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedConversationSpawnContext {
    pub folder_refs_block: Option<String>,
    pub folder_roots: Vec<PathBuf>,
    pub workspace_root: PathBuf,
    pub enforce_filesystem_roots: bool,
}

impl ResolvedConversationSpawnContext {
    pub(crate) fn without_app_state(
        context_type: ChatContextType,
        effective_mode: Option<AgentConversationWorkspaceMode>,
        working_directory: &Path,
    ) -> Self {
        let enforce_filesystem_roots = build_mcp_runtime_context(
            context_type,
            "",
            None,
            "",
            None,
            working_directory,
            None,
            None,
            &[],
            None,
            None,
            effective_mode,
        )
        .enforce_filesystem_roots;

        Self {
            folder_refs_block: None,
            folder_roots: Vec::new(),
            workspace_root: working_directory.to_path_buf(),
            enforce_filesystem_roots,
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn resolve_conversation_spawn_context(
    conversation: &ChatConversation,
    effective_mode: Option<AgentConversationWorkspaceMode>,
    project_id: Option<&str>,
    project_repo: Arc<dyn ProjectRepository>,
    working_directory: &Path,
    runtime_app_data_dir: Option<&Path>,
    folder_reference_app_data_dir: Option<&Path>,
    folder_reference_repo: Option<Arc<dyn ConversationFolderReferenceRepository>>,
) -> crate::error::AppResult<ResolvedConversationSpawnContext> {
    let conversation_id = conversation.id.as_str();
    let folder_roots = match (folder_reference_app_data_dir, folder_reference_repo.clone()) {
        (Some(app_data_dir), Some(folder_reference_repo)) => {
            resolve_mcp_filesystem_read_roots_with_folder_references(
                conversation.context_type,
                project_id,
                project_repo,
                working_directory,
                effective_mode,
                Some(&conversation_id),
                runtime_app_data_dir,
                app_data_dir,
                folder_reference_repo,
            )
            .await?
        }
        _ => {
            resolve_mcp_filesystem_read_roots(
                conversation.context_type,
                project_id,
                project_repo,
                working_directory,
                effective_mode,
                Some(&conversation_id),
                runtime_app_data_dir,
            )
            .await
        }
    };
    let folder_refs_block = if let Some(reason) =
        folder_references_skip_reason(conversation.context_type, effective_mode)
    {
        tracing::warn!(
            conversation_id = conversation.id.as_str(),
            reason,
            "folder_refs_skipped"
        );
        None
    } else if let (Some(app_data_dir), Some(folder_reference_repo)) = (
        folder_reference_app_data_dir,
        folder_reference_repo.as_ref(),
    ) {
        crate::application::conversation_folder_reference_service::ConversationFolderReferenceService::new(
            Arc::clone(folder_reference_repo),
            app_data_dir.to_path_buf(),
            crate::infrastructure::agents::limits_config().max_live_folder_references,
        )
        .render_prompt_block_for_roots(&conversation.id, &folder_roots)
        .await?
    } else {
        tracing::warn!(
            conversation_id = conversation.id.as_str(),
            reason = FOLDER_REFS_SKIPPED_CONTEXT_UNAVAILABLE,
            "folder_refs_skipped"
        );
        None
    };
    let enforce_filesystem_roots = build_mcp_runtime_context(
        conversation.context_type,
        &conversation.context_id,
        Some(conversation.coordination_mode),
        &conversation_id,
        None,
        working_directory,
        None,
        project_id,
        &folder_roots,
        None,
        mcp_lineage_parent_conversation_id(conversation),
        effective_mode,
    )
    .enforce_filesystem_roots;

    Ok(ResolvedConversationSpawnContext {
        folder_refs_block,
        folder_roots,
        workspace_root: working_directory.to_path_buf(),
        enforce_filesystem_roots,
    })
}
