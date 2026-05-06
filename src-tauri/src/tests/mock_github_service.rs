// MockGithubService — test double for GithubServiceTrait
//
// Configurable per-method return values and call tracking.
// No real `gh` or `git` invocations.

use async_trait::async_trait;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::domain::services::github_service::{
    GithubServiceTrait, PrReviewFeedback, PrStatus, PrSyncState,
};
use crate::error::AppError;
use crate::AppResult;

/// Shared state allowing callers to configure responses and inspect call counts.
#[derive(Debug, Default)]
pub struct MockGithubState {
    // --- Configurable responses ---
    pub create_draft_pr_result: Option<AppResult<(i64, String)>>,
    pub mark_pr_ready_result: Option<AppResult<()>>,
    pub update_pr_details_result: Option<AppResult<()>>,
    pub update_pr_base_result: Option<AppResult<()>>,
    pub check_pr_status_result: Option<AppResult<PrStatus>>,
    pub check_pr_sync_state_result: Option<AppResult<PrSyncState>>,
    pub check_pr_review_feedback_result: Option<AppResult<Option<PrReviewFeedback>>>,
    pub check_pr_review_feedback_delay_ms: u64,
    pub push_branch_result: Option<AppResult<()>>,
    pub close_pr_result: Option<AppResult<()>>,
    pub delete_remote_branch_result: Option<AppResult<()>>,
    pub fetch_remote_result: Option<AppResult<()>>,
    pub find_pr_by_head_branch_result: Option<AppResult<Option<(i64, String)>>>,

    // --- Call tracking ---
    pub create_draft_pr_calls: u32,
    pub mark_pr_ready_calls: u32,
    pub update_pr_details_calls: u32,
    pub update_pr_base_calls: u32,
    pub check_pr_status_calls: u32,
    pub check_pr_sync_state_calls: u32,
    pub check_pr_review_feedback_calls: u32,
    pub active_check_pr_review_feedback_calls: u32,
    pub max_concurrent_check_pr_review_feedback_calls: u32,
    pub push_branch_calls: u32,
    pub close_pr_calls: u32,
    pub delete_remote_branch_calls: u32,
    pub fetch_remote_calls: u32,
    pub find_pr_by_head_branch_calls: u32,

    // --- Last arguments recorded ---
    pub last_create_draft_pr_args: Option<(String, String, String, String)>,
    pub last_create_draft_pr_body: Option<String>,
    pub last_mark_pr_ready_number: Option<i64>,
    pub last_update_pr_details_args: Option<(i64, String, String)>,
    pub last_update_pr_details_body: Option<String>,
    pub last_update_pr_base_args: Option<(i64, String)>,
    pub last_check_pr_status_number: Option<i64>,
    pub last_check_pr_sync_state_number: Option<i64>,
    pub last_check_pr_review_feedback_number: Option<i64>,
    pub last_push_branch_name: Option<String>,
    pub last_close_pr_number: Option<i64>,
    pub last_delete_remote_branch_name: Option<String>,
    /// All branches passed to delete_remote_branch (accumulated across all calls).
    pub all_deleted_remote_branch_names: Vec<String>,
    pub last_fetch_remote_branch_name: Option<String>,
    pub last_find_pr_by_head_branch_name: Option<String>,
}

/// Mock implementation of GithubServiceTrait for unit tests.
///
/// # Example
/// ```rust
/// let mock = MockGithubService::new();
/// mock.state().create_draft_pr_result = Some(Ok((42, "https://github.com/...".into())));
/// // ... use in test
/// assert_eq!(mock.state().create_draft_pr_calls, 1);
/// ```
pub struct MockGithubService {
    state: Arc<Mutex<MockGithubState>>,
}

