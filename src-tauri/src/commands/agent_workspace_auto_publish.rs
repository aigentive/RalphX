use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;
use tauri::{Emitter, Listener, Manager};

use crate::application::agent_conversation_workspace::{
    is_terminal_agent_conversation_publication_status,
    resolve_valid_agent_conversation_workspace_path,
};
use crate::application::chat_service::events::{AGENT_RUN_COMPLETED, AGENT_TURN_COMPLETED};
use crate::application::publish_resilience::count_unpublished_publish_commits;
use crate::application::{AppState, GitService, TeamService};
use crate::commands::unified_chat_commands::publish_agent_conversation_workspace_for_app_state;
use crate::commands::ExecutionState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentConversationWorkspaceStatus,
    ChatContextType, ChatConversationId,
};

#[derive(Debug, Deserialize)]
struct AgentCompletionPayload {
    conversation_id: String,
    context_type: ChatContextType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AutoPublishFacts {
    has_uncommitted_changes: bool,
    unpublished_commit_count: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutoPublishDecision {
    Publish,
    Skip(AutoPublishSkipReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutoPublishSkipReason {
    InactiveWorkspace,
    NotEditWorkspace,
    ExecutionOwnedWorkspace,
    NoExistingPr,
    TerminalPr,
    PublishAlreadyActive,
    NoPendingLocalWork,
}

impl AutoPublishSkipReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::InactiveWorkspace => "inactive_workspace",
            Self::NotEditWorkspace => "not_edit_workspace",
            Self::ExecutionOwnedWorkspace => "execution_owned_workspace",
            Self::NoExistingPr => "no_existing_pr",
            Self::TerminalPr => "terminal_pr",
            Self::PublishAlreadyActive => "publish_already_active",
            Self::NoPendingLocalWork => "no_pending_local_work",
        }
    }
}

/// Register backend-only listeners that continue already-published agent
/// workspace PRs after an agent turn finishes. First-time PR creation remains a
/// deliberate user action; this only updates PRs that already exist.
pub(crate) fn install_agent_workspace_auto_publish_listeners(app: &tauri::App<tauri::Wry>) {
    let run_completed_handle = app.handle().clone();
    app.listen_any(AGENT_RUN_COMPLETED, move |event| {
        spawn_auto_publish_from_completion_event(
            run_completed_handle.clone(),
            AGENT_RUN_COMPLETED,
            event.payload(),
        );
    });

    let turn_completed_handle = app.handle().clone();
    app.listen_any(AGENT_TURN_COMPLETED, move |event| {
        spawn_auto_publish_from_completion_event(
            turn_completed_handle.clone(),
            AGENT_TURN_COMPLETED,
            event.payload(),
        );
    });
}

fn spawn_auto_publish_from_completion_event(
    app_handle: tauri::AppHandle,
    event_name: &'static str,
    payload: &str,
) {
    let payload = match serde_json::from_str::<AgentCompletionPayload>(payload) {
        Ok(payload) => payload,
        Err(error) => {
            tracing::warn!(
                event_name,
                error = %error,
                "Skipping agent workspace auto-publish: completion payload could not be parsed"
            );
            return;
        }
    };

    if payload.context_type != ChatContextType::Project {
        return;
    }

    let conversation_id = ChatConversationId::from_string(payload.conversation_id);
    tauri::async_runtime::spawn(async move {
        match auto_publish_existing_agent_workspace_pr_from_app_handle(&app_handle, conversation_id)
            .await
        {
            Ok(AutoPublishDecision::Publish) => {}
            Ok(AutoPublishDecision::Skip(reason)) => {
                tracing::debug!(
                    event_name,
                    reason = reason.as_str(),
                    "Skipped agent workspace auto-publish"
                );
            }
            Err(error) => {
                tracing::warn!(
                    event_name,
                    error = %error,
                    "Agent workspace auto-publish failed"
                );
            }
        }
    });
}

async fn auto_publish_existing_agent_workspace_pr_from_app_handle(
    app_handle: &tauri::AppHandle,
    conversation_id: ChatConversationId,
) -> Result<AutoPublishDecision, String> {
    let state = app_handle
        .try_state::<AppState>()
        .ok_or_else(|| "AppState is not available".to_string())?;
    let execution_state = app_handle
        .try_state::<Arc<ExecutionState>>()
        .ok_or_else(|| "ExecutionState is not available".to_string())?
        .inner()
        .clone();
    let team_service = app_handle
        .try_state::<Arc<TeamService>>()
        .map(|state| state.inner().clone());

    auto_publish_existing_agent_workspace_pr(
        state.inner(),
        &execution_state,
        team_service,
        Some(app_handle.clone()),
        conversation_id,
    )
    .await
}

async fn auto_publish_existing_agent_workspace_pr(
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
    team_service: Option<Arc<TeamService>>,
    app_handle: Option<tauri::AppHandle>,
    conversation_id: ChatConversationId,
) -> Result<AutoPublishDecision, String> {
    let Some(workspace) = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(AutoPublishDecision::Skip(
            AutoPublishSkipReason::NoExistingPr,
        ));
    };

