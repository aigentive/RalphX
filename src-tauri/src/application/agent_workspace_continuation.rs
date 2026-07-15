use crate::application::agent_conversation_workspace::{
    is_terminal_agent_conversation_publication_status,
    resolve_agent_conversation_workspace_path_for_send,
    resolve_effective_agent_conversation_workspace_path,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceStatus, Project,
};
use crate::domain::repositories::PlanBranchRepository;
use crate::error::AppResult;

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

    classify_resolved_workspace(
        workspace,
        resolve_agent_conversation_workspace_path_for_send(project, workspace),
    )
}

pub async fn classify_agent_workspace_continuation_with_plan_branch(
    project: &Project,
    workspace: &AgentConversationWorkspace,
    plan_branch_repo: Option<&dyn PlanBranchRepository>,
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

    let resolved = if workspace.linked_plan_branch_id.is_some() {
        match plan_branch_repo {
            Some(repo) => {
                resolve_effective_agent_conversation_workspace_path(project, workspace, repo)
                    .await
                    .map(|resolved| resolved.path)
            }
            None => Err(crate::error::AppError::Validation(
                "Linked plan branch repository is unavailable".to_string(),
            )),
        }
    } else {
        resolve_agent_conversation_workspace_path_for_send(project, workspace)
    };
    classify_resolved_workspace(workspace, resolved)
}

fn classify_resolved_workspace(
    workspace: &AgentConversationWorkspace,
    resolved: AppResult<std::path::PathBuf>,
) -> AgentWorkspaceContinuationAvailability {
    let terminal_pr = is_terminal_agent_conversation_publication_status(
        workspace.publication_pr_status.as_deref(),
    );
    match resolved {
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
