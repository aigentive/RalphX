use crate::application::agent_workspace_fixer_conversation::{
    ensure_agent_workspace_fixer_conversation_with_repo, AgentWorkspaceFixerKind,
    AgentWorkspaceFixerTitleContext,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentWorkspaceRepairSource,
    ChatConversationId, IdeationAnalysisBaseRefKind, ProjectId,
};
use crate::domain::repositories::ChatConversationRepository;
use crate::infrastructure::memory::MemoryChatConversationRepository;

fn test_workspace() -> AgentConversationWorkspace {
    AgentConversationWorkspace::new(
        ChatConversationId::from_string("conv-fixer-test"),
        ProjectId::from_string("proj-fixer-test".to_string()),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        None,
        "ralphx/fixer-test".to_string(),
        "/tmp/fixer-test".to_string(),
    )
}

#[test]
fn fixer_kind_agent_name_workspace_repair() {
    assert_eq!(
        AgentWorkspaceFixerKind::WorkspaceRepair.agent_name(),
        "ralphx:ralphx-agent-workspace-repair"
    );
}

#[test]
fn fixer_kind_agent_name_pr_fixer() {
    assert_eq!(
        AgentWorkspaceFixerKind::PrFixer.agent_name(),
        "ralphx:ralphx-agent-workspace-pr-fixer"
    );
}

#[test]
fn fixer_kind_launch_role_workspace_repair() {
    assert_eq!(
        AgentWorkspaceFixerKind::WorkspaceRepair.launch_role(),
        "workspace_repair"
    );
}

#[test]
fn fixer_kind_launch_role_pr_fixer() {
    assert_eq!(AgentWorkspaceFixerKind::PrFixer.launch_role(), "pr_fixer");
}

#[tokio::test]
async fn ensure_returns_existing_conversation_id() {
    let repo = MemoryChatConversationRepository::new();
    let workspace = test_workspace();
    let existing = ChatConversationId::from_string("existing-conv");

    let result = ensure_agent_workspace_fixer_conversation_with_repo(
        &repo,
        &workspace,
        Some(&existing),
        AgentWorkspaceFixerKind::WorkspaceRepair,
        AgentWorkspaceFixerTitleContext::ReviewBlocking,
    )
    .await
    .unwrap();

    assert_eq!(result, existing);
}

#[tokio::test]
async fn ensure_creates_new_conversation_when_none_exists() {
    let repo = MemoryChatConversationRepository::new();
    let workspace = test_workspace();

    let result = ensure_agent_workspace_fixer_conversation_with_repo(
        &repo,
        &workspace,
        None,
        AgentWorkspaceFixerKind::WorkspaceRepair,
        AgentWorkspaceFixerTitleContext::ReviewBlocking,
    )
    .await
    .unwrap();

    assert_ne!(result.as_str(), "");
}

#[tokio::test]
async fn create_sets_parent_conversation_id() {
    let repo = MemoryChatConversationRepository::new();
    let workspace = test_workspace();

    let conv_id = ensure_agent_workspace_fixer_conversation_with_repo(
        &repo,
        &workspace,
        None,
        AgentWorkspaceFixerKind::PrFixer,
        AgentWorkspaceFixerTitleContext::PullRequest(Some(42)),
    )
    .await
    .unwrap();

    let conv = repo.get_by_id(&conv_id).await.unwrap().unwrap();
    assert_eq!(
        conv.parent_conversation_id,
        Some(workspace.conversation_id.as_str())
    );
}

#[tokio::test]
async fn title_pr_fixer_with_number() {
    let repo = MemoryChatConversationRepository::new();
    let workspace = test_workspace();

    let conv_id = ensure_agent_workspace_fixer_conversation_with_repo(
        &repo,
        &workspace,
        None,
        AgentWorkspaceFixerKind::PrFixer,
        AgentWorkspaceFixerTitleContext::PullRequest(Some(123)),
    )
    .await
    .unwrap();

    let conv = repo.get_by_id(&conv_id).await.unwrap().unwrap();
    assert_eq!(conv.title, Some("Fix PR #123".to_string()));
}

#[tokio::test]
async fn title_pr_fixer_without_number() {
    let repo = MemoryChatConversationRepository::new();
    let workspace = test_workspace();

    let conv_id = ensure_agent_workspace_fixer_conversation_with_repo(
        &repo,
        &workspace,
        None,
        AgentWorkspaceFixerKind::PrFixer,
        AgentWorkspaceFixerTitleContext::ReviewBlocking,
    )
    .await
    .unwrap();

    let conv = repo.get_by_id(&conv_id).await.unwrap().unwrap();
    assert_eq!(conv.title, Some("Fix PR checks".to_string()));
}

