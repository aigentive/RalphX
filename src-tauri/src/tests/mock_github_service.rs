// MockGithubService — test double for GithubServiceTrait
//
// Configurable per-method return values and call tracking.
// No real `gh` or `git` invocations.

use async_trait::async_trait;
use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::domain::services::github_service::{
    validate_pr_metadata_patch, GithubConnectionStatus, GithubServiceTrait, PrBranchMatch,
    PrDetail, PrDiffAnnotations, PrHealth, PrReviewFeedback, PrReviewSubmissionEvent,
    PrReviewThread, PrSearchResult, PrStatus, PrSubmittedReview, PrSyncState,
};
use crate::error::AppError;
use crate::AppResult;

/// Shared state allowing callers to configure responses and inspect call counts.
#[derive(Debug, Default)]
pub struct MockGithubState {
    // --- Configurable responses ---
    pub create_issue_result: Option<AppResult<String>>,
    pub create_draft_pr_result: Option<AppResult<(i64, String)>>,
    pub mark_pr_ready_result: Option<AppResult<()>>,
    pub update_pr_details_result: Option<AppResult<()>>,
    pub patch_pr_metadata_result: Option<AppResult<()>>,
    pub patch_pr_metadata_responses: VecDeque<AppResult<()>>,
    pub update_pr_base_result: Option<AppResult<()>>,
    pub check_pr_status_result: Option<AppResult<PrStatus>>,
    pub check_pr_sync_state_result: Option<AppResult<PrSyncState>>,
    pub check_pr_review_feedback_result: Option<AppResult<Option<PrReviewFeedback>>>,
    pub check_pr_review_feedback_delay_ms: u64,
    pub fetch_pr_diff_annotations_result: Option<AppResult<PrDiffAnnotations>>,
    pub fetch_pr_diff_annotations_delay_ms: u64,
    pub fetch_pr_detail_result: Option<AppResult<PrDetail>>,
    pub fetch_pr_detail_responses: VecDeque<AppResult<PrDetail>>,
    pub fetch_pr_review_thread_result: Option<AppResult<PrReviewThread>>,
    pub fetch_github_connection_status_result: Option<AppResult<GithubConnectionStatus>>,
    pub fetch_pr_health_result: Option<AppResult<PrHealth>>,
    pub rerun_failed_workflow_result: Option<AppResult<()>>,
    pub enable_pr_auto_merge_result: Option<AppResult<()>>,
    pub enable_pr_auto_merge_delay_ms: u64,
    pub disable_pr_auto_merge_result: Option<AppResult<()>>,
    pub disable_pr_auto_merge_delay_ms: u64,
    pub disable_pr_auto_merge_followup_health_result: Option<AppResult<PrHealth>>,
    pub push_branch_result: Option<AppResult<()>>,
    pub push_branch_delay_ms: u64,
    pub push_branch_started: Option<Arc<tokio::sync::Notify>>,
    pub push_branch_with_expected_remote_oid_lease_result: Option<AppResult<()>>,
    pub push_branch_with_expected_remote_oid_lease_delay_ms: u64,
    pub push_branch_with_expected_remote_oid_lease_started: Option<Arc<tokio::sync::Notify>>,
    pub close_pr_result: Option<AppResult<()>>,
    pub delete_remote_branch_result: Option<AppResult<()>>,
    pub fetch_remote_result: Option<AppResult<()>>,
    pub get_pr_diff_patch_result: Option<AppResult<String>>,
    pub find_pr_by_head_branch_result: Option<AppResult<Option<(i64, String)>>>,
    pub find_pr_by_head_branch_responses: VecDeque<AppResult<Option<(i64, String)>>>,
    pub search_pull_requests_result: Option<AppResult<Vec<PrSearchResult>>>,
    pub find_latest_pr_by_head_branch_result: Option<AppResult<Option<PrBranchMatch>>>,
    pub list_pull_request_branch_matches_result: Option<AppResult<Vec<PrBranchMatch>>>,
    pub submit_pr_review_result: Option<AppResult<PrSubmittedReview>>,