    if let Some(reason) = static_auto_publish_skip_reason(&workspace) {
        return Ok(AutoPublishDecision::Skip(reason));
    }

    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Project not found: {}", workspace.project_id))?;
    let worktree_path = resolve_valid_agent_conversation_workspace_path(&project, &workspace)
        .await
        .map_err(|error| error.to_string())?;
    let facts = collect_auto_publish_facts(&workspace, worktree_path).await?;
    let decision = should_auto_publish_existing_pr(&workspace, facts);
    if decision != AutoPublishDecision::Publish {
        return Ok(decision);
    }

    tracing::info!(
        conversation_id = %workspace.conversation_id,
        pr_number = workspace.publication_pr_number,
        "Auto-publishing existing agent workspace PR after agent completion"
    );
    let result = publish_agent_conversation_workspace_for_app_state(
        state,
        execution_state,
        team_service,
        conversation_id,
        true,
    )
    .await;

    if let Some(app_handle) = app_handle.as_ref() {
        let _ = app_handle.emit(
            "agent:workspace_changed",
            serde_json::json!({ "conversation_id": conversation_id.as_str() }),
        );
    }

    if result.is_ok() {
        return Ok(AutoPublishDecision::Publish);
    }

    if publish_was_routed_to_agent_repair(state, &conversation_id).await? {
        tracing::info!(
            conversation_id = %workspace.conversation_id,
            "Auto-publish routed existing agent workspace PR through repair agent"
        );
        return Ok(AutoPublishDecision::Publish);
    }

    result.map(|_| AutoPublishDecision::Publish)
}

async fn publish_was_routed_to_agent_repair(
    state: &AppState,
    conversation_id: &ChatConversationId,
) -> Result<bool, String> {
    Ok(state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await
        .map_err(|error| error.to_string())?
        .and_then(|workspace| workspace.publication_push_status)
        .as_deref()
        == Some("needs_agent"))
}

async fn collect_auto_publish_facts(
    workspace: &AgentConversationWorkspace,
    worktree_path: PathBuf,
) -> Result<AutoPublishFacts, String> {
    let has_uncommitted_changes = GitService::has_uncommitted_changes(&worktree_path)
        .await
        .map_err(|error| error.to_string())?;
    let unpublished_commit_count =
        count_unpublished_publish_commits(&worktree_path, &workspace.branch_name)
            .await
            .map_err(|error| error.to_string())?;

    Ok(AutoPublishFacts {
        has_uncommitted_changes,
        unpublished_commit_count,
    })
}

fn should_auto_publish_existing_pr(
    workspace: &AgentConversationWorkspace,
    facts: AutoPublishFacts,
) -> AutoPublishDecision {
    if let Some(reason) = static_auto_publish_skip_reason(workspace) {
        return AutoPublishDecision::Skip(reason);
    }
    if !facts.has_uncommitted_changes && facts.unpublished_commit_count.unwrap_or(0) == 0 {
        return AutoPublishDecision::Skip(AutoPublishSkipReason::NoPendingLocalWork);
    }

    AutoPublishDecision::Publish
}

