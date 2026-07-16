use std::sync::Arc;

use crate::application::agent_conversation_archive::close_agent_workspace_pr_for_restart;
use crate::application::AppState;
use crate::domain::entities::plan_branch::PrStatus;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ArtifactId, ChatConversation,
    IdeationAnalysisBaseRefKind, IdeationSessionId, PlanBranch, Project,
};
use crate::domain::services::github_service::GithubServiceTrait;
use crate::error::AppError;
use crate::tests::mock_github_service::MockGithubService;

async fn setup_restart_pr_state() -> (
    tempfile::TempDir,
    AppState,
    AgentConversationWorkspace,
    PlanBranch,
) {
    let project_dir = tempfile::tempdir().expect("project directory should be created");
    let state = AppState::new_test();
    let project = state
        .project_repo
        .create(Project::new(
            "Restart PR test".to_string(),
            project_dir.path().to_string_lossy().to_string(),
        ))
        .await
        .expect("project should be persisted");
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .expect("conversation should be persisted");
    let session_id = IdeationSessionId::new();
    let plan_branch = state
        .plan_branch_repo
        .create(PlanBranch::new(
            ArtifactId::new(),
            session_id.clone(),
            project.id.clone(),
            "ralphx/restart-pr-test/plan".to_string(),
            "main".to_string(),
        ))
        .await
        .expect("plan branch should be persisted");
    state
        .plan_branch_repo
        .update_pr_info(
            &plan_branch.id,
            41,
            "https://github.com/example/repo/pull/41".to_string(),
            PrStatus::Open,
            false,
        )
        .await
        .expect("plan PR should be persisted");
    let plan_branch = state
        .plan_branch_repo
        .get_by_id(&plan_branch.id)
        .await
        .expect("plan branch lookup should succeed")
        .expect("plan branch should exist");

    let mut workspace = AgentConversationWorkspace::new(
        conversation.id.clone(),
        project.id,
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        None,
        "ralphx/restart-pr-test/plan".to_string(),
        project_dir
            .path()
            .join("linked-worktree")
            .to_string_lossy()
            .to_string(),
    );
    workspace.linked_ideation_session_id = Some(session_id);
    workspace.linked_plan_branch_id = Some(plan_branch.id.clone());
    let workspace = state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should be persisted");
    state
        .agent_conversation_workspace_repo
        .update_publication(
            &conversation.id,
            Some(41),
            Some("https://github.com/example/repo/pull/41"),
            Some("open"),
            Some("pushed"),
        )
        .await
        .expect("workspace PR should be persisted");

    (project_dir, state, workspace, plan_branch)
}

#[tokio::test]
async fn restart_closes_open_remote_pr_without_clearing_local_pointers() {
    let (_project_dir, mut state, workspace, plan_branch) = setup_restart_pr_state().await;
    let github = Arc::new(MockGithubService::new());
    let github_service: Arc<dyn GithubServiceTrait> = github.clone();
    state.github_service = Some(github_service);

    close_agent_workspace_pr_for_restart(&workspace, &plan_branch, &state)
        .await
        .expect("restart should close the remote PR");

    assert_eq!(github.state().check_pr_status_calls, 1);
    assert_eq!(github.state().close_pr_calls, 1);
    assert_eq!(github.state().last_close_pr_number, Some(41));
    let refreshed_branch = state
        .plan_branch_repo
        .get_by_id(&plan_branch.id)
        .await
        .expect("plan branch lookup should succeed")
        .expect("plan branch should exist");
    assert_eq!(refreshed_branch.pr_number, Some(41));
    assert!(refreshed_branch.pr_url.is_some());
    assert_eq!(refreshed_branch.pr_status, Some(PrStatus::Open));
    let refreshed_workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(refreshed_workspace.publication_pr_number, Some(41));
    assert_eq!(
        refreshed_workspace.publication_pr_status.as_deref(),
        Some("open")
    );
}

#[tokio::test]
async fn restart_treats_terminal_remote_pr_as_idempotently_closed() {
    for remote_status in [
        crate::domain::services::github_service::PrStatus::Closed,
        crate::domain::services::github_service::PrStatus::Merged {
            merge_commit_sha: Some("abc123".to_string()),
            merged_at: Some("2026-07-15T10:00:00Z".to_string()),
        },
    ] {
        let (_project_dir, mut state, workspace, plan_branch) = setup_restart_pr_state().await;
        let github = Arc::new(MockGithubService::new());
        github.state().check_pr_status_result = Some(Ok(remote_status));
        let github_service: Arc<dyn GithubServiceTrait> = github.clone();
        state.github_service = Some(github_service);

        close_agent_workspace_pr_for_restart(&workspace, &plan_branch, &state)
            .await
            .expect("terminal remote PR state should be retry-safe");

        assert_eq!(github.state().check_pr_status_calls, 1);
        assert_eq!(github.state().close_pr_calls, 0);
        assert_eq!(
            state
                .plan_branch_repo
                .get_by_id(&plan_branch.id)
                .await
                .expect("plan branch lookup should succeed")
                .expect("plan branch should exist")
                .pr_number,
            Some(41),
            "local pointers clear only in the replacement transaction"
        );
    }
}

