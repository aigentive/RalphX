//! PR-detail read model (Decision 4): one composition entry point returning the
//! full graph for a `(project, pr_number)` or `(project, branch)` selector —
//! description + checks/review summary + issue comments + live review thread +
//! attached RalphX conversations — reusing the existing `gh`/DB machinery.
//!
//! Contracts:
//! - **Never panics / never `Err`.** Every failure collapses into a typed
//!   [`PullRequestDetailState`]; per-section soft failures land in
//!   `sources_unavailable` while the rest of the graph still loads.
//! - **DB-first, live only on open.** Branch→PR resolution reads DB rows
//!   (`agent_conversation_workspaces` outbound `publication_pr_*` + inbound
//!   `source_pull_request`, `plan_branches.pr_*`); external branches fall back to
//!   a single live `find_latest_pr_by_head_branch`. The metrics `pr_facts_cte`
//!   is **never** invoked (it is a private dashboard string-builder).
//! - **Single comment writer.** Owned-published issue comments are refreshed
//!   ONLY through `import_agent_workspace_pr_comment_evidence`, then read back via
//!   `list_pr_comment_evidence` (raised limit — no 20-cap). Inbound/external PRs
//!   have no evidence rows, so their comments come live from `fetch_pr_health`.
//! - **Annotations are opt-in.** Initial open calls `fetch_pr_detail` (gate) then
//!   `fetch_pr_health` + `fetch_pr_review_thread` + `check_pr_review_feedback`
//!   concurrently; `fetch_pr_diff_annotations` is NOT called here. React-Query
//!   cancellation does not abort the Tauri future, so these fetches run to
//!   completion (each `gh` subprocess is `kill_on_drop` + 30s-capped).

pub mod types;

use std::path::Path;
use std::sync::Arc;

use crate::application::services::pr_merge_poller::import_agent_workspace_pr_comment_evidence;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentWorkspacePrCommentEvidence, ChatConversationId, ProjectId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, PlanBranchRepository, ProjectRepository,
};
use crate::domain::services::github_service::{
    GithubConnectionDiagnostic, GithubConnectionState, GithubConnectionStatus, GithubServiceTrait,
    PrDetail, PrHealth, PrHealthCheck, PrIssueCommentSummary, PrReviewFeedback,
    PrReviewThreadComment, PrStatus,
};
use crate::error::AppError;
use crate::utils::path_safety::validate_absolute_non_root_path;

use types::{
    PullRequestCheck, PullRequestDescription, PullRequestDetail, PullRequestDetailState,
    PullRequestIssueComment, PullRequestOrigin, PullRequestReviewFeedbackComment,
    PullRequestReviewSummary, PullRequestReviewThreadComment, PullRequestRxConversation,
};

/// `gh` subprocess error fragments (lowercased) used to classify failures into
/// the typed taxonomy. External-tool output is matched via named constants
/// rather than ad hoc string compares.
const GH_TIMEOUT_FRAGMENT: &str = "timed out";
const GH_RATE_LIMIT_FRAGMENT: &str = "rate limit";
const GH_NO_PR_FRAGMENTS: [&str; 3] = [
    "no pull request",
    "could not resolve to a pullrequest",
    "not found",
];

/// Generous issue-comment page for the detail view. Deliberately NOT the `20`
/// supervision cap used elsewhere — this read path is uncapped in intent while
/// still respecting bounded-collection hygiene.
const PR_DETAIL_COMMENT_LIMIT: usize = 500;

/// Borrowed dependencies for the PR-detail composition. The Tauri command builds
/// this from `AppState` (cheap `Arc` clones); tests build it from mocks.
pub struct PullRequestDetailDeps {
    pub github_service: Option<Arc<dyn GithubServiceTrait>>,
    pub project_repo: Arc<dyn ProjectRepository>,
    pub workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    pub plan_branch_repo: Arc<dyn PlanBranchRepository>,
}

