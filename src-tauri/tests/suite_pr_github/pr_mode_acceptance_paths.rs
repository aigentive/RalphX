
use std::sync::Arc;
use std::time::Duration;

use axum::{extract::State, http::HeaderMap, Json};
use ralphx_lib::application::services::PrPollerRegistry;
use ralphx_lib::application::{AppState, TeamService, TeamStateTracker};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{
    AcceptanceStatus, IdeationSession, IdeationSessionId, Priority, Project, ProjectId,
    ProposalCategory, TaskProposal,
};
use ralphx_lib::domain::services::github_service::GithubServiceTrait;
use ralphx_lib::http_server::handlers::{
    accept_finalize, external_apply_proposals, finalize_proposals, AcceptFinalizeRequest,
    ExternalApplyProposalsRequest, FinalizeProposalsRequest,
};
use ralphx_lib::http_server::project_scope::ProjectScope;
use ralphx_lib::http_server::types::HttpServerState;

use crate::common::MockGithubService;
use crate::support::real_git_repo::{setup_real_git_repo, RealGitRepo};

fn unrestricted_scope() -> ProjectScope {
    ProjectScope(None)
}

fn setup_http_state_with_pr_mode() -> (HttpServerState, Arc<MockGithubService>) {
    let mut app_state = AppState::new_sqlite_for_apply_test();
    let mock_github = Arc::new(MockGithubService::new());
    let github_trait: Arc<dyn GithubServiceTrait> = mock_github.clone();

    app_state.github_service = Some(Arc::clone(&github_trait));
    app_state.pr_poller_registry = Arc::new(PrPollerRegistry::new(
        Some(github_trait),
        Arc::clone(&app_state.plan_branch_repo),
    ));

    let app_state = Arc::new(app_state);
    let execution_state = Arc::new(ExecutionState::new());
    let tracker = TeamStateTracker::new();
    let team_service = Arc::new(TeamService::new_without_events(Arc::new(tracker.clone())));

    (
        HttpServerState {
            app_state,
            execution_state,
            team_tracker: tracker,
            team_service,
            delegation_service: Default::default(),
        },
        mock_github,
    )
}

async fn create_project_and_session(
    state: &HttpServerState,
    project_id: &str,
    repo: &RealGitRepo,
    acceptance_status: Option<AcceptanceStatus>,
) -> IdeationSessionId {
    let mut project = Project::new("PR Mode Acceptance".to_string(), repo.path_string());
    project.id = ProjectId::from_string(project_id.to_string());
    project.github_pr_enabled = true;
    state.app_state.project_repo.create(project).await.unwrap();

    let session = IdeationSession::new(ProjectId::from_string(project_id.to_string()));
    let session_id = state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap()
        .id;

    if let Some(status) = acceptance_status {
        state
            .app_state
            .ideation_session_repo
            .update_acceptance_status(&session_id, None, Some(status))
            .await
            .unwrap();
    }

    session_id
}

async fn create_single_feature_proposal(state: &HttpServerState, session_id: &IdeationSessionId) {
    let mut proposal = TaskProposal::new(
        session_id.clone(),
        "Create initial plan task",
        ProposalCategory::Feature,
        Priority::Medium,
    );
    proposal.affected_paths = Some("[\"README.md\"]".to_string());

    state
        .app_state
        .task_proposal_repo
        .create(proposal)
        .await
        .unwrap();
}

async fn wait_for_initial_scheduler_tick() {
    tokio::time::sleep(Duration::from_millis(300)).await;
}

#[tokio::test]
async fn accept_finalize_defers_pr_creation_until_plan_branch_has_changes() {
    let (state, mock_github) = setup_http_state_with_pr_mode();
    let repo = setup_real_git_repo();
    let session_id = create_project_and_session(
        &state,
        "proj-accept-finalize-pr",
        &repo,
        Some(AcceptanceStatus::Pending),
    )
    .await;
    create_single_feature_proposal(&state, &session_id).await;

    let response = accept_finalize(
        State(state.clone()),
        Json(AcceptFinalizeRequest {
            session_id: session_id.as_str().to_string(),
        }),
    )
    .await;

    assert!(
        response.is_ok(),
        "accept_finalize should succeed: {:?}",
        response.err()
    );

    wait_for_initial_scheduler_tick().await;

    let branch = state
        .app_state
        .plan_branch_repo
        .get_by_session_id(&session_id)
        .await
        .unwrap()
        .unwrap();
    assert!(branch.pr_eligible, "accepted plan branch should be PR-eligible");
    assert!(
        branch.pr_number.is_none(),
        "accepting a plan should not create a PR before the plan branch has reviewable changes"
    );
    assert_eq!(
        mock_github.push_calls(),
        0,
        "initial scheduling should not push an empty plan branch"
    );
    assert_eq!(
        mock_github.create_calls(),
        0,
        "initial scheduling should not create a draft PR before any work lands on the plan branch"
    );
}

#[tokio::test]
async fn finalize_proposals_defers_pr_creation_until_plan_branch_has_changes() {
    let (state, mock_github) = setup_http_state_with_pr_mode();
    let repo = setup_real_git_repo();
    let session_id =
        create_project_and_session(&state, "proj-internal-pr", &repo, None).await;
    create_single_feature_proposal(&state, &session_id).await;

    let response = finalize_proposals(
        State(state.clone()),
        HeaderMap::new(),
        Json(FinalizeProposalsRequest {
            session_id: session_id.as_str().to_string(),
        }),
    )
    .await;

    assert!(
        response.is_ok(),
        "finalize_proposals should succeed: {:?}",
        response.err()
    );

    wait_for_initial_scheduler_tick().await;

    let branch = state
        .app_state
        .plan_branch_repo
        .get_by_session_id(&session_id)
        .await
        .unwrap()
        .unwrap();
    assert!(branch.pr_eligible, "internal finalize path should create a PR-eligible plan branch");
    assert!(
        branch.pr_number.is_none(),
        "internal finalize should defer PR creation until the plan branch is ahead of base"
    );
    assert_eq!(mock_github.push_calls(), 0);
    assert_eq!(mock_github.create_calls(), 0);
}

#[tokio::test]
async fn external_apply_proposals_defers_pr_creation_until_plan_branch_has_changes() {
    let (state, mock_github) = setup_http_state_with_pr_mode();
    let repo = setup_real_git_repo();
    let session_id =
        create_project_and_session(&state, "proj-external-pr", &repo, None).await;
    create_single_feature_proposal(&state, &session_id).await;

    let response = external_apply_proposals(
        State(state.clone()),
        unrestricted_scope(),
        Json(ExternalApplyProposalsRequest {
            session_id: session_id.as_str().to_string(),
            proposal_ids: state
                .app_state
                .task_proposal_repo
                .get_by_session(&session_id)
                .await
                .unwrap()
                .into_iter()
                .map(|proposal| proposal.id.as_str().to_string())
                .collect(),
            target_column: "auto".to_string(),
            base_branch_override: None,
        }),
    )
    .await;

    assert!(
        response.is_ok(),
        "external_apply_proposals should succeed: {:?}",
        response.err()
    );

    wait_for_initial_scheduler_tick().await;

    let branch = state
        .app_state
        .plan_branch_repo
        .get_by_session_id(&session_id)
        .await
        .unwrap()
        .unwrap();
    assert!(branch.pr_eligible, "external apply path should create a PR-eligible plan branch");
    assert!(
        branch.pr_number.is_none(),
        "external apply should defer PR creation until the plan branch is ahead of base"
    );
    assert_eq!(mock_github.push_calls(), 0);
    assert_eq!(mock_github.create_calls(), 0);
}