impl MockGithubService {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(MockGithubState::default())),
        }
    }

    /// Access the inner state to configure responses or read call counts.
    pub fn state(&self) -> std::sync::MutexGuard<'_, MockGithubState> {
        self.state.lock().expect("MockGithubState lock poisoned")
    }

    /// Shorthand: configure create_draft_pr to succeed with the given values.
    pub fn will_create_pr(&self, number: i64, url: impl Into<String>) {
        self.state().create_draft_pr_result = Some(Ok((number, url.into())));
    }

    /// Shorthand: configure check_pr_status to return the given status.
    pub fn will_return_status(&self, status: PrStatus) {
        self.state().check_pr_status_result = Some(Ok(status));
    }

    /// Shorthand: configure check_pr_sync_state to return the given state.
    #[allow(dead_code)]
    pub fn will_return_sync_state(&self, state: PrSyncState) {
        self.state().check_pr_sync_state_result = Some(Ok(state));
    }

    /// Shorthand: configure check_pr_review_feedback to return requested changes.
    #[allow(dead_code)]
    pub fn will_return_review_feedback(&self, feedback: PrReviewFeedback) {
        self.state().check_pr_review_feedback_result = Some(Ok(Some(feedback)));
    }

    /// Shorthand: add an artificial delay to review feedback checks so tests can
    /// observe startup recovery concurrency.
    #[allow(dead_code)]
    pub fn with_review_feedback_delay_ms(&self, delay_ms: u64) {
        self.state().check_pr_review_feedback_delay_ms = delay_ms;
    }

    /// Shorthand: configure any method to fail with the given message (Infrastructure error).
    pub fn will_fail_create_pr(&self, msg: impl Into<String>) {
        self.state().create_draft_pr_result = Some(Err(AppError::Infrastructure(msg.into())));
    }

    /// Shorthand: configure find_pr_by_head_branch to return the given result.
    #[allow(dead_code)]
    pub fn set_find_pr_by_head_branch(&self, result: AppResult<Option<(i64, String)>>) {
        self.state().find_pr_by_head_branch_result = Some(result);
    }
}

impl Default for MockGithubService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl GithubServiceTrait for MockGithubService {
    async fn create_draft_pr(
        &self,
        _working_dir: &Path,
        base: &str,
        head: &str,
        title: &str,
        body_file: &Path,
    ) -> AppResult<(i64, String)> {
        let mut s = self.state.lock().expect("lock poisoned");
        s.create_draft_pr_calls += 1;
        s.last_create_draft_pr_args = Some((
            base.to_string(),
            head.to_string(),
            title.to_string(),
            body_file.to_string_lossy().into_owned(),
        ));
        s.last_create_draft_pr_body = std::fs::read_to_string(body_file).ok();
        s.create_draft_pr_result
            .take()
            .unwrap_or(Ok((1, "https://github.com/owner/repo/pull/1".to_string())))
    }

    async fn mark_pr_ready(&self, _working_dir: &Path, pr_number: i64) -> AppResult<()> {
        let mut s = self.state.lock().expect("lock poisoned");
        s.mark_pr_ready_calls += 1;
        s.last_mark_pr_ready_number = Some(pr_number);
        s.mark_pr_ready_result.take().unwrap_or(Ok(()))
    }

    async fn update_pr_details(
        &self,
        _working_dir: &Path,
        pr_number: i64,
        title: &str,
        body_file: &Path,
    ) -> AppResult<()> {
        let mut s = self.state.lock().expect("lock poisoned");
        s.update_pr_details_calls += 1;
        s.last_update_pr_details_args = Some((
            pr_number,
            title.to_string(),
            body_file.to_string_lossy().into_owned(),
        ));
        s.last_update_pr_details_body = std::fs::read_to_string(body_file).ok();
        s.update_pr_details_result.take().unwrap_or(Ok(()))
    }

    async fn update_pr_base(
        &self,
        _working_dir: &Path,
        pr_number: i64,
        base: &str,
    ) -> AppResult<()> {
        let mut s = self.state.lock().expect("lock poisoned");
        s.update_pr_base_calls += 1;
        s.last_update_pr_base_args = Some((pr_number, base.to_string()));
        s.update_pr_base_result.take().unwrap_or(Ok(()))
    }