    // --- Call tracking ---
    pub create_issue_calls: u32,
    pub create_draft_pr_calls: u32,
    pub mark_pr_ready_calls: u32,
    pub update_pr_details_calls: u32,
    pub patch_pr_metadata_calls: u32,
    pub update_pr_base_calls: u32,
    pub check_pr_status_calls: u32,
    pub check_pr_sync_state_calls: u32,
    pub check_pr_review_feedback_calls: u32,
    pub active_check_pr_review_feedback_calls: u32,
    pub max_concurrent_check_pr_review_feedback_calls: u32,
    pub fetch_pr_diff_annotations_calls: u32,
    pub fetch_pr_detail_calls: u32,
    pub fetch_pr_review_thread_calls: u32,
    pub fetch_github_connection_status_calls: u32,
    pub fetch_pr_health_calls: u32,
    pub rerun_failed_workflow_calls: u32,
    pub enable_pr_auto_merge_calls: u32,
    pub disable_pr_auto_merge_calls: u32,
    pub push_branch_calls: u32,
    pub push_branch_with_expected_remote_oid_lease_calls: u32,
    pub close_pr_calls: u32,
    pub delete_remote_branch_calls: u32,
    pub fetch_remote_calls: u32,
    pub get_pr_diff_patch_calls: u32,
    pub find_pr_by_head_branch_calls: u32,
    pub search_pull_requests_calls: u32,
    pub find_latest_pr_by_head_branch_calls: u32,
    pub list_pull_request_branch_matches_calls: u32,
    pub submit_pr_review_calls: u32,

    // --- Last arguments recorded ---
    pub last_create_issue_args: Option<(String, String, String)>,
    pub last_create_issue_body: Option<String>,
    pub last_create_draft_pr_args: Option<(String, String, String, String)>,
    pub last_create_draft_pr_body: Option<String>,
    pub last_mark_pr_ready_number: Option<i64>,
    pub last_update_pr_details_args: Option<(i64, String, String)>,
    pub last_update_pr_details_body: Option<String>,
    pub last_patch_pr_metadata_args: Option<(i64, Option<String>, Option<String>)>,
    pub last_patch_pr_metadata_body: Option<String>,
    pub last_update_pr_base_args: Option<(i64, String)>,
    pub last_check_pr_status_number: Option<i64>,
    pub last_check_pr_sync_state_number: Option<i64>,
    pub last_check_pr_review_feedback_number: Option<i64>,
    pub last_fetch_pr_diff_annotations_number: Option<i64>,
    pub last_fetch_pr_detail_number: Option<i64>,
    pub last_fetch_pr_review_thread_number: Option<i64>,
    pub last_fetch_pr_health_number: Option<i64>,
    pub last_rerun_failed_workflow_id: Option<i64>,
    pub last_mark_pr_ready_working_dir: Option<String>,
    pub last_enable_pr_auto_merge_args: Option<(i64, String)>,
    pub last_enable_pr_auto_merge_working_dir: Option<String>,
    pub last_disable_pr_auto_merge_number: Option<i64>,
    pub last_disable_pr_auto_merge_working_dir: Option<String>,
    pub last_push_branch_name: Option<String>,
    pub last_push_branch_with_expected_remote_oid_lease_args: Option<(String, String)>,
    pub last_close_pr_number: Option<i64>,
    pub last_delete_remote_branch_name: Option<String>,
    /// All branches passed to delete_remote_branch (accumulated across all calls).
    pub all_deleted_remote_branch_names: Vec<String>,
    pub last_fetch_remote_branch_name: Option<String>,
    pub last_get_pr_diff_patch_number: Option<i64>,
    pub last_get_pr_diff_patch_url: Option<String>,
    pub last_find_pr_by_head_branch_name: Option<String>,
    pub last_search_pull_requests_args: Option<(Option<String>, usize)>,
    pub last_find_latest_pr_by_head_branch_name: Option<String>,
    pub last_list_pull_request_branch_matches_limit: Option<usize>,
    pub last_submit_pr_review_args: Option<(i64, PrReviewSubmissionEvent, String)>,
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