#[tokio::test]
async fn title_review_blocking() {
    let repo = MemoryChatConversationRepository::new();
    let workspace = test_workspace();

    let conv_id = ensure_agent_workspace_fixer_conversation_with_repo(
        &repo,
        &workspace,
        None,
        AgentWorkspaceFixerKind::WorkspaceRepair,
        AgentWorkspaceFixerTitleContext::ReviewBlocking,
    )
    .await
    .unwrap();

    let conv = repo.get_by_id(&conv_id).await.unwrap().unwrap();
    assert_eq!(conv.title, Some("Fix review findings".to_string()));
}

#[tokio::test]
async fn title_repair_base_update() {
    let repo = MemoryChatConversationRepository::new();
    let workspace = test_workspace();

    let conv_id = ensure_agent_workspace_fixer_conversation_with_repo(
        &repo,
        &workspace,
        None,
        AgentWorkspaceFixerKind::WorkspaceRepair,
        AgentWorkspaceFixerTitleContext::Repair(AgentWorkspaceRepairSource::BaseUpdate),
    )
    .await
    .unwrap();

    let conv = repo.get_by_id(&conv_id).await.unwrap().unwrap();
    assert_eq!(conv.title, Some("Fix base update".to_string()));
}

#[tokio::test]
async fn title_repair_publish() {
    let repo = MemoryChatConversationRepository::new();
    let workspace = test_workspace();

    let conv_id = ensure_agent_workspace_fixer_conversation_with_repo(
        &repo,
        &workspace,
        None,
        AgentWorkspaceFixerKind::WorkspaceRepair,
        AgentWorkspaceFixerTitleContext::Repair(AgentWorkspaceRepairSource::Publish),
    )
    .await
    .unwrap();

    let conv = repo.get_by_id(&conv_id).await.unwrap().unwrap();
    assert_eq!(conv.title, Some("Fix publish".to_string()));
}

#[tokio::test]
async fn title_repair_pr_conflict() {
    let repo = MemoryChatConversationRepository::new();
    let workspace = test_workspace();

    let conv_id = ensure_agent_workspace_fixer_conversation_with_repo(
        &repo,
        &workspace,
        None,
        AgentWorkspaceFixerKind::WorkspaceRepair,
        AgentWorkspaceFixerTitleContext::Repair(AgentWorkspaceRepairSource::PrConflict),
    )
    .await
    .unwrap();

    let conv = repo.get_by_id(&conv_id).await.unwrap().unwrap();
    assert_eq!(conv.title, Some("Fix PR conflict".to_string()));
}

#[tokio::test]
async fn title_repair_fallback() {
    let repo = MemoryChatConversationRepository::new();
    let workspace = test_workspace();

    let conv_id = ensure_agent_workspace_fixer_conversation_with_repo(
        &repo,
        &workspace,
        None,
        AgentWorkspaceFixerKind::WorkspaceRepair,
        AgentWorkspaceFixerTitleContext::Repair(AgentWorkspaceRepairSource::Legacy),
    )
    .await
    .unwrap();

    let conv = repo.get_by_id(&conv_id).await.unwrap().unwrap();
    assert_eq!(conv.title, Some("Fix workspace".to_string()));
}

#[tokio::test]
async fn title_workspace_repair_pull_request_with_number() {
    let repo = MemoryChatConversationRepository::new();
    let workspace = test_workspace();

    let conv_id = ensure_agent_workspace_fixer_conversation_with_repo(
        &repo,
        &workspace,
        None,
        AgentWorkspaceFixerKind::WorkspaceRepair,
        AgentWorkspaceFixerTitleContext::PullRequest(Some(99)),
    )
    .await
    .unwrap();

    let conv = repo.get_by_id(&conv_id).await.unwrap().unwrap();
    assert_eq!(conv.title, Some("Fix PR #99".to_string()));
}

#[tokio::test]
async fn title_workspace_repair_pull_request_none() {
    let repo = MemoryChatConversationRepository::new();
    let workspace = test_workspace();

    let conv_id = ensure_agent_workspace_fixer_conversation_with_repo(
        &repo,
        &workspace,
        None,
        AgentWorkspaceFixerKind::WorkspaceRepair,
        AgentWorkspaceFixerTitleContext::PullRequest(None),
    )
    .await
    .unwrap();

    let conv = repo.get_by_id(&conv_id).await.unwrap().unwrap();
    assert_eq!(conv.title, Some("Fix workspace".to_string()));
}