/// Selector for the PR to load: by number or by branch (exactly one expected).
pub struct PullRequestDetailRequest {
    pub project_id: ProjectId,
    pub pr_number: Option<i64>,
    pub branch: Option<String>,
}

/// A PR resolved from the DB/live union, ready for live detail composition.
struct ResolvedPr {
    number: i64,
    /// Known head ref (from DB) or `None` when only a number was given (the
    /// `fetch_pr_detail` payload then supplies it).
    head_ref: Option<String>,
    origin: PullRequestOrigin,
    /// The owning conversation for an owned-published PR — gates the DB
    /// comment-evidence path (single writer); `None` for inbound/external.
    owning_conversation_id: Option<ChatConversationId>,
}

/// Compose the full PR-detail graph. Always returns a typed payload.
pub async fn load_pull_request_detail(
    deps: PullRequestDetailDeps,
    request: PullRequestDetailRequest,
) -> PullRequestDetail {
    // 1. Resolve project + working dir (Decision 5).
    let project = match deps.project_repo.get_by_id(&request.project_id).await {
        Ok(Some(project)) => project,
        _ => return PullRequestDetail::empty(PullRequestDetailState::RepoUnresolvable),
    };
    if project.working_directory.trim().is_empty() {
        return PullRequestDetail::empty(PullRequestDetailState::RepoUnresolvable);
    }
    let working_dir = match validate_absolute_non_root_path(
        Path::new(&project.working_directory),
        "project working directory",
    ) {
        Ok(path) => path,
        Err(_) => return PullRequestDetail::empty(PullRequestDetailState::RepoUnresolvable),
    };

    // 2. GitHub service must be present (guarded `Option<Arc<dyn>>`).
    let Some(github) = deps.github_service.clone() else {
        return PullRequestDetail::empty(PullRequestDetailState::RepoUnresolvable);
    };

    // 3. Auth gate — distinct gh-unauthenticated state (Decision 1 / P1).
    let status = github
        .fetch_github_connection_status()
        .await
        .unwrap_or_else(|_| {
            GithubConnectionStatus::probe_failed(GithubConnectionDiagnostic::ServiceFailure)
        });
    match status.state {
        GithubConnectionState::Authenticated => {}
        GithubConnectionState::Unauthenticated | GithubConnectionState::CredentialRejected => {
            return PullRequestDetail::empty(PullRequestDetailState::GhUnauthenticated);
        }
        GithubConnectionState::ProviderUnavailable | GithubConnectionState::ProbeFailed => {
            return PullRequestDetail::empty(PullRequestDetailState::FetchUnavailable);
        }
        GithubConnectionState::CliUnavailable => {
            return PullRequestDetail::empty(PullRequestDetailState::CliUnavailable);
        }
    }

    // 4. Resolve the PR holder (DB-first; live only for external branches).
    let Some(resolved) = resolve_pr(
        &deps,
        github.as_ref(),
        &working_dir,
        &request.project_id,
        request.pr_number,
        request.branch.as_deref(),
    )
    .await
    else {
        return PullRequestDetail::empty(PullRequestDetailState::NoPr);
    };

    // 5. Description fetch is the existence gate.
    let detail = match github.fetch_pr_detail(&working_dir, resolved.number).await {
        Ok(detail) => detail,
        Err(error) => return PullRequestDetail::empty(classify_gh_error(&error)),
    };
    let head_ref = resolved
        .head_ref
        .clone()
        .unwrap_or_else(|| detail.head_ref_name.clone());

    // 6. Best-effort concurrent fetches (annotations are NOT requested here).
    let (health_result, thread_result, feedback_result) = tokio::join!(
        github.fetch_pr_health(&working_dir, resolved.number),
        github.fetch_pr_review_thread(&working_dir, resolved.number),
        github.check_pr_review_feedback(&working_dir, resolved.number),
    );

    let mut sources_unavailable = Vec::new();

    let health = match health_result {
        Ok(health) => Some(health),
        Err(_) => {
            sources_unavailable.push("checks".to_string());
            None
        }
    };

    let review_thread = match thread_result {
        Ok(thread) => thread
            .comments
            .into_iter()
            .map(map_review_thread_comment)
            .collect(),
        Err(_) => {
            sources_unavailable.push("reviewThread".to_string());
            Vec::new()
        }
    };

    let checks = health
        .as_ref()
        .map(|health| health.checks.iter().map(map_check).collect())
        .unwrap_or_default();
    let review_decision = health
        .as_ref()
        .and_then(|health| health.review_decision.clone());

    let review_feedback = match feedback_result {
        Ok(feedback) => feedback,
        Err(_) => {
            sources_unavailable.push("reviewFeedback".to_string());
            None
        }
    };
    let review_summary = Some(build_review_summary(review_decision, review_feedback));

    // 7. Issue comments — owned-published via the single-writer evidence cache;
    //    inbound/external live from health.
    let issue_comments = build_issue_comments(&deps, &resolved, health.as_ref()).await;

    // 8. Attached RalphX conversations (workspaces whose branch == head ref).
    let rx_conversations = deps
        .workspace_repo
        .find_by_head_ref(&request.project_id, &head_ref)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(map_rx_conversation)
        .collect();

    PullRequestDetail {
        state: PullRequestDetailState::Loaded,
        origin: Some(resolved.origin),
        description: Some(map_description(&detail)),
        checks,
        review_summary,
        issue_comments,
        review_thread,
        rx_conversations,
        // Reverse branch→ticket linkage is deferred in v1 (forward ticket→PR
        // already ships via get_ticket_associations); field kept for graph shape.
        linked_tickets: Vec::new(),
        sources_unavailable,
    }
}