    /// Shorthand: configure create_issue to succeed with the given URL.
    pub fn will_create_issue(&self, url: impl Into<String>) {
        self.state().create_issue_result = Some(Ok(url.into()));
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

    /// Shorthand: configure PR diff annotations returned by fetch_pr_diff_annotations.
    #[allow(dead_code)]
    pub fn will_return_pr_diff_annotations(&self, annotations: PrDiffAnnotations) {
        self.state().fetch_pr_diff_annotations_result = Some(Ok(annotations));
    }

    /// Shorthand: add an artificial delay to annotation fetches so tests can
    /// observe command-level request coalescing.
    #[allow(dead_code)]
    pub fn with_pr_diff_annotations_delay_ms(&self, delay_ms: u64) {
        self.state().fetch_pr_diff_annotations_delay_ms = delay_ms;
    }

    /// Shorthand: configure fetch_pr_detail to succeed with the given detail.
    #[allow(dead_code)]
    pub fn will_return_pr_detail(&self, detail: PrDetail) {
        self.state().fetch_pr_detail_result = Some(Ok(detail));
    }

    /// Queue exact PR detail responses for authority drift and recovery tests.
    #[allow(dead_code)]
    pub fn queue_pr_detail(&self, result: AppResult<PrDetail>) {
        self.state().fetch_pr_detail_responses.push_back(result);
    }

    /// Shorthand: configure fetch_pr_detail to fail with the given message.
    #[allow(dead_code)]
    pub fn will_fail_pr_detail(&self, msg: impl Into<String>) {
        self.state().fetch_pr_detail_result = Some(Err(AppError::Infrastructure(msg.into())));
    }

    /// Shorthand: configure fetch_pr_review_thread to return the given thread.
    #[allow(dead_code)]
    pub fn will_return_pr_review_thread(&self, thread: PrReviewThread) {
        self.state().fetch_pr_review_thread_result = Some(Ok(thread));
    }

    /// Shorthand: configure fetch_pr_review_thread to fail with the given message.
    #[allow(dead_code)]
    pub fn will_fail_pr_review_thread(&self, msg: impl Into<String>) {
        self.state().fetch_pr_review_thread_result =
            Some(Err(AppError::Infrastructure(msg.into())));
    }

    /// Shorthand: configure the gh connection status (auth gate).
    #[allow(dead_code)]
    pub fn will_return_connection_status(&self, status: GithubConnectionStatus) {
        self.state().fetch_github_connection_status_result = Some(Ok(status));
    }

    /// Shorthand: report an authenticated gh session for the given host/account.
    #[allow(dead_code)]
    pub fn will_be_authenticated(&self, host: impl Into<String>, account: impl Into<String>) {
        self.state().fetch_github_connection_status_result =
            Some(Ok(GithubConnectionStatus::authenticated(host, account)));
    }

    /// Shorthand: configure any method to fail with the given message (Infrastructure error).
    pub fn will_fail_create_pr(&self, msg: impl Into<String>) {
        self.state().create_draft_pr_result = Some(Err(AppError::Infrastructure(msg.into())));
    }

    /// Queue exact metadata-patch responses for ambiguous outcome and retry tests.
    #[allow(dead_code)]
    pub fn queue_patch_pr_metadata_result(&self, result: AppResult<()>) {
        self.state().patch_pr_metadata_responses.push_back(result);
    }

    /// Shorthand: configure find_pr_by_head_branch to return the given result.
    #[allow(dead_code)]
    pub fn set_find_pr_by_head_branch(&self, result: AppResult<Option<(i64, String)>>) {
        self.state().find_pr_by_head_branch_result = Some(result);
    }

    /// Queue exact head-branch lookup responses for retry and duplicate tests.
    #[allow(dead_code)]
    pub fn queue_find_pr_by_head_branch(&self, result: AppResult<Option<(i64, String)>>) {
        self.state()
            .find_pr_by_head_branch_responses
            .push_back(result);
    }

    /// Shorthand: configure pull request search to return the given results.
    #[allow(dead_code)]
    pub fn will_return_pull_request_search(&self, results: Vec<PrSearchResult>) {
        self.state().search_pull_requests_result = Some(Ok(results));
    }

    /// Shorthand: configure all-state head branch lookup to return the given result.
    #[allow(dead_code)]
    pub fn set_find_latest_pr_by_head_branch(&self, result: AppResult<Option<PrBranchMatch>>) {
        self.state().find_latest_pr_by_head_branch_result = Some(result);
    }

    /// Shorthand: configure all-state PR branch matches to return the given results.
    #[allow(dead_code)]
    pub fn will_return_pull_request_branch_matches(&self, results: Vec<PrBranchMatch>) {
        self.state().list_pull_request_branch_matches_result = Some(Ok(results));
    }

    /// Shorthand: configure submit_pr_review to succeed with the given review id/url.
    #[allow(dead_code)]
    pub fn will_submit_pr_review(&self, id: impl Into<String>, url: Option<String>) {
        self.state().submit_pr_review_result = Some(Ok(PrSubmittedReview { id: id.into(), url }));
    }

    /// Shorthand: configure submit_pr_review to fail with the given message.
    #[allow(dead_code)]
    pub fn will_fail_submit_pr_review(&self, msg: impl Into<String>) {
        self.state().submit_pr_review_result = Some(Err(AppError::Infrastructure(msg.into())));
    }
}

impl Default for MockGithubService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl GithubServiceTrait for MockGithubService {
    async fn create_issue(
        &self,
        _working_dir: &Path,
        repository: &str,
        title: &str,
        body_file: &Path,
    ) -> AppResult<String> {
        let mut s = self.state.lock().expect("lock poisoned");
        s.create_issue_calls += 1;
        s.last_create_issue_args = Some((
            repository.to_string(),
            title.to_string(),
            body_file.to_string_lossy().into_owned(),
        ));
        s.last_create_issue_body = std::fs::read_to_string(body_file).ok();
        s.create_issue_result
            .take()
            .unwrap_or(Ok("https://github.com/owner/repo/issues/1".to_string()))
    }

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

