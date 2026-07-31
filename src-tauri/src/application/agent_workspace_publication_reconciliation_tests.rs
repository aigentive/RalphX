use std::sync::Arc;

use crate::application::agent_workspace_publication_reconciliation::{
    correct_foreign_agent_workspace_publication, PublicationCorrectionOutcome,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentConversationWorkspaceStatus,
    ChatConversation, IdeationAnalysisBaseRefKind, Project,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, ChatConversationRepository,
};
use crate::domain::services::github_service::PrDetail;
use crate::domain::services::{GithubServiceTrait, PrStatus};
use crate::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryChatConversationRepository,
};
use crate::tests::mock_github_service::MockGithubService;

fn project() -> Project {
    Project::new(
        "Publication recovery".to_string(),
        "/tmp/publication-recovery".to_string(),
    )
}

fn workspace(project: &Project) -> AgentConversationWorkspace {
    AgentConversationWorkspace::new(
        crate::domain::entities::ChatConversationId::from_string(
            "publication-recovery-conversation".to_string(),
        ),
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("0".repeat(40)),
        "ralphx/publication-recovery".to_string(),
        "/tmp/publication-recovery-worktree".to_string(),
    )
}

async fn setup(
    project: Project,
    mut workspace: AgentConversationWorkspace,
) -> (
    Project,
    AgentConversationWorkspace,
    Arc<MemoryAgentConversationWorkspaceRepository>,
    Arc<MemoryChatConversationRepository>,
    Arc<MockGithubService>,
) {
    workspace.publication_pr_number = Some(41);
    workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/41".to_string());
    workspace.publication_pr_status = Some("closed".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .unwrap();
    let conversation_repo = Arc::new(MemoryChatConversationRepository::new());
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.id = workspace.conversation_id.clone();
    conversation_repo.create(conversation).await.unwrap();
    let github = Arc::new(MockGithubService::new());
    (
        project,
        workspace,
        workspace_repo,
        conversation_repo,
        github,
    )
}

#[tokio::test]
async fn foreign_publication_is_cleared_once_and_terminal_archive_is_restored() {
    let project = project();
    let mut workspace = workspace(&project);
    workspace.status = AgentConversationWorkspaceStatus::Archived;
    let (project, workspace, workspace_repo, conversation_repo, github) =
        setup(project, workspace).await;
    github.will_return_pr_detail(PrDetail {
        number: 41,
        title: "Foreign PR".to_string(),
        body: None,
        author: None,
        created_at: None,
        url: None,
        state: PrStatus::Closed,
        is_draft: false,
        head_ref_name: "main".to_string(),
        base_ref_name: "develop".to_string(),
    });
    let workspace_repo_trait: Arc<dyn AgentConversationWorkspaceRepository> =
        workspace_repo.clone();
    let conversation_repo_trait: Arc<dyn ChatConversationRepository> = conversation_repo.clone();
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();

    let outcome = correct_foreign_agent_workspace_publication(
        workspace_repo_trait.clone(),
        conversation_repo_trait.clone(),
        github_trait.clone(),
        &project,
        &workspace,
    )
    .await
    .unwrap();
    assert_eq!(outcome, PublicationCorrectionOutcome::Corrected);

    let cleared = workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(cleared.publication_pr_number, None);
    assert_eq!(cleared.status, AgentConversationWorkspaceStatus::Active);
    let events = workspace_repo
        .list_publication_events(&workspace.conversation_id)
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].step, "publication_association_corrected");

    let rerun = correct_foreign_agent_workspace_publication(
        workspace_repo_trait,
        conversation_repo_trait,
        github_trait,
        &project,
        &cleared,
    )
    .await
    .unwrap();
    assert_eq!(rerun, PublicationCorrectionOutcome::NotApplicable);
    assert_eq!(
        workspace_repo
            .list_publication_events(&workspace.conversation_id)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn owned_publication_and_read_failures_leave_workspace_untouched() {
    let project = project();
    let workspace = workspace(&project);
    let (project, workspace, workspace_repo, conversation_repo, github) =
        setup(project, workspace).await;
    github.will_return_pr_detail(PrDetail {
        number: 41,
        title: "Owned PR".to_string(),
        body: None,
        author: None,
        created_at: None,
        url: None,
        state: PrStatus::Open,
        is_draft: true,
        head_ref_name: workspace.branch_name.clone(),
        base_ref_name: "main".to_string(),
    });
    let workspace_repo_trait: Arc<dyn AgentConversationWorkspaceRepository> =
        workspace_repo.clone();
    let conversation_repo_trait: Arc<dyn ChatConversationRepository> = conversation_repo.clone();
    let github_trait: Arc<dyn GithubServiceTrait> = github.clone();

    assert_eq!(
        correct_foreign_agent_workspace_publication(
            workspace_repo_trait.clone(),
            conversation_repo_trait,
            github_trait,
            &project,
            &workspace,
        )
        .await
        .unwrap(),
        PublicationCorrectionOutcome::Skipped
    );
    assert_eq!(
        workspace_repo
            .get_by_conversation_id(&workspace.conversation_id)
            .await
            .unwrap()
            .unwrap()
            .publication_pr_number,
        Some(41)
    );

    github.will_fail_pr_detail("offline");
    assert_eq!(
        correct_foreign_agent_workspace_publication(
            workspace_repo_trait,
            Arc::new(MemoryChatConversationRepository::new()),
            github.clone(),
            &project,
            &workspace,
        )
        .await
        .unwrap(),
        PublicationCorrectionOutcome::Unverified
    );
    assert_eq!(
        workspace_repo
            .get_by_conversation_id(&workspace.conversation_id)
            .await
            .unwrap()
            .unwrap()
            .publication_pr_number,
        Some(41)
    );
    assert!(workspace_repo
        .list_publication_events(&workspace.conversation_id)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn archived_conversation_prevents_status_restoration_and_inapplicable_workspaces_skip() {
    let project = project();
    let mut workspace = workspace(&project);
    workspace.status = AgentConversationWorkspaceStatus::Archived;
    let (project, workspace, workspace_repo, conversation_repo, github) =
        setup(project, workspace).await;
    conversation_repo
        .archive(&workspace.conversation_id)
        .await
        .unwrap();
    github.will_return_pr_detail(PrDetail {
        number: 41,
        title: "Foreign PR".to_string(),
        body: None,
        author: None,
        created_at: None,
        url: None,
        state: PrStatus::Closed,
        is_draft: false,
        head_ref_name: "main".to_string(),
        base_ref_name: "develop".to_string(),
    });

    assert_eq!(
        correct_foreign_agent_workspace_publication(
            workspace_repo.clone(),
            conversation_repo,
            github,
            &project,
            &workspace,
        )
        .await
        .unwrap(),
        PublicationCorrectionOutcome::Corrected
    );
    assert_eq!(
        workspace_repo
            .get_by_conversation_id(&workspace.conversation_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AgentConversationWorkspaceStatus::Archived
    );

    let mut review_workspace = workspace.clone();
    review_workspace.mode = AgentConversationWorkspaceMode::ReviewPr;
    assert_eq!(
        correct_foreign_agent_workspace_publication(
            workspace_repo,
            Arc::new(MemoryChatConversationRepository::new()),
            Arc::new(MockGithubService::new()),
            &project,
            &review_workspace,
        )
        .await
        .unwrap(),
        PublicationCorrectionOutcome::NotApplicable
    );

    let mut linked_workspace = workspace;
    linked_workspace.mode = AgentConversationWorkspaceMode::Edit;
    linked_workspace.linked_plan_branch_id = Some(
        crate::domain::entities::PlanBranchId::from_string("linked-plan-branch".to_string()),
    );
    assert_eq!(
        correct_foreign_agent_workspace_publication(
            Arc::new(MemoryAgentConversationWorkspaceRepository::new()),
            Arc::new(MemoryChatConversationRepository::new()),
            Arc::new(MockGithubService::new()),
            &project,
            &linked_workspace,
        )
        .await
        .unwrap(),
        PublicationCorrectionOutcome::NotApplicable
    );
}