fn static_auto_publish_skip_reason(
    workspace: &AgentConversationWorkspace,
) -> Option<AutoPublishSkipReason> {
    if workspace.status != AgentConversationWorkspaceStatus::Active {
        return Some(AutoPublishSkipReason::InactiveWorkspace);
    }
    if workspace.mode != AgentConversationWorkspaceMode::Edit {
        return Some(AutoPublishSkipReason::NotEditWorkspace);
    }
    if workspace.is_execution_owned() {
        return Some(AutoPublishSkipReason::ExecutionOwnedWorkspace);
    }
    if workspace.publication_pr_number.is_none() {
        return Some(AutoPublishSkipReason::NoExistingPr);
    }
    if is_terminal_agent_conversation_publication_status(workspace.publication_pr_status.as_deref())
    {
        return Some(AutoPublishSkipReason::TerminalPr);
    }
    if workspace
        .publication_push_status
        .as_deref()
        .is_some_and(is_active_publish_status)
    {
        return Some(AutoPublishSkipReason::PublishAlreadyActive);
    }

    None
}

fn is_active_publish_status(status: &str) -> bool {
    matches!(
        status,
        "checking" | "committing" | "refreshing" | "describing" | "pushing" | "needs_agent"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::{IdeationAnalysisBaseRefKind, ProjectId};

    fn workspace() -> AgentConversationWorkspace {
        let mut workspace = AgentConversationWorkspace::new(
            ChatConversationId::from_string("11111111-1111-1111-1111-111111111111"),
            ProjectId::from_string("project-1".to_string()),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("main".to_string()),
            Some("0".repeat(40)),
            "ralphx/test/agent-workspace".to_string(),
            "/tmp/ralphx-agent-workspace".to_string(),
        );
        workspace.publication_pr_number = Some(42);
        workspace.publication_pr_status = Some("open".to_string());
        workspace.publication_push_status = Some("pushed".to_string());
        workspace
    }

    #[test]
    fn auto_publish_requires_existing_pr() {
        let mut workspace = workspace();
        workspace.publication_pr_number = None;

        let decision = should_auto_publish_existing_pr(
            &workspace,
            AutoPublishFacts {
                has_uncommitted_changes: true,
                unpublished_commit_count: Some(0),
            },
        );

        assert_eq!(
            decision,
            AutoPublishDecision::Skip(AutoPublishSkipReason::NoExistingPr)
        );
    }

    #[test]
    fn auto_publish_runs_for_existing_pr_with_uncommitted_changes() {
        let decision = should_auto_publish_existing_pr(
            &workspace(),
            AutoPublishFacts {
                has_uncommitted_changes: true,
                unpublished_commit_count: Some(0),
            },
        );

        assert_eq!(decision, AutoPublishDecision::Publish);
    }

    #[test]
    fn auto_publish_runs_for_existing_pr_with_unpublished_commits() {
        let decision = should_auto_publish_existing_pr(
            &workspace(),
            AutoPublishFacts {
                has_uncommitted_changes: false,
                unpublished_commit_count: Some(2),
            },
        );

        assert_eq!(decision, AutoPublishDecision::Publish);
    }

    #[test]
    fn auto_publish_skips_existing_pr_without_pending_local_work() {
        let decision = should_auto_publish_existing_pr(
            &workspace(),
            AutoPublishFacts {
                has_uncommitted_changes: false,
                unpublished_commit_count: Some(0),
            },
        );

        assert_eq!(
            decision,
            AutoPublishDecision::Skip(AutoPublishSkipReason::NoPendingLocalWork)
        );
    }

    #[test]
    fn auto_publish_skips_when_publish_or_repair_already_active() {
        let mut workspace = workspace();
        workspace.publication_push_status = Some("needs_agent".to_string());

        let decision = should_auto_publish_existing_pr(
            &workspace,
            AutoPublishFacts {
                has_uncommitted_changes: true,
                unpublished_commit_count: Some(0),
            },
        );

        assert_eq!(
            decision,
            AutoPublishDecision::Skip(AutoPublishSkipReason::PublishAlreadyActive)
        );
    }
}
