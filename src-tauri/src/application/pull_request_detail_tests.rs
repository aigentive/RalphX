//! Tests for the PR-detail composition. GitHub access is faked via
//! `MockGithubService`; project/workspace/plan-branch reads use in-memory repos.
//! Never touches a real `gh`.

use std::sync::Arc;

use crate::application::pull_request_detail::types::{
    PullRequestDetailState, PullRequestOrigin,
};
use crate::application::pull_request_detail::{
    load_pull_request_detail, PullRequestDetailDeps, PullRequestDetailRequest,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatConversationId,
    IdeationAnalysisBaseRefKind, Project, ProjectId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, PlanBranchRepository, ProjectRepository,
};
use crate::domain::services::github_service::{
    GithubServiceTrait, PrBranchMatch, PrDetail, PrHealth, PrHealthCheck, PrIssueCommentSummary,
    PrMergeStateStatus, PrMergeableState, PrStatus, PrSyncState,
};
use crate::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryPlanBranchRepository, MemoryProjectRepository,
};
use crate::tests::mock_github_service::MockGithubService;

const WORKING_DIR: &str = "/tmp/ralphx/pr-detail-repo";

struct Harness {
    github: Arc<MockGithubService>,
    project_repo: Arc<dyn ProjectRepository>,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    plan_branch_repo: Arc<dyn PlanBranchRepository>,
    project_id: ProjectId,
}

impl Harness {
    async fn new() -> Self {
        let github = Arc::new(MockGithubService::new());
        let project_repo: Arc<dyn ProjectRepository> = Arc::new(MemoryProjectRepository::new());
        let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
            Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let plan_branch_repo: Arc<dyn PlanBranchRepository> =
            Arc::new(MemoryPlanBranchRepository::new());

        let project = project_repo
            .create(Project::new("PR Detail".to_string(), WORKING_DIR.to_string()))
            .await
            .unwrap();
        let project_id = project.id.clone();

        Self {
            github,
            project_repo,
            workspace_repo,
            plan_branch_repo,
            project_id,
        }
    }

    fn deps(&self) -> PullRequestDetailDeps {
        PullRequestDetailDeps {
            github_service: Some(Arc::clone(&self.github) as Arc<dyn GithubServiceTrait>),
            project_repo: Arc::clone(&self.project_repo),
            workspace_repo: Arc::clone(&self.workspace_repo),
            plan_branch_repo: Arc::clone(&self.plan_branch_repo),
        }
    }

    async fn seed_workspace(&self, conversation_id: &str, branch: &str, publication_pr: Option<i64>) {
        let mut workspace = AgentConversationWorkspace::new(
            ChatConversationId::from_string(conversation_id),
            self.project_id.clone(),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            None,
            None,
            branch.to_string(),
            format!("/tmp/ralphx/{conversation_id}"),
        );
        workspace.publication_pr_number = publication_pr;
        workspace.publication_pr_url =
            publication_pr.map(|n| format!("https://github.com/owner/repo/pull/{n}"));
        workspace.publication_pr_status = publication_pr.map(|_| "open".to_string());
        self.workspace_repo.create_or_update(workspace).await.unwrap();
    }
}

fn sync_state(head_ref: &str) -> PrSyncState {
    PrSyncState {
        status: PrStatus::Open,
        merge_state_status: Some(PrMergeStateStatus::Clean),
        mergeable: Some(PrMergeableState::Mergeable),
        is_draft: false,
        head_ref_name: head_ref.to_string(),
        base_ref_name: "main".to_string(),
        head_ref_oid: None,
        base_ref_oid: None,
    }
}

fn health(checks: Vec<PrHealthCheck>, comments: Vec<PrIssueCommentSummary>) -> PrHealth {
    PrHealth {
        sync_state: sync_state("feature/x"),
        review_decision: Some("REVIEW_REQUIRED".to_string()),
        checks,
        issue_comments: comments,
        auto_merge_request: None,
    }
}

fn issue_comment(id: &str, body: &str) -> PrIssueCommentSummary {
    PrIssueCommentSummary {
        id: id.to_string(),
        author: Some("octocat".to_string()),
        body: body.to_string(),
        url: Some("https://github.com/owner/repo/pull/7#issuecomment-1".to_string()),
        created_at: Some("2026-06-24T09:00:00Z".to_string()),
        updated_at: None,
        is_bot: false,
        is_codecov: false,
    }
}

fn pr_detail(number: i64, head_ref: &str) -> PrDetail {
    PrDetail {
        number,
        title: "Add PR visibility".to_string(),
        body: Some("Body".to_string()),
        author: Some("adriandemian".to_string()),
        created_at: Some("2026-06-24T08:00:00Z".to_string()),
        url: Some(format!("https://github.com/owner/repo/pull/{number}")),
        state: PrStatus::Open,
        is_draft: false,
        head_ref_name: head_ref.to_string(),
        base_ref_name: "main".to_string(),
    }
}

#[tokio::test]
async fn unauthenticated_short_circuits_before_any_pr_fetch() {
    let harness = Harness::new().await;
    // gh connection-status default is unavailable (authenticated = false).
    let detail = load_pull_request_detail(
        harness.deps(),
        PullRequestDetailRequest {
            project_id: harness.project_id.clone(),
            pr_number: Some(7),
            branch: None,
        },
    )
    .await;

    assert_eq!(detail.state, PullRequestDetailState::GhUnauthenticated);
    assert!(detail.description.is_none());
    // No PR-data fetch happened — the auth gate fired first.
    assert_eq!(harness.github.state().fetch_pr_detail_calls, 0);
}