/// Resolve the PR holder for the selector, DB-first. Returns `None` (→ `NoPr`)
/// when neither a DB row nor a live external PR matches.
async fn resolve_pr(
    deps: &PullRequestDetailDeps,
    github: &dyn GithubServiceTrait,
    working_dir: &Path,
    project_id: &ProjectId,
    pr_number: Option<i64>,
    branch: Option<&str>,
) -> Option<ResolvedPr> {
    let workspaces = deps
        .workspace_repo
        .get_by_project_id(project_id)
        .await
        .unwrap_or_default();
    let plan_branches = deps
        .plan_branch_repo
        .get_by_project_id(project_id)
        .await
        .unwrap_or_default();

    if let Some(number) = pr_number {
        // Owned outbound publication PR.
        if let Some(workspace) = workspaces
            .iter()
            .find(|workspace| workspace.publication_pr_number == Some(number))
        {
            return Some(ResolvedPr {
                number,
                head_ref: Some(workspace.branch_name.clone()),
                origin: PullRequestOrigin::OwnedOutbound,
                owning_conversation_id: Some(workspace.conversation_id.clone()),
            });
        }
        // Owned inbound review PR.
        if let Some(workspace) = workspaces.iter().find(|workspace| {
            workspace
                .source_pull_request
                .as_ref()
                .is_some_and(|source| source.number == number)
        }) {
            let head_ref = workspace
                .source_pull_request
                .as_ref()
                .map(|source| source.head_ref_name.clone());
            return Some(ResolvedPr {
                number,
                head_ref,
                origin: PullRequestOrigin::OwnedInbound,
                owning_conversation_id: None,
            });
        }
        // Task-pipeline plan-branch PR.
        if let Some(plan_branch) = plan_branches
            .iter()
            .find(|branch| branch.pr_number == Some(number))
        {
            return Some(ResolvedPr {
                number,
                head_ref: Some(plan_branch.branch_name.clone()),
                origin: PullRequestOrigin::PlanBranch,
                owning_conversation_id: None,
            });
        }
        // External: we still have the number; head ref comes from fetch_pr_detail.
        return Some(ResolvedPr {
            number,
            head_ref: None,
            origin: PullRequestOrigin::External,
            owning_conversation_id: None,
        });
    }

    let branch = branch?;

    // Owned outbound: workspace on this branch with a publication PR.
    if let Some((workspace, number)) = workspaces.iter().find_map(|workspace| {
        workspace
            .publication_pr_number
            .filter(|_| workspace.branch_name == branch)
            .map(|number| (workspace, number))
    }) {
        return Some(ResolvedPr {
            number,
            head_ref: Some(branch.to_string()),
            origin: PullRequestOrigin::OwnedOutbound,
            owning_conversation_id: Some(workspace.conversation_id.clone()),
        });
    }
    // Owned inbound: workspace whose source PR head ref == branch.
    if let Some(number) = workspaces.iter().find_map(|workspace| {
        workspace
            .source_pull_request
            .as_ref()
            .filter(|source| source.head_ref_name == branch)
            .map(|source| source.number)
    }) {
        return Some(ResolvedPr {
            number,
            head_ref: Some(branch.to_string()),
            origin: PullRequestOrigin::OwnedInbound,
            owning_conversation_id: None,
        });
    }
    // Plan-branch on this branch with a PR.
    if let Some(number) = plan_branches.iter().find_map(|plan_branch| {
        plan_branch
            .pr_number
            .filter(|_| plan_branch.branch_name == branch)
    }) {
        return Some(ResolvedPr {
            number,
            head_ref: Some(branch.to_string()),
            origin: PullRequestOrigin::PlanBranch,
            owning_conversation_id: None,
        });
    }
    // External: a single live head-branch lookup.
    if let Ok(Some(found)) = github
        .find_latest_pr_by_head_branch(working_dir, branch)
        .await
    {
        return Some(ResolvedPr {
            number: found.number,
            head_ref: Some(branch.to_string()),
            origin: PullRequestOrigin::External,
            owning_conversation_id: None,
        });
    }

    None
}