    async fn mark_pr_ready(&self, working_dir: &Path, pr_number: i64) -> AppResult<()> {
        let mut s = self.state.lock().expect("lock poisoned");
        s.mark_pr_ready_calls += 1;
        s.last_mark_pr_ready_number = Some(pr_number);
        s.last_mark_pr_ready_working_dir = Some(working_dir.to_string_lossy().into_owned());
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

    async fn patch_pr_metadata(
        &self,
        _working_dir: &Path,
        pr_number: i64,
        title: Option<&str>,
        body_file: Option<&Path>,
    ) -> AppResult<()> {
        validate_pr_metadata_patch(title, body_file)?;
        let mut s = self.state.lock().expect("lock poisoned");
        s.patch_pr_metadata_calls += 1;
        s.last_patch_pr_metadata_args = Some((
            pr_number,
            title.map(str::to_string),
            body_file.map(|path| path.to_string_lossy().into_owned()),
        ));
        s.last_patch_pr_metadata_body =
            body_file.and_then(|path| std::fs::read_to_string(path).ok());
        s.patch_pr_metadata_responses
            .pop_front()
            .or_else(|| s.patch_pr_metadata_result.take())
            .unwrap_or(Ok(()))
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

    async fn fetch_pr_diff_annotations(
        &self,
        _working_dir: &Path,
        pr_number: i64,
    ) -> AppResult<PrDiffAnnotations> {
        let (delay_ms, result) = {
            let mut s = self.state.lock().expect("lock poisoned");
            s.fetch_pr_diff_annotations_calls += 1;
            s.last_fetch_pr_diff_annotations_number = Some(pr_number);
            (
                s.fetch_pr_diff_annotations_delay_ms,
                s.fetch_pr_diff_annotations_result.take(),
            )
        };

        if delay_ms > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        }

        result.unwrap_or_else(|| Ok(PrDiffAnnotations::empty(pr_number)))
    }

    async fn fetch_pr_detail(&self, _working_dir: &Path, pr_number: i64) -> AppResult<PrDetail> {
        let mut s = self.state.lock().expect("lock poisoned");
        s.fetch_pr_detail_calls += 1;
        s.last_fetch_pr_detail_number = Some(pr_number);
        s.fetch_pr_detail_responses
            .pop_front()
            .or_else(|| s.fetch_pr_detail_result.take())
            .unwrap_or_else(|| {
                Err(AppError::Infrastructure(
                    "MockGithubService::fetch_pr_detail not configured".to_string(),
                ))
            })
    }

    async fn fetch_pr_review_thread(
        &self,
        _working_dir: &Path,
        pr_number: i64,
    ) -> AppResult<PrReviewThread> {
        let mut s = self.state.lock().expect("lock poisoned");
        s.fetch_pr_review_thread_calls += 1;
        s.last_fetch_pr_review_thread_number = Some(pr_number);
        s.fetch_pr_review_thread_result
            .take()
            .unwrap_or_else(|| Ok(PrReviewThread::empty(pr_number)))
    }

    async fn fetch_github_connection_status(&self) -> AppResult<GithubConnectionStatus> {
        let mut s = self.state.lock().expect("lock poisoned");
        s.fetch_github_connection_status_calls += 1;
        s.fetch_github_connection_status_result
            .take()
            .unwrap_or_else(|| Ok(GithubConnectionStatus::unavailable()))
    }

    async fn fetch_pr_health(&self, working_dir: &Path, pr_number: i64) -> AppResult<PrHealth> {
        let configured = {
            let mut s = self.state.lock().expect("lock poisoned");
            s.fetch_pr_health_calls += 1;
            s.last_fetch_pr_health_number = Some(pr_number);
            s.fetch_pr_health_result.take()
        };
        if let Some(result) = configured {
            return result;
        }
        let sync_state = self.check_pr_sync_state(working_dir, pr_number).await?;
        Ok(PrHealth {
            sync_state,
            review_decision: None,
            checks: Vec::new(),
            issue_comments: Vec::new(),
            auto_merge_request: None,
        })
    }

    async fn rerun_failed_workflow(&self, _working_dir: &Path, run_id: i64) -> AppResult<()> {
        let mut s = self.state.lock().expect("lock poisoned");
        s.rerun_failed_workflow_calls += 1;
        s.last_rerun_failed_workflow_id = Some(run_id);
        s.rerun_failed_workflow_result
            .take()
            .unwrap_or_else(|| Ok(()))
    }

    async fn enable_pr_auto_merge(
        &self,
        working_dir: &Path,
        pr_number: i64,
        method: &str,
    ) -> AppResult<()> {
        let (delay_ms, result) = {
            let mut s = self.state.lock().expect("lock poisoned");
            s.enable_pr_auto_merge_calls += 1;
            s.last_enable_pr_auto_merge_args = Some((pr_number, method.to_string()));
            s.last_enable_pr_auto_merge_working_dir =
                Some(working_dir.to_string_lossy().into_owned());
            (
                s.enable_pr_auto_merge_delay_ms,
                s.enable_pr_auto_merge_result.take(),
            )
        };
        if delay_ms > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        }
        result.unwrap_or(Ok(()))
    }

