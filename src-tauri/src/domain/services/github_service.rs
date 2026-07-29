// GitHub service abstraction (AD2)
//
// Trait-based design allows production `gh` CLI implementation and mock for tests.
// All methods take `working_dir: &Path` for stateless, multi-project support (AD7).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::{AppError, AppResult};

pub const EMPTY_PR_METADATA_PATCH_VALIDATION_ERROR: &str =
    "pull request metadata patch requires a title or body";

/// Reject a metadata patch that omits both mutable fields.
pub fn validate_pr_metadata_patch(title: Option<&str>, body_file: Option<&Path>) -> AppResult<()> {
    if title.is_none() && body_file.is_none() {
        return Err(AppError::Validation(
            EMPTY_PR_METADATA_PATCH_VALIDATION_ERROR.to_string(),
        ));
    }

    Ok(())
}

/// Status of a GitHub pull request
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrStatus {
    Open,
    Closed,
    Merged {
        /// SHA of the merge commit, if available
        merge_commit_sha: Option<String>,
        /// Timestamp when GitHub marked the pull request merged, if available
        merged_at: Option<String>,
    },
}

/// GitHub's merge-state status for an open pull request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrMergeStateStatus {
    Clean,
    Behind,
    Dirty,
    Blocked,
    Draft,
    Unknown,
    Unstable,
    HasHooks,
    Other(String),
}

/// GitHub's coarse mergeability value for a pull request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrMergeableState {
    Mergeable,
    Conflicting,
    Unknown,
    Other(String),
}

/// Rich GitHub pull-request state used by PR-mode freshness reconciliation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrSyncState {
    pub status: PrStatus,
    pub merge_state_status: Option<PrMergeStateStatus>,
    pub mergeable: Option<PrMergeableState>,
    pub is_draft: bool,
    pub head_ref_name: String,
    pub base_ref_name: String,
    pub head_ref_oid: Option<String>,
    pub base_ref_oid: Option<String>,
}

/// GitHub auto-merge request state for a pull request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrAutoMergeRequest {
    pub enabled_by: Option<String>,
    pub merge_method: Option<String>,
}

/// Lightweight status-check summary used by PR supervision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrHealthCheck {
    pub name: String,
    pub status: Option<String>,
    pub conclusion: Option<String>,
    pub details_url: Option<String>,
}

/// PR issue comment summary used for bot-only feedback such as Codecov.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrIssueCommentSummary {
    pub id: String,
    pub author: Option<String>,
    pub body: String,
    pub url: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub is_bot: bool,
    pub is_codecov: bool,
}

/// Cheap PR health payload for background supervision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrHealth {
    pub sync_state: PrSyncState,
    pub review_decision: Option<String>,
    pub checks: Vec<PrHealthCheck>,
    pub issue_comments: Vec<PrIssueCommentSummary>,
    pub auto_merge_request: Option<PrAutoMergeRequest>,
}

/// Pull request found by an exact head-branch lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrBranchMatch {
    pub number: i64,
    pub url: String,
    pub status: PrStatus,
    pub is_draft: bool,
    pub head_ref_name: String,
    pub updated_at: Option<String>,
    pub author_login: Option<String>,
}

impl PrBranchMatch {
    pub fn publication_status(&self) -> &'static str {
        match self.status {
            PrStatus::Open if self.is_draft => "draft",
            PrStatus::Open => "open",
            PrStatus::Closed => "closed",
            PrStatus::Merged { .. } => "merged",
        }
    }
}

/// Pull request search result used by branch/base picker UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrSearchResult {
    pub number: i64,
    pub title: String,
    pub url: String,
    pub head_ref_name: String,
    pub head_ref_oid: Option<String>,
    pub base_ref_name: String,
    pub is_draft: bool,
    pub updated_at: Option<String>,
    pub author_login: Option<String>,
    #[serde(default)]
    pub assignee_logins: Vec<String>,
    pub review_decision: Option<String>,
    #[serde(default)]
    pub latest_review_author_logins: Vec<String>,
    #[serde(default)]
    pub review_request_logins: Vec<String>,
    pub is_cross_repository: bool,
}