/// Owned-published PRs refresh + read DB evidence (single writer); inbound and
/// external PRs surface live issue comments from `fetch_pr_health`.
async fn build_issue_comments(
    deps: &PullRequestDetailDeps,
    resolved: &ResolvedPr,
    health: Option<&PrHealth>,
) -> Vec<PullRequestIssueComment> {
    if let Some(conversation_id) = &resolved.owning_conversation_id {
        // Refresh ONLY through the single writer, then read back uncapped.
        if let Some(health) = health {
            let _ = import_agent_workspace_pr_comment_evidence(
                Arc::clone(&deps.workspace_repo),
                conversation_id,
                resolved.number,
                health,
            )
            .await;
        }
        deps.workspace_repo
            .list_pr_comment_evidence(conversation_id, resolved.number, PR_DETAIL_COMMENT_LIMIT)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(map_evidence_comment)
            .collect()
    } else {
        health
            .map(|health| health.issue_comments.iter().map(map_live_comment).collect())
            .unwrap_or_default()
    }
}

/// Map a `gh` subprocess error into the typed taxonomy. Unknown fetch failures
/// collapse to `FetchTimeout` (the "fetch failed or timed out" bucket, PO7).
fn classify_gh_error(error: &AppError) -> PullRequestDetailState {
    let text = error.to_string().to_ascii_lowercase();
    if text.contains(GH_TIMEOUT_FRAGMENT) {
        return PullRequestDetailState::FetchTimeout;
    }
    if text.contains(GH_RATE_LIMIT_FRAGMENT) {
        return PullRequestDetailState::RateLimited;
    }
    if GH_NO_PR_FRAGMENTS
        .iter()
        .any(|fragment| text.contains(fragment))
    {
        return PullRequestDetailState::NoPr;
    }
    PullRequestDetailState::FetchTimeout
}

