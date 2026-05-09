// GitHub service abstraction (AD2)
//
// Trait-based design allows production `gh` CLI implementation and mock for tests.
// All methods take `working_dir: &Path` for stateless, multi-project support (AD7).

use async_trait::async_trait;
use std::path::Path;

use crate::AppResult;

/// Status of a GitHub pull request
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrStatus {
    Open,
    Closed,
    Merged {
        /// SHA of the merge commit, if available
        merge_commit_sha: Option<String>,
    },
}

/// GitHub's merge-state status for an open pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrMergeableState {
    Mergeable,
    Conflicting,
    Unknown,
    Other(String),
}

/// Rich GitHub pull-request state used by PR-mode freshness reconciliation.
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// Inline review comment attached to a GitHub pull request review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrReviewCommentFeedback {
    pub id: String,
    pub author: String,
    pub path: Option<String>,
    pub line: Option<i64>,
    pub body: String,
}

/// Actionable GitHub review feedback that should re-enter RalphX revision flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrReviewFeedback {
    pub review_id: String,
    pub author: String,
    pub submitted_at: Option<String>,
    pub body: Option<String>,
    pub comments: Vec<PrReviewCommentFeedback>,
}

/// Abstraction over GitHub operations (production: `gh` CLI, tests: mock)
#[async_trait]
pub trait GithubServiceTrait: Send + Sync {
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

    /// Push a branch to origin.
    async fn push_branch(&self, working_dir: &Path, branch: &str) -> AppResult<()>;

    /// Close (without merging) a pull request.
    async fn close_pr(&self, working_dir: &Path, pr_number: i64) -> AppResult<()>;

    /// Delete a remote branch. Already-deleted branches are treated as no-op.
    async fn delete_remote_branch(&self, working_dir: &Path, branch: &str) -> AppResult<()>;

    /// Fetch a branch from origin.
    async fn fetch_remote(&self, working_dir: &Path, branch: &str) -> AppResult<()>;

    /// Find an existing open PR by head branch. Returns (pr_number, pr_url) if found.
    async fn find_pr_by_head_branch(
        &self,
        working_dir: &Path,
        head: &str,
    ) -> AppResult<Option<(i64, String)>>;
}