#[tokio::test]
async fn repo_unresolvable_when_project_missing() {
    let harness = Harness::new().await;
    harness.github.will_be_authenticated("github.com", "adriandemian");

    let detail = load_pull_request_detail(
        harness.deps(),
        PullRequestDetailRequest {
            project_id: ProjectId::from_string("does-not-exist".to_string()),
            pr_number: Some(7),
            branch: None,
        },
    )
    .await;

    assert_eq!(detail.state, PullRequestDetailState::RepoUnresolvable);
}

#[tokio::test]
async fn no_pr_when_branch_has_no_association() {
    let harness = Harness::new().await;
    harness.github.will_be_authenticated("github.com", "adriandemian");
    // No workspace/plan-branch and no live external match.
    harness.github.set_find_latest_pr_by_head_branch(Ok(None));

    let detail = load_pull_request_detail(
        harness.deps(),
        PullRequestDetailRequest {
            project_id: harness.project_id.clone(),
            pr_number: None,
            branch: Some("orphan/branch".to_string()),
        },
    )
    .await;

    assert_eq!(detail.state, PullRequestDetailState::NoPr);
    assert_eq!(harness.github.state().fetch_pr_detail_calls, 0);
}

#[tokio::test]
async fn owned_outbound_branch_loads_full_graph_with_evidence_comments() {
    let harness = Harness::new().await;
    harness.github.will_be_authenticated("github.com", "adriandemian");
    harness
        .seed_workspace("11111111-1111-1111-1111-111111111111", "feature/x", Some(7))
        .await;
    harness.github.will_return_pr_detail(pr_detail(7, "feature/x"));
    harness.github.state().fetch_pr_health_result = Some(Ok(health(
        vec![PrHealthCheck {
            name: "ci".to_string(),
            status: Some("completed".to_string()),
            conclusion: Some("success".to_string()),
            details_url: None,
        }],
        vec![issue_comment("c1", "Looks good")],
    )));

    let detail = load_pull_request_detail(
        harness.deps(),
        PullRequestDetailRequest {
            project_id: harness.project_id.clone(),
            pr_number: None,
            branch: Some("feature/x".to_string()),
        },
    )
    .await;

    assert_eq!(detail.state, PullRequestDetailState::Loaded);
    assert_eq!(detail.origin, Some(PullRequestOrigin::OwnedOutbound));
    assert_eq!(detail.description.as_ref().unwrap().number, 7);
    assert_eq!(detail.checks.len(), 1);

    // Issue comment came back through the single-writer evidence cache.
    assert_eq!(detail.issue_comments.len(), 1);
    assert_eq!(detail.issue_comments[0].source, "evidence");
    assert_eq!(detail.issue_comments[0].body, "Looks good");

    // RX rollup found the owning workspace by head ref.
    assert_eq!(detail.rx_conversations.len(), 1);
    assert_eq!(detail.rx_conversations[0].branch_name, "feature/x");

    // Annotations are opt-in — never fetched on open.
    assert_eq!(harness.github.state().fetch_pr_diff_annotations_calls, 0);
}

#[tokio::test]
async fn external_branch_loads_via_live_lookup_with_live_comments() {
    let harness = Harness::new().await;
    harness.github.will_be_authenticated("github.com", "adriandemian");
    harness.github.set_find_latest_pr_by_head_branch(Ok(Some(PrBranchMatch {
        number: 99,
        url: "https://github.com/owner/repo/pull/99".to_string(),
        status: PrStatus::Open,
        is_draft: false,
        head_ref_name: "external/feature".to_string(),
        updated_at: None,
    })));
    harness.github.will_return_pr_detail(pr_detail(99, "external/feature"));
    harness.github.state().fetch_pr_health_result =
        Some(Ok(health(Vec::new(), vec![issue_comment("c9", "external note")])));

    let detail = load_pull_request_detail(
        harness.deps(),
        PullRequestDetailRequest {
            project_id: harness.project_id.clone(),
            pr_number: None,
            branch: Some("external/feature".to_string()),
        },
    )
    .await;

    assert_eq!(detail.state, PullRequestDetailState::Loaded);
    assert_eq!(detail.origin, Some(PullRequestOrigin::External));
    // No owning workspace → comments are live (not evidence-cached).
    assert_eq!(detail.issue_comments.len(), 1);
    assert_eq!(detail.issue_comments[0].source, "live");
    // No RalphX conversation attached to an external branch.
    assert!(detail.rx_conversations.is_empty());
}

#[tokio::test]
async fn fetch_pr_detail_timeout_maps_to_fetch_timeout_state() {
    let harness = Harness::new().await;
    harness.github.will_be_authenticated("github.com", "adriandemian");
    harness
        .seed_workspace("22222222-2222-2222-2222-222222222222", "feature/y", Some(7))
        .await;
    harness
        .github
        .will_fail_pr_detail("gh command timed out after 30s");

    let detail = load_pull_request_detail(
        harness.deps(),
        PullRequestDetailRequest {
            project_id: harness.project_id.clone(),
            pr_number: Some(7),
            branch: None,
        },
    )
    .await;

    assert_eq!(detail.state, PullRequestDetailState::FetchTimeout);
}

#[tokio::test]
async fn rate_limited_gh_error_maps_to_rate_limited_state() {
    let harness = Harness::new().await;
    harness.github.will_be_authenticated("github.com", "adriandemian");
    harness.github.will_fail_pr_detail("API rate limit exceeded");

    let detail = load_pull_request_detail(
        harness.deps(),
        PullRequestDetailRequest {
            project_id: harness.project_id.clone(),
            pr_number: Some(42),
            branch: None,
        },
    )
    .await;

    assert_eq!(detail.state, PullRequestDetailState::RateLimited);
}
