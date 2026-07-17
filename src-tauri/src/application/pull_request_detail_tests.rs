//! Tests for the PR-detail composition. GitHub access is faked via
//! `MockGithubService`; project/workspace/plan-branch reads use in-memory repos.
//! Never touches a real `gh`.

use std::sync::Arc;

use crate::application::pull_request_detail::types::{PullRequestDetailState, PullRequestOrigin};
use crate::application::pull_request_detail::{
    load_pull_request_detail, PullRequestDetailDeps, PullRequestDetailRequest,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentWorkspaceSourcePullRequest,
    ArtifactId, ChatConversationId, IdeationAnalysisBaseRefKind, IdeationSessionId, PlanBranch,
    Project, ProjectId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, PlanBranchRepository, ProjectRepository,
};
use crate::domain::services::github_service::{
    GithubServiceTrait, PrBranchMatch, PrDetail, PrHealth, PrHealthCheck, PrIssueCommentSummary,
    PrMergeStateStatus, PrMergeableState, PrReviewCommentFeedback, PrReviewFeedback,
    PrReviewThread, PrReviewThreadComment, PrStatus, PrSyncState,
};
use crate::error::AppError;
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
        Self::new_with_working_dir(WORKING_DIR).await
    }

    async fn new_with_working_dir(working_dir: &str) -> Self {
        let github = Arc::new(MockGithubService::new());
        let project_repo: Arc<dyn ProjectRepository> = Arc::new(MemoryProjectRepository::new());
        let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
            Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let plan_branch_repo: Arc<dyn PlanBranchRepository> =
            Arc::new(MemoryPlanBranchRepository::new());

        let project = project_repo
            .create(Project::new(
                "PR Detail".to_string(),
                working_dir.to_string(),
            ))
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

    fn deps_without_github(&self) -> PullRequestDetailDeps {
        PullRequestDetailDeps {
            github_service: None,
            project_repo: Arc::clone(&self.project_repo),
            workspace_repo: Arc::clone(&self.workspace_repo),
            plan_branch_repo: Arc::clone(&self.plan_branch_repo),
        }
    }

    async fn seed_workspace(
        &self,
        conversation_id: &str,
        branch: &str,
        publication_pr: Option<i64>,
    ) {
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
        self.workspace_repo
            .create_or_update(workspace)
            .await
            .unwrap();
    }

    async fn seed_source_workspace(&self, conversation_id: &str, branch: &str, pr_number: i64) {
        let mut workspace = AgentConversationWorkspace::new(
            ChatConversationId::from_string(conversation_id),
            self.project_id.clone(),
            AgentConversationWorkspaceMode::ReviewPr,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            None,
            None,
            branch.to_string(),
            format!("/tmp/ralphx/{conversation_id}"),
        );
        workspace.source_pull_request = Some(AgentWorkspaceSourcePullRequest {
            number: pr_number,
            url: Some(format!("https://github.com/owner/repo/pull/{pr_number}")),
            title: Some(format!("Review PR #{pr_number}")),
            head_ref_name: branch.to_string(),
            base_ref_name: Some("main".to_string()),
            head_ref_oid: Some("abc123".to_string()),
        });
        self.workspace_repo
            .create_or_update(workspace)
            .await
            .unwrap();
    }

    async fn seed_plan_branch(&self, branch: &str, pr_number: i64) {
        let mut plan_branch = PlanBranch::new(
            ArtifactId::from_string(format!("artifact-{branch}")),
            IdeationSessionId::from_string(format!("session-{branch}")),
            self.project_id.clone(),
            branch.to_string(),
            "main".to_string(),
        );
        plan_branch.pr_number = Some(pr_number);
        plan_branch.pr_url = Some(format!("https://github.com/owner/repo/pull/{pr_number}"));
        self.plan_branch_repo.create(plan_branch).await.unwrap();
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
    pr_detail_with_state(number, head_ref, PrStatus::Open, false)
}

fn pr_detail_with_state(number: i64, head_ref: &str, state: PrStatus, is_draft: bool) -> PrDetail {
    PrDetail {
        number,
        title: "Add PR visibility".to_string(),
        body: Some("Body".to_string()),
        author: Some("adriandemian".to_string()),
        created_at: Some("2026-06-24T08:00:00Z".to_string()),
        url: Some(format!("https://github.com/owner/repo/pull/{number}")),
        state,
        is_draft,
        head_ref_name: head_ref.to_string(),
        base_ref_name: "main".to_string(),
    }
}

#[tokio::test]
async fn unauthenticated_short_circuits_before_any_pr_fetch() {
    let harness = Harness::new().await;
    harness.github.will_return_connection_status(
        crate::domain::services::github_service::GithubConnectionStatus::unauthenticated(),
    );

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
async fn provider_unavailable_never_collapses_to_gh_unauthenticated() {
    let harness = Harness::new().await;
    harness.github.will_return_connection_status(
        crate::domain::services::github_service::GithubConnectionStatus::provider_unavailable(
            crate::domain::services::github_service::GithubConnectionDiagnostic::Http5xx,
        ),
    );

    let detail = load_pull_request_detail(
        harness.deps(),
        PullRequestDetailRequest {
            project_id: harness.project_id.clone(),
            pr_number: Some(7),
            branch: None,
        },
    )
    .await;

    assert_eq!(detail.state, PullRequestDetailState::FetchUnavailable);
    assert_ne!(detail.state, PullRequestDetailState::GhUnauthenticated);
    assert_eq!(harness.github.state().fetch_pr_detail_calls, 0);
}

#[tokio::test]
async fn cli_unavailable_uses_cli_unavailable_state_instead_of_repo_unresolvable() {
    let harness = Harness::new().await;
    harness.github.will_return_connection_status(
        crate::domain::services::github_service::GithubConnectionStatus::cli_unavailable(),
    );

    let detail = load_pull_request_detail(
        harness.deps(),
        PullRequestDetailRequest {
            project_id: harness.project_id.clone(),
            pr_number: Some(7),
            branch: None,
        },
    )
    .await;

    assert_eq!(detail.state, PullRequestDetailState::CliUnavailable);
    assert_ne!(detail.state, PullRequestDetailState::RepoUnresolvable);
    assert_eq!(harness.github.state().fetch_pr_detail_calls, 0);
}

#[tokio::test]
async fn rejected_credentials_still_require_credential_repair() {
    let harness = Harness::new().await;
    harness.github.will_return_connection_status(
        crate::domain::services::github_service::GithubConnectionStatus::credential_rejected(),
    );

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
    assert_eq!(harness.github.state().fetch_pr_detail_calls, 0);
}

#[tokio::test]
async fn repo_unresolvable_when_project_missing() {
    let harness = Harness::new().await;
    harness
        .github
        .will_be_authenticated("github.com", "adriandemian");

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
async fn repo_unresolvable_when_project_working_dir_is_unsafe() {
    let harness = Harness::new_with_working_dir("../relative").await;
    harness
        .github
        .will_be_authenticated("github.com", "adriandemian");

    let detail = load_pull_request_detail(
        harness.deps(),
        PullRequestDetailRequest {
            project_id: harness.project_id.clone(),
            pr_number: Some(7),
            branch: None,
        },
    )
    .await;

    assert_eq!(detail.state, PullRequestDetailState::RepoUnresolvable);
    assert_eq!(
        harness.github.state().fetch_github_connection_status_calls,
        0
    );
}

#[tokio::test]
async fn repo_unresolvable_when_github_service_missing() {
    let harness = Harness::new().await;

    let detail = load_pull_request_detail(
        harness.deps_without_github(),
        PullRequestDetailRequest {
            project_id: harness.project_id.clone(),
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
    harness
        .github
        .will_be_authenticated("github.com", "adriandemian");
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
    harness
        .github
        .will_be_authenticated("github.com", "adriandemian");
    harness
        .seed_workspace("11111111-1111-1111-1111-111111111111", "feature/x", Some(7))
        .await;
    harness
        .github
        .will_return_pr_detail(pr_detail(7, "feature/x"));
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
async fn owned_inbound_pr_number_uses_source_pull_request_with_live_comments() {
    let harness = Harness::new().await;
    harness
        .github
        .will_be_authenticated("github.com", "adriandemian");
    harness
        .seed_source_workspace("33333333-3333-3333-3333-333333333333", "review/source", 44)
        .await;
    harness
        .github
        .will_return_pr_detail(pr_detail(44, "review/source"));
    harness.github.state().fetch_pr_health_result = Some(Ok(health(
        Vec::new(),
        vec![issue_comment("c44", "review note")],
    )));

    let detail = load_pull_request_detail(
        harness.deps(),
        PullRequestDetailRequest {
            project_id: harness.project_id.clone(),
            pr_number: Some(44),
            branch: None,
        },
    )
    .await;

    assert_eq!(detail.state, PullRequestDetailState::Loaded);
    assert_eq!(detail.origin, Some(PullRequestOrigin::OwnedInbound));
    assert_eq!(detail.issue_comments.len(), 1);
    assert_eq!(detail.issue_comments[0].source, "live");
    assert_eq!(detail.rx_conversations[0].branch_name, "review/source");
}

#[tokio::test]
async fn plan_branch_pr_number_uses_plan_origin() {
    let harness = Harness::new().await;
    harness
        .github
        .will_be_authenticated("github.com", "adriandemian");
    harness.seed_plan_branch("plan/branch", 45).await;
    harness
        .github
        .will_return_pr_detail(pr_detail(45, "plan/branch"));

    let detail = load_pull_request_detail(
        harness.deps(),
        PullRequestDetailRequest {
            project_id: harness.project_id.clone(),
            pr_number: Some(45),
            branch: None,
        },
    )
    .await;

    assert_eq!(detail.state, PullRequestDetailState::Loaded);
    assert_eq!(detail.origin, Some(PullRequestOrigin::PlanBranch));
    assert_eq!(
        detail.description.as_ref().unwrap().head_ref_name,
        "plan/branch"
    );
}

#[tokio::test]
async fn branch_resolution_prefers_inbound_then_plan_branch_before_live_lookup() {
    let inbound = Harness::new().await;
    inbound
        .github
        .will_be_authenticated("github.com", "adriandemian");
    inbound
        .seed_source_workspace("44444444-4444-4444-4444-444444444444", "review/head", 46)
        .await;
    inbound
        .github
        .will_return_pr_detail(pr_detail(46, "review/head"));

    let inbound_detail = load_pull_request_detail(
        inbound.deps(),
        PullRequestDetailRequest {
            project_id: inbound.project_id.clone(),
            pr_number: None,
            branch: Some("review/head".to_string()),
        },
    )
    .await;

    assert_eq!(inbound_detail.origin, Some(PullRequestOrigin::OwnedInbound));
    assert_eq!(
        inbound.github.state().find_latest_pr_by_head_branch_calls,
        0
    );

    let plan = Harness::new().await;
    plan.github
        .will_be_authenticated("github.com", "adriandemian");
    plan.seed_plan_branch("plan/head", 47).await;
    plan.github
        .will_return_pr_detail(pr_detail(47, "plan/head"));

    let plan_detail = load_pull_request_detail(
        plan.deps(),
        PullRequestDetailRequest {
            project_id: plan.project_id.clone(),
            pr_number: None,
            branch: Some("plan/head".to_string()),
        },
    )
    .await;

    assert_eq!(plan_detail.origin, Some(PullRequestOrigin::PlanBranch));
    assert_eq!(plan.github.state().find_latest_pr_by_head_branch_calls, 0);
}

#[tokio::test]
async fn external_branch_loads_via_live_lookup_with_live_comments() {
    let harness = Harness::new().await;
    harness
        .github
        .will_be_authenticated("github.com", "adriandemian");
    harness
        .github
        .set_find_latest_pr_by_head_branch(Ok(Some(PrBranchMatch {
            number: 99,
            url: "https://github.com/owner/repo/pull/99".to_string(),
            status: PrStatus::Open,
            is_draft: false,
            head_ref_name: "external/feature".to_string(),
            updated_at: None,
            author_login: None,
        })));
    harness
        .github
        .will_return_pr_detail(pr_detail(99, "external/feature"));
    harness.github.state().fetch_pr_health_result = Some(Ok(health(
        Vec::new(),
        vec![issue_comment("c9", "external note")],
    )));

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
async fn best_effort_source_failures_are_reported_without_failing_description() {
    let harness = Harness::new().await;
    harness
        .github
        .will_be_authenticated("github.com", "adriandemian");
    harness
        .seed_workspace(
            "55555555-5555-5555-5555-555555555555",
            "feature/soft",
            Some(55),
        )
        .await;
    harness
        .github
        .will_return_pr_detail(pr_detail(55, "feature/soft"));
    harness.github.state().fetch_pr_health_result = Some(Err(AppError::Infrastructure(
        "checks unavailable".to_string(),
    )));
    harness
        .github
        .will_fail_pr_review_thread("review thread unavailable");
    harness.github.state().check_pr_review_feedback_result = Some(Err(AppError::Infrastructure(
        "feedback unavailable".to_string(),
    )));

    let detail = load_pull_request_detail(
        harness.deps(),
        PullRequestDetailRequest {
            project_id: harness.project_id.clone(),
            pr_number: Some(55),
            branch: None,
        },
    )
    .await;

    assert_eq!(detail.state, PullRequestDetailState::Loaded);
    assert_eq!(detail.description.as_ref().unwrap().number, 55);
    assert_eq!(
        detail.sources_unavailable,
        vec![
            "checks".to_string(),
            "reviewThread".to_string(),
            "reviewFeedback".to_string()
        ]
    );
    assert!(detail.checks.is_empty());
    assert!(detail.review_thread.is_empty());
}

#[tokio::test]
async fn review_feedback_and_thread_comments_are_mapped_into_detail() {
    let harness = Harness::new().await;
    harness
        .github
        .will_be_authenticated("github.com", "adriandemian");
    harness
        .seed_workspace(
            "66666666-6666-6666-6666-666666666666",
            "feature/review",
            Some(66),
        )
        .await;
    harness.github.will_return_pr_detail(pr_detail_with_state(
        66,
        "feature/review",
        PrStatus::Closed,
        false,
    ));
    harness.github.state().fetch_pr_health_result = Some(Ok(health(Vec::new(), Vec::new())));
    harness.github.will_return_pr_review_thread(PrReviewThread {
        pr_number: 66,
        comments: vec![PrReviewThreadComment {
            id: "thread-1".to_string(),
            author: Some("reviewer".to_string()),
            body: "Inline note".to_string(),
            path: Some("src/lib.rs".to_string()),
            side: Some("RIGHT".to_string()),
            line: Some(12),
            url: Some("https://github.com/owner/repo/pull/66#discussion_r1".to_string()),
            created_at: Some("2026-06-24T10:00:00Z".to_string()),
            in_reply_to_id: Some("parent-1".to_string()),
            is_outdated: true,
        }],
    });
    harness.github.state().check_pr_review_feedback_result = Some(Ok(Some(PrReviewFeedback {
        review_id: "review-1".to_string(),
        author: "reviewer".to_string(),
        submitted_at: Some("2026-06-24T10:05:00Z".to_string()),
        body: Some("Please change this".to_string()),
        comments: vec![PrReviewCommentFeedback {
            id: "feedback-1".to_string(),
            author: "reviewer".to_string(),
            path: Some("src/lib.rs".to_string()),
            line: Some(13),
            body: "Fix this line".to_string(),
        }],
    })));

    let detail = load_pull_request_detail(
        harness.deps(),
        PullRequestDetailRequest {
            project_id: harness.project_id.clone(),
            pr_number: Some(66),
            branch: None,
        },
    )
    .await;

    assert_eq!(detail.description.as_ref().unwrap().state, "closed");
    assert_eq!(detail.review_thread.len(), 1);
    assert_eq!(
        detail.review_thread[0].in_reply_to_id.as_deref(),
        Some("parent-1")
    );
    assert!(detail.review_thread[0].is_outdated);
    let summary = detail
        .review_summary
        .expect("review summary should be present");
    assert_eq!(
        summary.latest_changes_requested_author.as_deref(),
        Some("reviewer")
    );
    assert_eq!(summary.latest_changes_requested_comments.len(), 1);
    assert_eq!(summary.latest_changes_requested_comments[0].line, Some(13));
}

#[tokio::test]
async fn status_labels_include_draft_and_merged() {
    let draft = Harness::new().await;
    draft
        .github
        .will_be_authenticated("github.com", "adriandemian");
    draft
        .seed_workspace(
            "77777777-7777-7777-7777-777777777777",
            "feature/draft",
            Some(77),
        )
        .await;
    draft.github.will_return_pr_detail(pr_detail_with_state(
        77,
        "feature/draft",
        PrStatus::Open,
        true,
    ));

    let draft_detail = load_pull_request_detail(
        draft.deps(),
        PullRequestDetailRequest {
            project_id: draft.project_id.clone(),
            pr_number: Some(77),
            branch: None,
        },
    )
    .await;

    assert_eq!(draft_detail.description.as_ref().unwrap().state, "draft");

    let merged = Harness::new().await;
    merged
        .github
        .will_be_authenticated("github.com", "adriandemian");
    merged
        .seed_workspace(
            "88888888-8888-8888-8888-888888888888",
            "feature/merged",
            Some(88),
        )
        .await;
    merged.github.will_return_pr_detail(pr_detail_with_state(
        88,
        "feature/merged",
        PrStatus::Merged {
            merge_commit_sha: Some("abc123".to_string()),
            merged_at: None,
        },
        false,
    ));

    let merged_detail = load_pull_request_detail(
        merged.deps(),
        PullRequestDetailRequest {
            project_id: merged.project_id.clone(),
            pr_number: Some(88),
            branch: None,
        },
    )
    .await;

    assert_eq!(merged_detail.description.as_ref().unwrap().state, "merged");
}

#[tokio::test]
async fn fetch_pr_detail_timeout_maps_to_fetch_timeout_state() {
    let harness = Harness::new().await;
    harness
        .github
        .will_be_authenticated("github.com", "adriandemian");
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
    harness
        .github
        .will_be_authenticated("github.com", "adriandemian");
    harness
        .github
        .will_fail_pr_detail("API rate limit exceeded");

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