/// Inline review comment attached to a GitHub pull request review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrReviewCommentFeedback {
    pub id: String,
    pub author: String,
    pub path: Option<String>,
    pub line: Option<i64>,
    pub body: String,
}

/// Actionable GitHub review feedback that should re-enter RalphX revision flow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrReviewFeedback {
    pub review_id: String,
    pub author: String,
    pub submitted_at: Option<String>,
    pub body: Option<String>,
    pub comments: Vec<PrReviewCommentFeedback>,
}

/// Normalized GitHub PR annotation that can be rendered against RalphX diffs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrDiffAnnotation {
    pub id: String,
    pub source: String,
    pub path: Option<String>,
    pub side: Option<String>,
    pub start_line: Option<i64>,
    pub end_line: Option<i64>,
    pub start_column: Option<i64>,
    pub end_column: Option<i64>,
    pub level: String,
    pub status: Option<String>,
    pub title: Option<String>,
    pub message: String,
    pub author: Option<String>,
    pub check_name: Option<String>,
    pub url: Option<String>,
    pub is_outdated: bool,
    pub created_at: Option<String>,
}

/// A source that could not be synced without failing the whole annotation payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrAnnotationSourceUnavailable {
    pub source: String,
    pub reason: String,
}

/// Live PR annotation payload for a published agent workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrDiffAnnotations {
    pub pr_number: i64,
    pub head_sha: Option<String>,
    pub annotations: Vec<PrDiffAnnotation>,
    pub sources_unavailable: Vec<PrAnnotationSourceUnavailable>,
}

impl PrDiffAnnotations {
    pub fn empty(pr_number: i64) -> Self {
        Self {
            pr_number,
            head_sha: None,
            annotations: Vec::new(),
            sources_unavailable: Vec::new(),
        }
    }
}

/// Summary-level GitHub pull request review event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrReviewSubmissionEvent {
    Approve,
    RequestChanges,
    Comment,
}

impl std::fmt::Display for PrReviewSubmissionEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Approve => write!(f, "APPROVE"),
            Self::RequestChanges => write!(f, "REQUEST_CHANGES"),
            Self::Comment => write!(f, "COMMENT"),
        }
    }
}

/// Result from submitting a GitHub pull request review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrSubmittedReview {
    pub id: String,
    pub url: Option<String>,
}

/// Full PR description payload for the read-only detail view.
///
/// Sourced from `gh pr view <n> --json title,body,author,createdAt,url,state,isDraft,headRefName,baseRefName`.
/// Supplies the Description section (title/body) that lighter PR reads do not fetch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrDetail {
    pub number: i64,
    pub title: String,
    pub body: Option<String>,
    pub author: Option<String>,
    pub created_at: Option<String>,
    pub url: Option<String>,
    pub state: PrStatus,
    pub is_draft: bool,
    pub head_ref_name: String,
    pub base_ref_name: String,
}

/// A single inline review-thread comment attached to a PR diff line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrReviewThreadComment {
    pub id: String,
    pub author: Option<String>,
    pub body: String,
    pub path: Option<String>,
    pub side: Option<String>,
    pub line: Option<i64>,
    pub url: Option<String>,
    pub created_at: Option<String>,
    /// Parent review-comment id when this is a reply (enables threading).
    pub in_reply_to_id: Option<String>,
    /// The anchored line no longer maps to the current head (outdated comment).
    pub is_outdated: bool,
}

/// Live, transient inline review-thread payload for a PR.
///
/// Factored out of [`GithubServiceTrait::fetch_pr_diff_annotations`] so the
/// detail view can render the review conversation without the heavier
/// check-run / code-scanning annotation fan-out (opt-in only). Not persisted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrReviewThread {
    pub pr_number: i64,
    pub comments: Vec<PrReviewThreadComment>,
}

