use crate::application::agent_conversation_workspace::{
    is_terminal_agent_conversation_publication_status,
    resolve_agent_conversation_workspace_path_for_send,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceStatus, Project,
};

const WORKSPACE_MISSING_ERROR: &str = "Agent conversation workspace is missing";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentWorkspaceContinuationAvailability {
    Available,
    Blocked(AgentWorkspaceContinuationBlock),
}

impl AgentWorkspaceContinuationAvailability {
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }

    pub fn blocked_reason(&self) -> Option<&AgentWorkspaceContinuationBlock> {
        match self {
            Self::Available => None,
            Self::Blocked(reason) => Some(reason),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentWorkspaceContinuationBlock {
    ArchivedWorkspace,
    TerminalWorkspace,
    CleanedAfterTerminal,
    LocalWorkspaceMissing,
    UnknownRequiresManualCheck(String),
}

impl AgentWorkspaceContinuationBlock {
    pub fn code(&self) -> &'static str {
        match self {
            Self::ArchivedWorkspace => "archived_workspace",
            Self::TerminalWorkspace => "terminal_workspace",
            Self::CleanedAfterTerminal => "cleaned_after_terminal",
            Self::LocalWorkspaceMissing => "local_workspace_missing",
            Self::UnknownRequiresManualCheck(_) => "unknown_requires_manual_check",
        }
    }

    pub fn user_message(&self) -> String {
        match self {
            Self::ArchivedWorkspace => {
                "This Agent workspace is archived and cannot be resumed.".to_string()
            }
            Self::TerminalWorkspace => {
                "This Agent workspace has reached a terminal PR state and should not be resumed automatically.".to_string()
            }
            Self::CleanedAfterTerminal => {
                "This Agent workspace was cleaned after its PR reached a terminal state. Start a fresh Agent conversation to continue from the current checkout.".to_string()
            }
            Self::LocalWorkspaceMissing => {
                "This Agent workspace is missing locally. Restore the worktree or start a fresh Agent conversation.".to_string()
            }
            Self::UnknownRequiresManualCheck(detail) => {
                format!(
                    "This Agent workspace cannot be resumed until its workspace state is checked manually: {detail}"
                )
            }
        }
    }
}

pub fn classify_agent_workspace_continuation(
    project: &Project,
    workspace: &AgentConversationWorkspace,
) -> AgentWorkspaceContinuationAvailability {
    match workspace.status {
        AgentConversationWorkspaceStatus::Archived => {
            return AgentWorkspaceContinuationAvailability::Blocked(
                AgentWorkspaceContinuationBlock::ArchivedWorkspace,
            );
        }
        AgentConversationWorkspaceStatus::Missing => {
            return AgentWorkspaceContinuationAvailability::Blocked(
                AgentWorkspaceContinuationBlock::LocalWorkspaceMissing,
            );
        }
        AgentConversationWorkspaceStatus::Active => {}
    }

    let terminal_pr = is_terminal_agent_conversation_publication_status(
        workspace.publication_pr_status.as_deref(),
    );
    match resolve_agent_conversation_workspace_path_for_send(project, workspace) {
        Ok(_) if terminal_pr => AgentWorkspaceContinuationAvailability::Blocked(
            AgentWorkspaceContinuationBlock::TerminalWorkspace,
        ),
        Ok(_) => AgentWorkspaceContinuationAvailability::Available,
        Err(error) => {
            let detail = error.to_string();
            if detail.contains(WORKSPACE_MISSING_ERROR) {
                let reason = if terminal_pr {
                    AgentWorkspaceContinuationBlock::CleanedAfterTerminal
                } else {
                    AgentWorkspaceContinuationBlock::LocalWorkspaceMissing
                };
                AgentWorkspaceContinuationAvailability::Blocked(reason)
            } else {
                AgentWorkspaceContinuationAvailability::Blocked(
                    AgentWorkspaceContinuationBlock::UnknownRequiresManualCheck(detail),
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::agent_conversation_workspace::resolve_agent_conversation_workspace_path;
    use crate::domain::entities::{
        AgentConversationWorkspaceMode, ChatConversationId, IdeationAnalysisBaseRefKind,
    };
    use std::fs;

    fn test_project(parent: &tempfile::TempDir) -> Project {
        let project_root = parent.path().join("project-root");
        fs::create_dir_all(&project_root).expect("project root should be created");
        let mut project = Project::new(
            "Continuation Guard".to_string(),
            project_root.to_string_lossy().to_string(),
        );
        project.worktree_parent_directory = Some(
            parent
                .path()
                .join("worktrees")
                .to_string_lossy()
                .to_string(),
        );
        project
    }

    fn test_workspace(
        project: &Project,
        conversation_id: ChatConversationId,
    ) -> AgentConversationWorkspace {
        let expected_path = resolve_agent_conversation_workspace_path(project, &conversation_id)
            .expect("expected workspace path should resolve");
        AgentConversationWorkspace::new(
            conversation_id,
            project.id.clone(),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("main".to_string()),
            Some("base-sha".to_string()),
            "ralphx/continuation/agent-test".to_string(),
            expected_path.to_string_lossy().to_string(),
        )
    }

    fn create_git_worktree(project: &Project, workspace: &AgentConversationWorkspace) {
        let path = resolve_agent_conversation_workspace_path(project, &workspace.conversation_id)
            .expect("expected workspace path should resolve");
        fs::create_dir_all(&path).expect("workspace path should be created");
        fs::write(path.join(".git"), "gitdir: ../.git/worktrees/agent-test")
            .expect("git marker should be created");
    }

    #[test]
    fn classify_allows_active_non_terminal_workspace_with_valid_worktree() {
        let parent = tempfile::tempdir().expect("temp dir should be created");
        let project = test_project(&parent);
        let workspace = test_workspace(&project, ChatConversationId::new());
        create_git_worktree(&project, &workspace);

        let availability = classify_agent_workspace_continuation(&project, &workspace);

        assert!(availability.is_available());
        assert_eq!(availability.blocked_reason(), None);
    }

    #[test]
    fn classify_blocks_terminal_workspace_even_when_worktree_exists() {
        let parent = tempfile::tempdir().expect("temp dir should be created");
        let project = test_project(&parent);
        let mut workspace = test_workspace(&project, ChatConversationId::new());
        workspace.publication_pr_status = Some("closed".to_string());
        create_git_worktree(&project, &workspace);

        let reason = classify_agent_workspace_continuation(&project, &workspace)
            .blocked_reason()
            .cloned();

        assert_eq!(
            reason,
            Some(AgentWorkspaceContinuationBlock::TerminalWorkspace)
        );
        assert_eq!(reason.unwrap().code(), "terminal_workspace");
    }

    #[test]
    fn classify_blocks_cleaned_workspace_after_terminal_pr() {
        let parent = tempfile::tempdir().expect("temp dir should be created");
        let project = test_project(&parent);
        let mut workspace = test_workspace(&project, ChatConversationId::new());
        workspace.publication_pr_status = Some("merged".to_string());

        let reason = classify_agent_workspace_continuation(&project, &workspace)
            .blocked_reason()
            .cloned();

        assert_eq!(
            reason,
            Some(AgentWorkspaceContinuationBlock::CleanedAfterTerminal)
        );
        assert_eq!(reason.unwrap().code(), "cleaned_after_terminal");
    }

    #[test]
    fn classify_blocks_missing_non_terminal_workspace() {
        let parent = tempfile::tempdir().expect("temp dir should be created");
        let project = test_project(&parent);
        let workspace = test_workspace(&project, ChatConversationId::new());

        let reason = classify_agent_workspace_continuation(&project, &workspace)
            .blocked_reason()
            .cloned();

        assert_eq!(
            reason,
            Some(AgentWorkspaceContinuationBlock::LocalWorkspaceMissing)
        );
        assert_eq!(reason.unwrap().code(), "local_workspace_missing");
    }

    #[test]
    fn classify_blocks_archived_and_recorded_missing_status_before_path_check() {
        let parent = tempfile::tempdir().expect("temp dir should be created");
        let project = test_project(&parent);
        let mut archived = test_workspace(&project, ChatConversationId::new());
        archived.status = AgentConversationWorkspaceStatus::Archived;
        let mut missing = test_workspace(&project, ChatConversationId::new());
        missing.status = AgentConversationWorkspaceStatus::Missing;

        assert_eq!(
            classify_agent_workspace_continuation(&project, &archived)
                .blocked_reason()
                .cloned(),
            Some(AgentWorkspaceContinuationBlock::ArchivedWorkspace)
        );
        assert_eq!(
            classify_agent_workspace_continuation(&project, &missing)
                .blocked_reason()
                .cloned(),
            Some(AgentWorkspaceContinuationBlock::LocalWorkspaceMissing)
        );
    }

    #[test]
    fn classify_unknown_manual_check_for_non_missing_path_error() {
        let parent = tempfile::tempdir().expect("temp dir should be created");
        let project = test_project(&parent);
        let mut workspace = test_workspace(&project, ChatConversationId::new());
        workspace.worktree_path = parent
            .path()
            .join("unexpected-worktree-path")
            .to_string_lossy()
            .to_string();

        let reason = classify_agent_workspace_continuation(&project, &workspace)
            .blocked_reason()
            .cloned();

        assert_eq!(
            reason.as_ref().map(AgentWorkspaceContinuationBlock::code),
            Some("unknown_requires_manual_check")
        );
        assert!(reason.unwrap().user_message().contains("checked manually"));
    }

    #[test]
    fn continuation_block_codes_and_messages_cover_all_variants() {
        let cases = [
            (
                AgentWorkspaceContinuationBlock::ArchivedWorkspace,
                "archived_workspace",
                "archived",
            ),
            (
                AgentWorkspaceContinuationBlock::TerminalWorkspace,
                "terminal_workspace",
                "terminal PR state",
            ),
            (
                AgentWorkspaceContinuationBlock::CleanedAfterTerminal,
                "cleaned_after_terminal",
                "cleaned",
            ),
            (
                AgentWorkspaceContinuationBlock::LocalWorkspaceMissing,
                "local_workspace_missing",
                "missing locally",
            ),
            (
                AgentWorkspaceContinuationBlock::UnknownRequiresManualCheck(
                    "repository unavailable".to_string(),
                ),
                "unknown_requires_manual_check",
                "repository unavailable",
            ),
        ];

        for (reason, code, message_part) in cases {
            assert_eq!(reason.code(), code);
            assert!(
                reason.user_message().contains(message_part),
                "message for {code} should include {message_part:?}"
            );
        }
    }
}
