//! Shared test utilities for PR integration tests.
//!
//! `MockGithubService` is NOT accessible from `tests/` (it lives behind `#[cfg(test)]` in
//! `lib.rs`), so we define an inline mock here that is usable from all integration test files
//! via `mod common;`.

use async_trait::async_trait;
use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, Mutex};

use ralphx_lib::domain::services::github_service::{
    GithubServiceTrait, PrMergeStateStatus, PrMergeableState, PrReviewFeedback, PrStatus,
    PrSyncState,
};
use ralphx_lib::{AppError, AppResult};

// ============================================================================
// MockGithubService
// ============================================================================

/// Programmable mock for `GithubServiceTrait` used in PR integration tests.
///
/// Call counters are public so tests can assert on interaction counts.
/// Canned responses can be pre-loaded via the `will_return_*` / `will_fail_*` helpers.
#[allow(dead_code)]
pub struct MockGithubService {
    /// Status responses to return in sequence (last entry repeats when exhausted).
    status_responses: Arc<Mutex<VecDeque<AppResult<PrStatus>>>>,
    sync_state_responses: Arc<Mutex<VecDeque<AppResult<PrSyncState>>>>,
    review_feedback_responses: Arc<Mutex<VecDeque<AppResult<Option<PrReviewFeedback>>>>>,
    pub check_pr_status_calls: Arc<Mutex<u32>>,
    pub check_pr_sync_state_calls: Arc<Mutex<u32>>,
    pub check_pr_review_feedback_calls: Arc<Mutex<u32>>,
    pub push_branch_calls: Arc<Mutex<u32>>,
    pub create_draft_pr_calls: Arc<Mutex<u32>>,
    pub mark_pr_ready_calls: Arc<Mutex<u32>>,
    pub update_pr_details_calls: Arc<Mutex<u32>>,
    pub update_pr_base_calls: Arc<Mutex<u32>>,
    pub close_pr_calls: Arc<Mutex<u32>>,
    pub delete_remote_branch_calls: Arc<Mutex<u32>>,
    pub find_pr_by_head_branch_calls: Arc<Mutex<u32>>,
    push_branch_result: Arc<Mutex<Option<AppResult<()>>>>,
    #[allow(clippy::type_complexity)]
    create_draft_pr_result: Arc<Mutex<Option<AppResult<(i64, String)>>>>,
    mark_pr_ready_result: Arc<Mutex<Option<AppResult<()>>>>,
    update_pr_details_result: Arc<Mutex<Option<AppResult<()>>>>,
    update_pr_base_result: Arc<Mutex<Option<AppResult<()>>>>,
    #[allow(clippy::type_complexity)]
    find_pr_by_head_branch_result: Arc<Mutex<Option<AppResult<Option<(i64, String)>>>>>,
}

#[allow(dead_code)]
impl MockGithubService {
    pub fn new() -> Self {
        Self {
            status_responses: Arc::new(Mutex::new(VecDeque::new())),
            sync_state_responses: Arc::new(Mutex::new(VecDeque::new())),
            review_feedback_responses: Arc::new(Mutex::new(VecDeque::new())),
            check_pr_status_calls: Arc::new(Mutex::new(0)),
            check_pr_sync_state_calls: Arc::new(Mutex::new(0)),
            check_pr_review_feedback_calls: Arc::new(Mutex::new(0)),
            push_branch_calls: Arc::new(Mutex::new(0)),
            create_draft_pr_calls: Arc::new(Mutex::new(0)),
            mark_pr_ready_calls: Arc::new(Mutex::new(0)),
            update_pr_details_calls: Arc::new(Mutex::new(0)),
            update_pr_base_calls: Arc::new(Mutex::new(0)),
            close_pr_calls: Arc::new(Mutex::new(0)),
            delete_remote_branch_calls: Arc::new(Mutex::new(0)),
            find_pr_by_head_branch_calls: Arc::new(Mutex::new(0)),
            push_branch_result: Arc::new(Mutex::new(None)),
            create_draft_pr_result: Arc::new(Mutex::new(None)),
            mark_pr_ready_result: Arc::new(Mutex::new(None)),
            update_pr_details_result: Arc::new(Mutex::new(None)),
            update_pr_base_result: Arc::new(Mutex::new(None)),
            find_pr_by_head_branch_result: Arc::new(Mutex::new(None)),
        }
    }