fn pr_state_label(status: &PrStatus, is_draft: bool) -> String {
    match status {
        PrStatus::Merged { .. } => "merged",
        PrStatus::Closed => "closed",
        PrStatus::Open if is_draft => "draft",
        PrStatus::Open => "open",
    }
    .to_string()
}

fn map_description(detail: &PrDetail) -> PullRequestDescription {
    PullRequestDescription {
        number: detail.number,
        title: detail.title.clone(),
        body: detail.body.clone(),
        author: detail.author.clone(),
        created_at: detail.created_at.clone(),
        url: detail.url.clone(),
        state: pr_state_label(&detail.state, detail.is_draft),
        is_draft: detail.is_draft,
        head_ref_name: detail.head_ref_name.clone(),
        base_ref_name: detail.base_ref_name.clone(),
    }
}

fn map_check(check: &PrHealthCheck) -> PullRequestCheck {
    PullRequestCheck {
        name: check.name.clone(),
        status: check.status.clone(),
        conclusion: check.conclusion.clone(),
        details_url: check.details_url.clone(),
    }
}

fn build_review_summary(
    review_decision: Option<String>,
    feedback: Option<PrReviewFeedback>,
) -> PullRequestReviewSummary {
    match feedback {
        Some(feedback) => PullRequestReviewSummary {
            review_decision,
            latest_changes_requested_author: Some(feedback.author),
            latest_changes_requested_body: feedback.body,
            latest_changes_requested_submitted_at: feedback.submitted_at,
            latest_changes_requested_comments: feedback
                .comments
                .into_iter()
                .map(|comment| PullRequestReviewFeedbackComment {
                    id: comment.id,
                    author: comment.author,
                    path: comment.path,
                    line: comment.line,
                    body: comment.body,
                })
                .collect(),
        },
        None => PullRequestReviewSummary {
            review_decision,
            latest_changes_requested_author: None,
            latest_changes_requested_body: None,
            latest_changes_requested_submitted_at: None,
            latest_changes_requested_comments: Vec::new(),
        },
    }
}

fn map_review_thread_comment(comment: PrReviewThreadComment) -> PullRequestReviewThreadComment {
    PullRequestReviewThreadComment {
        id: comment.id,
        author: comment.author,
        body: comment.body,
        path: comment.path,
        side: comment.side,
        line: comment.line,
        url: comment.url,
        created_at: comment.created_at,
        in_reply_to_id: comment.in_reply_to_id,
        is_outdated: comment.is_outdated,
    }
}

fn map_evidence_comment(evidence: AgentWorkspacePrCommentEvidence) -> PullRequestIssueComment {
    PullRequestIssueComment {
        id: evidence.comment_id,
        author: evidence.author,
        body: evidence.body,
        url: evidence.url,
        created_at: evidence.github_created_at,
        updated_at: evidence.github_updated_at,
        is_bot: evidence.is_bot,
        is_codecov: evidence.is_codecov,
        source: "evidence".to_string(),
    }
}

fn map_live_comment(comment: &PrIssueCommentSummary) -> PullRequestIssueComment {
    PullRequestIssueComment {
        id: comment.id.clone(),
        author: comment.author.clone(),
        body: comment.body.clone(),
        url: comment.url.clone(),
        created_at: comment.created_at.clone(),
        updated_at: comment.updated_at.clone(),
        is_bot: comment.is_bot,
        is_codecov: comment.is_codecov,
        source: "live".to_string(),
    }
}

fn map_rx_conversation(workspace: AgentConversationWorkspace) -> PullRequestRxConversation {
    PullRequestRxConversation {
        conversation_id: workspace.conversation_id.to_string(),
        branch_name: workspace.branch_name,
        linked_ideation_session_id: workspace
            .linked_ideation_session_id
            .map(|id| id.to_string()),
        publication_pr_number: workspace.publication_pr_number,
        publication_pr_status: workspace.publication_pr_status,
    }
}
