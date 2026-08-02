use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentWorkspaceRepairSource, ChatConversation, ChatConversationId,
};
use crate::domain::repositories::ChatConversationRepository;
use crate::error::AppResult;
use crate::infrastructure::agents::claude::agent_names::{
    AGENT_WORKSPACE_PR_FIXER, AGENT_WORKSPACE_REPAIR,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentWorkspaceFixerKind {
    WorkspaceRepair,
    PrFixer,
}

impl AgentWorkspaceFixerKind {
    pub fn agent_name(self) -> &'static str {
        match self {
            Self::WorkspaceRepair => AGENT_WORKSPACE_REPAIR,
            Self::PrFixer => AGENT_WORKSPACE_PR_FIXER,
        }
    }

    pub fn launch_role(self) -> &'static str {
        match self {
            Self::WorkspaceRepair => "workspace_repair",
            Self::PrFixer => "pr_fixer",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentWorkspaceFixerTitleContext {
    Repair(AgentWorkspaceRepairSource),
    ReviewBlocking,
    PullRequest(Option<i64>),
}

pub async fn ensure_agent_workspace_fixer_conversation(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    existing: Option<&ChatConversationId>,
    kind: AgentWorkspaceFixerKind,
    title_context: AgentWorkspaceFixerTitleContext,
) -> AppResult<ChatConversationId> {
    ensure_agent_workspace_fixer_conversation_with_repo(
        state.chat_conversation_repo.as_ref(),
        workspace,
        existing,
        kind,
        title_context,
    )
    .await
}

pub(crate) async fn ensure_agent_workspace_fixer_conversation_with_repo(
    conversation_repo: &dyn ChatConversationRepository,
    workspace: &AgentConversationWorkspace,
    existing: Option<&ChatConversationId>,
    kind: AgentWorkspaceFixerKind,
    title_context: AgentWorkspaceFixerTitleContext,
) -> AppResult<ChatConversationId> {
    if let Some(existing) = existing {
        return Ok(*existing);
    }

    create_agent_workspace_fixer_conversation_with_repo(
        conversation_repo,
        workspace,
        kind,
        title_context,
    )
    .await
}

pub async fn create_agent_workspace_fixer_conversation(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    kind: AgentWorkspaceFixerKind,
    title_context: AgentWorkspaceFixerTitleContext,
) -> AppResult<ChatConversationId> {
    create_agent_workspace_fixer_conversation_with_repo(
        state.chat_conversation_repo.as_ref(),
        workspace,
        kind,
        title_context,
    )
    .await
}

async fn create_agent_workspace_fixer_conversation_with_repo(
    conversation_repo: &dyn ChatConversationRepository,
    workspace: &AgentConversationWorkspace,
    kind: AgentWorkspaceFixerKind,
    title_context: AgentWorkspaceFixerTitleContext,
) -> AppResult<ChatConversationId> {
    let mut conversation = ChatConversation::new_project(workspace.project_id.clone());
    conversation.parent_conversation_id = Some(workspace.conversation_id.as_str());
    conversation.title = Some(agent_workspace_fixer_conversation_title(
        kind,
        title_context,
    ));
    let conversation = conversation_repo.create(conversation).await?;
    Ok(conversation.id)
}

fn agent_workspace_fixer_conversation_title(
    kind: AgentWorkspaceFixerKind,
    title_context: AgentWorkspaceFixerTitleContext,
) -> String {
    match (kind, title_context) {
        (
            AgentWorkspaceFixerKind::PrFixer,
            AgentWorkspaceFixerTitleContext::PullRequest(Some(number)),
        ) => {
            format!("Fix PR #{number}")
        }
        (AgentWorkspaceFixerKind::PrFixer, _) => "Fix PR checks".to_string(),
        (_, AgentWorkspaceFixerTitleContext::ReviewBlocking) => "Fix review findings".to_string(),
        (_, AgentWorkspaceFixerTitleContext::Repair(AgentWorkspaceRepairSource::BaseUpdate)) => {
            "Fix base update".to_string()
        }
        (_, AgentWorkspaceFixerTitleContext::Repair(AgentWorkspaceRepairSource::Publish)) => {
            "Fix publish".to_string()
        }
        (_, AgentWorkspaceFixerTitleContext::Repair(AgentWorkspaceRepairSource::PrConflict)) => {
            "Fix PR conflict".to_string()
        }
        (_, AgentWorkspaceFixerTitleContext::Repair(_)) => "Fix workspace".to_string(),
        (AgentWorkspaceFixerKind::WorkspaceRepair, AgentWorkspaceFixerTitleContext::PullRequest(Some(number))) => {
            format!("Fix PR #{number}")
        }
        (AgentWorkspaceFixerKind::WorkspaceRepair, AgentWorkspaceFixerTitleContext::PullRequest(None)) => {
            "Fix workspace".to_string()
        }
    }
}