    async fn check_pr_status(&self, _working_dir: &Path, pr_number: i64) -> AppResult<PrStatus> {
        let mut s = self.state.lock().expect("lock poisoned");
        s.check_pr_status_calls += 1;
        s.last_check_pr_status_number = Some(pr_number);
        s.check_pr_status_result
            .take()
            .unwrap_or(Ok(PrStatus::Open))
    }

    async fn check_pr_sync_state(
        &self,
        _working_dir: &Path,
        pr_number: i64,
    ) -> AppResult<PrSyncState> {
        let mut s = self.state.lock().expect("lock poisoned");
        s.check_pr_sync_state_calls += 1;
        s.last_check_pr_sync_state_number = Some(pr_number);
        s.check_pr_sync_state_result.take().unwrap_or_else(|| {
            Ok(PrSyncState {
                status: PrStatus::Open,
                merge_state_status: None,
                mergeable: None,
                is_draft: false,
                head_ref_name: "feature".to_string(),
                base_ref_name: "main".to_string(),
                head_ref_oid: None,
                base_ref_oid: None,
            })
        })
    }

    async fn check_pr_review_feedback(
        &self,
        _working_dir: &Path,
        pr_number: i64,
    ) -> AppResult<Option<PrReviewFeedback>> {
        let (delay_ms, result) = {
            let mut s = self.state.lock().expect("lock poisoned");
            s.check_pr_review_feedback_calls += 1;
            s.active_check_pr_review_feedback_calls += 1;
            s.max_concurrent_check_pr_review_feedback_calls = s
                .max_concurrent_check_pr_review_feedback_calls
                .max(s.active_check_pr_review_feedback_calls);
            s.last_check_pr_review_feedback_number = Some(pr_number);
            (
                s.check_pr_review_feedback_delay_ms,
                s.check_pr_review_feedback_result.take(),
            )
        };

        if delay_ms > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        }

        let mut s = self.state.lock().expect("lock poisoned");
        s.active_check_pr_review_feedback_calls =
            s.active_check_pr_review_feedback_calls.saturating_sub(1);
        result.unwrap_or(Ok(None))
    }

    async fn push_branch(&self, _working_dir: &Path, branch: &str) -> AppResult<()> {
        let mut s = self.state.lock().expect("lock poisoned");
        s.push_branch_calls += 1;
        s.last_push_branch_name = Some(branch.to_string());
        s.push_branch_result.take().unwrap_or(Ok(()))
    }

    async fn close_pr(&self, _working_dir: &Path, pr_number: i64) -> AppResult<()> {
        let mut s = self.state.lock().expect("lock poisoned");
        s.close_pr_calls += 1;
        s.last_close_pr_number = Some(pr_number);
        s.close_pr_result.take().unwrap_or(Ok(()))
    }

    async fn delete_remote_branch(&self, _working_dir: &Path, branch: &str) -> AppResult<()> {
        let mut s = self.state.lock().expect("lock poisoned");
        s.delete_remote_branch_calls += 1;
        s.last_delete_remote_branch_name = Some(branch.to_string());
        s.all_deleted_remote_branch_names.push(branch.to_string());
        s.delete_remote_branch_result.take().unwrap_or(Ok(()))
    }

    async fn fetch_remote(&self, _working_dir: &Path, branch: &str) -> AppResult<()> {
        let mut s = self.state.lock().expect("lock poisoned");
        s.fetch_remote_calls += 1;
        s.last_fetch_remote_branch_name = Some(branch.to_string());
        s.fetch_remote_result.take().unwrap_or(Ok(()))
    }

    async fn find_pr_by_head_branch(
        &self,
        _working_dir: &Path,
        head: &str,
    ) -> AppResult<Option<(i64, String)>> {
        let mut s = self.state.lock().expect("lock poisoned");
        s.find_pr_by_head_branch_calls += 1;
        s.last_find_pr_by_head_branch_name = Some(head.to_string());
        s.find_pr_by_head_branch_result.take().unwrap_or(Ok(None))
    }
}