#[tokio::test]
async fn restart_checks_remote_pr_even_when_local_state_is_terminal() {
    let (_project_dir, mut state, mut workspace, mut plan_branch) = setup_restart_pr_state().await;
    plan_branch.pr_status = Some(PrStatus::Closed);
    workspace.publication_pr_status = Some("closed".to_string());
    let github = Arc::new(MockGithubService::new());
    let github_service: Arc<dyn GithubServiceTrait> = github.clone();
    state.github_service = Some(github_service);

    close_agent_workspace_pr_for_restart(&workspace, &plan_branch, &state)
        .await
        .expect("remote open state must override stale terminal local state");

    assert_eq!(github.state().check_pr_status_calls, 1);
    assert_eq!(github.state().close_pr_calls, 1);
    assert_eq!(github.state().last_close_pr_number, Some(41));
}

#[tokio::test]
async fn restart_reconciles_distinct_plan_and_workspace_pr_numbers() {
    let (_project_dir, mut state, mut workspace, plan_branch) = setup_restart_pr_state().await;
    workspace.publication_pr_number = Some(42);
    let github = Arc::new(MockGithubService::new());
    let github_service: Arc<dyn GithubServiceTrait> = github.clone();
    state.github_service = Some(github_service);

    close_agent_workspace_pr_for_restart(&workspace, &plan_branch, &state)
        .await
        .expect("every stored PR pointer must be reconciled");

    assert_eq!(github.state().check_pr_status_calls, 2);
    assert_eq!(github.state().close_pr_calls, 2);
    assert_eq!(github.state().last_close_pr_number, Some(42));
}

#[tokio::test]
async fn restart_fails_closed_when_remote_pr_status_lookup_fails() {
    let (_project_dir, mut state, workspace, plan_branch) = setup_restart_pr_state().await;
    let github = Arc::new(MockGithubService::new());
    github.state().check_pr_status_result = Some(Err(AppError::Infrastructure("offline".into())));
    let github_service: Arc<dyn GithubServiceTrait> = github.clone();
    state.github_service = Some(github_service);

    let error = close_agent_workspace_pr_for_restart(&workspace, &plan_branch, &state)
        .await
        .expect_err("restart should fail when remote PR status cannot be proven");

    assert!(error.to_string().contains("could not check existing PR 41"));
    assert_eq!(github.state().close_pr_calls, 0);
    assert_eq!(
        state
            .plan_branch_repo
            .get_by_id(&plan_branch.id)
            .await
            .expect("plan branch lookup should succeed")
            .expect("plan branch should exist")
            .pr_number,
        Some(41)
    );
}

#[tokio::test]
async fn restart_does_not_clear_local_pr_state_when_remote_close_fails() {
    let (_project_dir, mut state, workspace, plan_branch) = setup_restart_pr_state().await;
    let github = Arc::new(MockGithubService::new());
    github.state().close_pr_result = Some(Err(AppError::Infrastructure("offline".to_string())));
    let github_service: Arc<dyn GithubServiceTrait> = github.clone();
    state.github_service = Some(github_service);

    let error = close_agent_workspace_pr_for_restart(&workspace, &plan_branch, &state)
        .await
        .expect_err("restart should fail when remote PR closure fails");

    assert!(error.to_string().contains("could not close existing PR 41"));
    assert_eq!(github.state().close_pr_calls, 1);
    let refreshed_branch = state
        .plan_branch_repo
        .get_by_id(&plan_branch.id)
        .await
        .expect("plan branch lookup should succeed")
        .expect("plan branch should exist");
    assert_eq!(refreshed_branch.pr_number, Some(41));
    assert!(refreshed_branch.pr_status.is_some());
    let refreshed_workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(refreshed_workspace.publication_pr_number, Some(41));
    assert_eq!(
        refreshed_workspace.publication_pr_status.as_deref(),
        Some("open")
    );
}

#[tokio::test]
async fn restart_requires_github_integration_before_clearing_local_pr_state() {
    let (_project_dir, state, workspace, plan_branch) = setup_restart_pr_state().await;

    let error = close_agent_workspace_pr_for_restart(&workspace, &plan_branch, &state)
        .await
        .expect_err("restart should require GitHub integration for an open PR");

    assert!(matches!(error, AppError::Validation(_)));
    let refreshed_branch = state
        .plan_branch_repo
        .get_by_id(&plan_branch.id)
        .await
        .expect("plan branch lookup should succeed")
        .expect("plan branch should exist");
    assert_eq!(refreshed_branch.pr_number, Some(41));
    let refreshed_workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(refreshed_workspace.publication_pr_number, Some(41));
}