    /// Enqueue a status response. Responses are returned in FIFO order. When the
    /// queue is exhausted, subsequent calls return `PrStatus::Open`.
    pub fn will_return_status(&self, status: PrStatus) {
        self.status_responses.lock().unwrap().push_back(Ok(status));
    }

    pub fn will_return_sync_state(&self, state: PrSyncState) {
        self.sync_state_responses
            .lock()
            .unwrap()
            .push_back(Ok(state));
    }

    pub fn will_return_review_feedback(&self, feedback: PrReviewFeedback) {
        self.review_feedback_responses
            .lock()
            .unwrap()
            .push_back(Ok(Some(feedback)));
    }

    /// Make the next `push_branch` call fail with the given message.
    pub fn will_fail_push(&self, msg: impl Into<String>) {
        *self.push_branch_result.lock().unwrap() = Some(Err(AppError::Infrastructure(msg.into())));
    }

    /// Make the next `create_draft_pr` call fail with the given message.
    pub fn will_fail_create_pr(&self, msg: impl Into<String>) {
        *self.create_draft_pr_result.lock().unwrap() =
            Some(Err(AppError::Infrastructure(msg.into())));
    }

    /// Make the next `create_draft_pr` call fail with a duplicate-PR error.
    pub fn will_fail_create_pr_duplicate(&self) {
        *self.create_draft_pr_result.lock().unwrap() = Some(Err(AppError::DuplicatePr));
    }

    /// Make the next `mark_pr_ready` call fail with the given message.
    pub fn will_fail_mark_ready(&self, msg: impl Into<String>) {
        *self.mark_pr_ready_result.lock().unwrap() =
            Some(Err(AppError::Infrastructure(msg.into())));
    }

    /// Make the next `update_pr_base` call fail with the given message.
    pub fn will_fail_update_pr_base(&self, msg: impl Into<String>) {
        *self.update_pr_base_result.lock().unwrap() =
            Some(Err(AppError::Infrastructure(msg.into())));
    }

    /// Make the next `find_pr_by_head_branch` call return an existing PR.
    pub fn will_return_existing_pr(&self, pr_number: i64, pr_url: impl Into<String>) {
        *self.find_pr_by_head_branch_result.lock().unwrap() =
            Some(Ok(Some((pr_number, pr_url.into()))));
    }

    // --- Convenience accessors ---

    pub fn check_calls(&self) -> u32 {
        *self.check_pr_status_calls.lock().unwrap()
    }
    pub fn sync_state_calls(&self) -> u32 {
        *self.check_pr_sync_state_calls.lock().unwrap()
    }
    pub fn review_feedback_calls(&self) -> u32 {
        *self.check_pr_review_feedback_calls.lock().unwrap()
    }
    pub fn push_calls(&self) -> u32 {
        *self.push_branch_calls.lock().unwrap()
    }
    pub fn create_calls(&self) -> u32 {
        *self.create_draft_pr_calls.lock().unwrap()
    }
    pub fn mark_ready_calls(&self) -> u32 {
        *self.mark_pr_ready_calls.lock().unwrap()
    }
    pub fn update_pr_details_calls(&self) -> u32 {
        *self.update_pr_details_calls.lock().unwrap()
    }
    pub fn update_pr_base_calls(&self) -> u32 {
        *self.update_pr_base_calls.lock().unwrap()
    }
    pub fn delete_branch_calls(&self) -> u32 {
        *self.delete_remote_branch_calls.lock().unwrap()
    }
    pub fn find_pr_calls(&self) -> u32 {
        *self.find_pr_by_head_branch_calls.lock().unwrap()
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
        _wd: &Path,
        _base: &str,
        _head: &str,
        _title: &str,
        _body: &Path,
    ) -> AppResult<(i64, String)> {
        *self.create_draft_pr_calls.lock().unwrap() += 1;
        if let Some(result) = self.create_draft_pr_result.lock().unwrap().take() {
            return result;
        }
        Ok((1, "https://github.com/owner/repo/pull/1".to_string()))
    }