    async fn disable_pr_auto_merge(&self, working_dir: &Path, pr_number: i64) -> AppResult<()> {
        let (delay_ms, result) = {
            let mut s = self.state.lock().expect("lock poisoned");
            s.disable_pr_auto_merge_calls += 1;
            s.last_disable_pr_auto_merge_number = Some(pr_number);
            s.last_disable_pr_auto_merge_working_dir =
                Some(working_dir.to_string_lossy().into_owned());
            if let Some(followup) = s.disable_pr_auto_merge_followup_health_result.take() {
                s.fetch_pr_health_result = Some(followup);
            }
            (
                s.disable_pr_auto_merge_delay_ms,
                s.disable_pr_auto_merge_result.take().unwrap_or(Ok(())),
            )
        };
        if delay_ms > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        }
        result
    }

    async fn push_branch(&self, _working_dir: &Path, branch: &str) -> AppResult<()> {
        let (delay_ms, result) = {
            let mut state = self.state.lock().expect("lock poisoned");
            state.push_branch_calls += 1;
            state.last_push_branch_name = Some(branch.to_string());
            if let Some(started) = state.push_branch_started.as_ref() {
                started.notify_one();
            }
            (
                state.push_branch_delay_ms,
                state.push_branch_result.take().unwrap_or(Ok(())),
            )
        };
        if delay_ms > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        }
        result
    }

    async fn push_branch_with_expected_remote_oid_lease(
        &self,
        _working_dir: &Path,
        local_ref: &str,
        expected_remote_oid: &str,
    ) -> AppResult<()> {
        let (delay_ms, result) = {
            let mut state = self.state.lock().expect("lock poisoned");
            state.push_branch_with_expected_remote_oid_lease_calls += 1;
            state.last_push_branch_with_expected_remote_oid_lease_args =
                Some((local_ref.to_string(), expected_remote_oid.to_string()));
            if let Some(started) = state
                .push_branch_with_expected_remote_oid_lease_started
                .as_ref()
            {
                started.notify_one();
            }
            (
                state.push_branch_with_expected_remote_oid_lease_delay_ms,
                state
                    .push_branch_with_expected_remote_oid_lease_result
                    .take()
                    .unwrap_or(Ok(())),
            )
        };
        if delay_ms > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        }
        result
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

    async fn get_pr_diff_patch(
        &self,
        _working_dir: &Path,
        pr_number: i64,
        pr_url: Option<&str>,
    ) -> AppResult<String> {
        let mut s = self.state.lock().expect("lock poisoned");
        s.get_pr_diff_patch_calls += 1;
        s.last_get_pr_diff_patch_number = Some(pr_number);
        s.last_get_pr_diff_patch_url = pr_url.map(str::to_string);
        s.get_pr_diff_patch_result
            .take()
            .unwrap_or_else(|| Ok(String::new()))
    }

    async fn find_pr_by_head_branch(
        &self,
        _working_dir: &Path,
        head: &str,
    ) -> AppResult<Option<(i64, String)>> {
        let mut s = self.state.lock().expect("lock poisoned");
        s.find_pr_by_head_branch_calls += 1;
        s.last_find_pr_by_head_branch_name = Some(head.to_string());
        s.find_pr_by_head_branch_responses
            .pop_front()
            .or_else(|| s.find_pr_by_head_branch_result.take())
            .unwrap_or(Ok(None))
    }

    async fn search_pull_requests(
        &self,
        _working_dir: &Path,
        query: Option<&str>,
        limit: usize,
    ) -> AppResult<Vec<PrSearchResult>> {
        let mut s = self.state.lock().expect("lock poisoned");
        s.search_pull_requests_calls += 1;
        s.last_search_pull_requests_args = Some((query.map(str::to_string), limit));
        s.search_pull_requests_result
            .take()
            .unwrap_or_else(|| Ok(Vec::new()))
    }

    async fn find_latest_pr_by_head_branch(
        &self,
        _working_dir: &Path,
        head: &str,
    ) -> AppResult<Option<PrBranchMatch>> {
        let mut s = self.state.lock().expect("lock poisoned");
        s.find_latest_pr_by_head_branch_calls += 1;
        s.last_find_latest_pr_by_head_branch_name = Some(head.to_string());
        s.find_latest_pr_by_head_branch_result
            .take()
            .unwrap_or(Ok(None))
    }

    async fn list_pull_request_branch_matches(
        &self,
        _working_dir: &Path,
        limit: usize,
    ) -> AppResult<Vec<PrBranchMatch>> {
        let mut s = self.state.lock().expect("lock poisoned");
        s.list_pull_request_branch_matches_calls += 1;
        s.last_list_pull_request_branch_matches_limit = Some(limit);
        s.list_pull_request_branch_matches_result
            .take()
            .unwrap_or_else(|| Ok(Vec::new()))
    }

    async fn submit_pr_review(
        &self,
        _working_dir: &Path,
        pr_number: i64,
        event: PrReviewSubmissionEvent,
        body: &str,
    ) -> AppResult<PrSubmittedReview> {
        let mut s = self.state.lock().expect("lock poisoned");
        s.submit_pr_review_calls += 1;
        s.last_submit_pr_review_args = Some((pr_number, event, body.to_string()));
        s.submit_pr_review_result.take().unwrap_or_else(|| {
            Err(AppError::Infrastructure(
                "GitHub review submission is unavailable for this runtime".to_string(),
            ))
        })
    }
}