impl PrReviewThread {
    pub fn empty(pr_number: i64) -> Self {
        Self {
            pr_number,
            comments: Vec::new(),
        }
    }
}

/// Canonical GitHub connection state observed through the locally-installed `gh` CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GithubConnectionState {
    Authenticated,
    Unauthenticated,
    CredentialRejected,
    ProviderUnavailable,
    CliUnavailable,
    ProbeFailed,
}

/// Redacted reason category for a non-healthy GitHub connection observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GithubConnectionDiagnostic {
    MissingCredentials,
    CredentialsRejected,
    Http5xx,
    Network,
    Timeout,
    CliLaunch,
    MalformedResponse,
    UnexpectedResponse,
    ServiceFailure,
}

/// Read-only reflection of local credential presence and live GitHub validation.
///
/// RalphX stores no GitHub token. `state` is authoritative; the booleans remain
/// derived compatibility fields while callers migrate to the tagged contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GithubConnectionStatus {
    pub state: GithubConnectionState,
    pub diagnostic: Option<GithubConnectionDiagnostic>,
    /// Whether the `gh` binary is installed (resolvable + spawnable).
    pub gh_installed: bool,
    /// Whether GitHub accepted the locally-present credential during this probe.
    pub authenticated: bool,
    /// The authenticated host (e.g. `github.com`), if any.
    pub host: Option<String>,
    /// The active authenticated account login, if any.
    pub account: Option<String>,
}

impl GithubConnectionStatus {
    pub fn authenticated(host: impl Into<String>, account: impl Into<String>) -> Self {
        Self {
            state: GithubConnectionState::Authenticated,
            diagnostic: None,
            gh_installed: true,
            authenticated: true,
            host: Some(host.into()),
            account: Some(account.into()),
        }
    }

    pub fn unauthenticated() -> Self {
        Self {
            state: GithubConnectionState::Unauthenticated,
            diagnostic: Some(GithubConnectionDiagnostic::MissingCredentials),
            gh_installed: true,
            authenticated: false,
            host: None,
            account: None,
        }
    }

    pub fn credential_rejected() -> Self {
        Self {
            state: GithubConnectionState::CredentialRejected,
            diagnostic: Some(GithubConnectionDiagnostic::CredentialsRejected),
            gh_installed: true,
            authenticated: false,
            host: Some("github.com".to_string()),
            account: None,
        }
    }

    pub fn provider_unavailable(diagnostic: GithubConnectionDiagnostic) -> Self {
        Self {
            state: GithubConnectionState::ProviderUnavailable,
            diagnostic: Some(diagnostic),
            gh_installed: true,
            authenticated: false,
            host: Some("github.com".to_string()),
            account: None,
        }
    }

    pub fn cli_unavailable() -> Self {
        Self {
            state: GithubConnectionState::CliUnavailable,
            diagnostic: Some(GithubConnectionDiagnostic::CliLaunch),
            gh_installed: false,
            authenticated: false,
            host: None,
            account: None,
        }
    }

    pub fn probe_failed(diagnostic: GithubConnectionDiagnostic) -> Self {
        Self {
            state: GithubConnectionState::ProbeFailed,
            diagnostic: Some(diagnostic),
            gh_installed: true,
            authenticated: false,
            host: Some("github.com".to_string()),
            account: None,
        }
    }

    pub fn requires_credential_repair(&self) -> bool {
        matches!(
            self.state,
            GithubConnectionState::Unauthenticated | GithubConnectionState::CredentialRejected
        )
    }

    pub fn has_local_credential(&self) -> bool {
        matches!(
            self.state,
            GithubConnectionState::Authenticated
                | GithubConnectionState::CredentialRejected
                | GithubConnectionState::ProviderUnavailable
        )
    }

    /// Compatibility constructor for a missing/unlaunchable GitHub CLI.
    pub fn unavailable() -> Self {
        Self::cli_unavailable()
    }
}