    async fn mark_pr_ready(&self, _wd: &Path, _pr_number: i64) -> AppResult<()> {
        *self.mark_pr_ready_calls.lock().unwrap() += 1;
        if let Some(result) = self.mark_pr_ready_result.lock().unwrap().take() {
            return result;
        }
        Ok(())
    }

    async fn update_pr_details(
        &self,
        _wd: &Path,
        _pr_number: i64,
        _title: &str,
        _body: &Path,
    ) -> AppResult<()> {
        *self.update_pr_details_calls.lock().unwrap() += 1;
        if let Some(result) = self.update_pr_details_result.lock().unwrap().take() {
            return result;
        }
        Ok(())
    }

    async fn update_pr_base(&self, _wd: &Path, _pr_number: i64, _base: &str) -> AppResult<()> {
        *self.update_pr_base_calls.lock().unwrap() += 1;
        if let Some(result) = self.update_pr_base_result.lock().unwrap().take() {
            return result;
        }
        Ok(())
    }

    async fn check_pr_status(&self, _wd: &Path, _pr_number: i64) -> AppResult<PrStatus> {
        *self.check_pr_status_calls.lock().unwrap() += 1;
        let mut q = self.status_responses.lock().unwrap();
        if let Some(result) = q.pop_front() {
            return result;
        }
        Ok(PrStatus::Open)
    }

    async fn check_pr_sync_state(&self, _wd: &Path, _pr_number: i64) -> AppResult<PrSyncState> {
        *self.check_pr_sync_state_calls.lock().unwrap() += 1;
        let mut q = self.sync_state_responses.lock().unwrap();
        if let Some(result) = q.pop_front() {
            return result;
        }
        Ok(PrSyncState {
            status: PrStatus::Open,
            merge_state_status: Some(PrMergeStateStatus::Clean),
            mergeable: Some(PrMergeableState::Mergeable),
            is_draft: false,
            head_ref_name: "feature".to_string(),
            base_ref_name: "main".to_string(),
            head_ref_oid: None,
            base_ref_oid: None,
        })
    }

    async fn check_pr_review_feedback(
        &self,
        _wd: &Path,
        _pr_number: i64,
    ) -> AppResult<Option<PrReviewFeedback>> {
        *self.check_pr_review_feedback_calls.lock().unwrap() += 1;
        let mut q = self.review_feedback_responses.lock().unwrap();
        if let Some(result) = q.pop_front() {
            return result;
        }
        Ok(None)
    }

    async fn push_branch(&self, _wd: &Path, _branch: &str) -> AppResult<()> {
        *self.push_branch_calls.lock().unwrap() += 1;
        if let Some(result) = self.push_branch_result.lock().unwrap().take() {
            return result;
        }
        Ok(())
    }

    async fn close_pr(&self, _wd: &Path, _pr_number: i64) -> AppResult<()> {
        *self.close_pr_calls.lock().unwrap() += 1;
        Ok(())
    }

    async fn delete_remote_branch(&self, _wd: &Path, _branch: &str) -> AppResult<()> {
        *self.delete_remote_branch_calls.lock().unwrap() += 1;
        Ok(())
    }

    async fn fetch_remote(&self, _wd: &Path, _branch: &str) -> AppResult<()> {
        Ok(())
    }

    async fn find_pr_by_head_branch(
        &self,
        _wd: &Path,
        _head: &str,
    ) -> AppResult<Option<(i64, String)>> {
        *self.find_pr_by_head_branch_calls.lock().unwrap() += 1;
        if let Some(result) = self.find_pr_by_head_branch_result.lock().unwrap().take() {
            return result;
        }
        Ok(None)
    }
}