/// Abstraction over GitHub operations (production: `gh` CLI, tests: mock)
#[async_trait]
pub trait GithubServiceTrait: Send + Sync {
    /// Create a GitHub issue. Returns the created issue URL.
    async fn create_issue(
        &self,
        working_dir: &Path,
        repository: &str,
        title: &str,
        body_file: &Path,
    ) -> AppResult<String>;

    /// Create a draft pull request. Returns (pr_number, pr_url).
    async fn create_draft_pr(
        &self,
        working_dir: &Path,
        base: &str,
        head: &str,
        title: &str,
        body_file: &Path,
    ) -> AppResult<(i64, String)>;

    /// Convert an existing draft PR to ready-for-review.
    async fn mark_pr_ready(&self, working_dir: &Path, pr_number: i64) -> AppResult<()>;

    /// Update an existing pull request title/body.
    async fn update_pr_details(
        &self,
        working_dir: &Path,
        pr_number: i64,
        title: &str,
        body_file: &Path,
    ) -> AppResult<()>;

    /// Patch one or both existing pull-request metadata fields without
    /// synthesizing or resending the omitted field.
    async fn patch_pr_metadata(
        &self,
        working_dir: &Path,
        pr_number: i64,
        title: Option<&str>,
        body_file: Option<&Path>,
    ) -> AppResult<()> {
        validate_pr_metadata_patch(title, body_file)?;
        let _ = (working_dir, pr_number, title, body_file);
        Err(AppError::Validation(
            "GitHub metadata patching is not supported by this service".to_string(),
        ))
    }

    /// Update an existing pull request's base branch.
    async fn update_pr_base(&self, working_dir: &Path, pr_number: i64, base: &str)
        -> AppResult<()>;

    /// Check the current status of a PR.
    async fn check_pr_status(&self, working_dir: &Path, pr_number: i64) -> AppResult<PrStatus>;

    /// Check the current status and branch-freshness state of a PR.
    async fn check_pr_sync_state(
        &self,
        working_dir: &Path,
        pr_number: i64,
    ) -> AppResult<PrSyncState>;

    /// Return the latest outstanding GitHub requested-changes review, if any.
    async fn check_pr_review_feedback(
        &self,
        _working_dir: &Path,
        _pr_number: i64,
    ) -> AppResult<Option<PrReviewFeedback>> {
        Ok(None)
    }

    /// Fetch normalized review/check annotations for display in RalphX diff viewers.
    async fn fetch_pr_diff_annotations(
        &self,
        _working_dir: &Path,
        pr_number: i64,
    ) -> AppResult<PrDiffAnnotations> {
        Ok(PrDiffAnnotations::empty(pr_number))
    }

    /// Fetch the full PR description payload (title/body/author/state/refs) for
    /// the read-only detail view. The default errors so non-`gh` doubles need no
    /// override unless they exercise the detail path (no meaningful empty value).
    async fn fetch_pr_detail(&self, _working_dir: &Path, _pr_number: i64) -> AppResult<PrDetail> {
        Err(crate::error::AppError::Infrastructure(
            "GitHub PR detail fetch is unavailable for this runtime".to_string(),
        ))
    }

    /// Fetch the live inline review-thread comments for a PR (transient; never
    /// persisted). Factored out of `fetch_pr_diff_annotations` so the detail view
    /// can render the review conversation without the opt-in annotation fan-out.
    /// The default returns an empty thread so non-`gh` doubles need no override.
    async fn fetch_pr_review_thread(
        &self,
        _working_dir: &Path,
        pr_number: i64,
    ) -> AppResult<PrReviewThread> {
        Ok(PrReviewThread::empty(pr_number))
    }

    /// Submit a summary-level GitHub pull request review.
    async fn submit_pr_review(
        &self,
        _working_dir: &Path,
        _pr_number: i64,
        _event: PrReviewSubmissionEvent,
        _body: &str,
    ) -> AppResult<PrSubmittedReview> {
        Err(crate::error::AppError::Infrastructure(
            "GitHub review submission is unavailable for this runtime".to_string(),
        ))
    }

    /// Fetch lightweight PR health for background supervision.
    async fn fetch_pr_health(&self, working_dir: &Path, pr_number: i64) -> AppResult<PrHealth> {
        let sync_state = self.check_pr_sync_state(working_dir, pr_number).await?;
        Ok(PrHealth {
            sync_state,
            review_decision: None,
            checks: Vec::new(),
            issue_comments: Vec::new(),
            auto_merge_request: None,
        })
    }

    /// Enable GitHub auto-merge for a PR with the selected merge method.
    async fn enable_pr_auto_merge(
        &self,
        _working_dir: &Path,
        _pr_number: i64,
        _method: &str,
    ) -> AppResult<()> {
        Ok(())
    }

    /// Disable GitHub auto-merge for a PR.
    async fn disable_pr_auto_merge(&self, _working_dir: &Path, _pr_number: i64) -> AppResult<()> {
        Ok(())
    }

    /// Push a branch to origin.
    async fn push_branch(&self, working_dir: &Path, branch: &str) -> AppResult<()>;

    /// Replace one proven local branch ref only when origin still has the exact expected OID.
    ///
    /// Callers must establish workspace ownership and Git target-lease authority before using
    /// this operation. `local_ref` is deliberately fully qualified so the implementation cannot
    /// synthesize a destination from an untrusted branch-name convention.
    async fn push_branch_with_expected_remote_oid_lease(
        &self,
        _working_dir: &Path,
        _local_ref: &str,
        _expected_remote_oid: &str,
    ) -> AppResult<()> {
        Err(AppError::Validation(
            "exact force-with-lease branch publishing is not supported by this service".to_string(),
        ))
    }

    /// Close (without merging) a pull request.
    async fn close_pr(&self, working_dir: &Path, pr_number: i64) -> AppResult<()>;

    /// Delete a remote branch. Already-deleted branches are treated as no-op.
    async fn delete_remote_branch(&self, working_dir: &Path, branch: &str) -> AppResult<()>;

    /// Fetch a branch from origin.
    async fn fetch_remote(&self, working_dir: &Path, branch: &str) -> AppResult<()>;

    /// Return a pull request's unified patch diff.
    async fn get_pr_diff_patch(
        &self,
        working_dir: &Path,
        pr_number: i64,
        pr_url: Option<&str>,
    ) -> AppResult<String>;

    /// Find an existing open PR by head branch. Returns (pr_number, pr_url) if found.
    async fn find_pr_by_head_branch(
        &self,
        working_dir: &Path,
        head: &str,
    ) -> AppResult<Option<(i64, String)>>;

    /// Search open pull requests for base-picker selection.
    async fn search_pull_requests(
        &self,
        _working_dir: &Path,
        _query: Option<&str>,
        _limit: usize,
    ) -> AppResult<Vec<PrSearchResult>> {
        Ok(Vec::new())
    }

    /// Find the latest PR for a head branch across all GitHub states.
    async fn find_latest_pr_by_head_branch(
        &self,
        _working_dir: &Path,
        _head: &str,
    ) -> AppResult<Option<PrBranchMatch>> {
        Ok(None)
    }

    /// List recent PR branch matches across all GitHub states.
    async fn list_pull_request_branch_matches(
        &self,
        _working_dir: &Path,
        _limit: usize,
    ) -> AppResult<Vec<PrBranchMatch>> {
        Ok(Vec::new())
    }

    /// Reflect the locally-authenticated `gh` CLI via `gh auth status`.
    ///
    /// Returns a typed status (never an error) distinguishing gh not-installed,
    /// gh unauthenticated, and authenticated (+ host/account). The default body
    /// reports "unavailable" so non-`gh` doubles need no override.
    async fn fetch_github_connection_status(&self) -> AppResult<GithubConnectionStatus> {
        Ok(GithubConnectionStatus::unavailable())
    }
}

#[cfg(test)]
#[path = "github_service_tests.rs"]
mod tests;
