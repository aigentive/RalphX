use super::*;

fn pr_search_result_with_state(state: Option<&str>) -> PrSearchResult {
    PrSearchResult {
        number: 42,
        title: "PR state helper".to_string(),
        url: "https://github.com/owner/repo/pull/42".to_string(),
        head_ref_name: "feature/state-helper".to_string(),
        head_ref_oid: Some("abc123".to_string()),
        base_ref_name: "main".to_string(),
        is_draft: false,
        state: state.map(str::to_string),
        merged_at: None,
        updated_at: None,
        author_login: None,
        assignee_logins: Vec::new(),
        review_decision: None,
        latest_review_author_logins: Vec::new(),
        review_request_logins: Vec::new(),
        is_cross_repository: false,
    }
}

#[test]
fn pr_search_result_state_predicates_are_case_insensitive_and_keep_unknown_open() {
    let open = pr_search_result_with_state(Some("OPEN"));
    let lowercase_open = pr_search_result_with_state(Some("open"));
    let merged = pr_search_result_with_state(Some("MERGED"));
    let closed = pr_search_result_with_state(Some("CLOSED"));
    let unknown = pr_search_result_with_state(None);

    assert!(open.is_open());
    assert!(lowercase_open.is_open());
    assert!(unknown.is_open());
    assert!(!merged.is_open());
    assert!(!closed.is_open());

    assert!(merged.is_merged());
    assert!(!open.is_merged());
    assert!(!closed.is_merged());
    assert!(!unknown.is_merged());

    assert!(closed.is_closed_unmerged());
    assert!(!open.is_closed_unmerged());
    assert!(!merged.is_closed_unmerged());
    assert!(!unknown.is_closed_unmerged());
}

struct DefaultOnlyGithubService;

#[async_trait]
impl GithubServiceTrait for DefaultOnlyGithubService {
    async fn create_issue(
        &self,
        _working_dir: &Path,
        _repository: &str,
        _title: &str,
        _body_file: &Path,
    ) -> AppResult<String> {
        unimplemented!("not needed for default annotation coverage")
    }

    async fn create_draft_pr(
        &self,
        _working_dir: &Path,
        _base: &str,
        _head: &str,
        _title: &str,
        _body_file: &Path,
    ) -> AppResult<(i64, String)> {
        unimplemented!("not needed for default annotation coverage")
    }

    async fn mark_pr_ready(&self, _working_dir: &Path, _pr_number: i64) -> AppResult<()> {
        unimplemented!("not needed for default annotation coverage")
    }

    async fn update_pr_details(
        &self,
        _working_dir: &Path,
        _pr_number: i64,
        _title: &str,
        _body_file: &Path,
    ) -> AppResult<()> {
        unimplemented!("not needed for default annotation coverage")
    }

    async fn update_pr_base(
        &self,
        _working_dir: &Path,
        _pr_number: i64,
        _base: &str,
    ) -> AppResult<()> {
        unimplemented!("not needed for default annotation coverage")
    }

    async fn check_pr_status(&self, _working_dir: &Path, _pr_number: i64) -> AppResult<PrStatus> {
        unimplemented!("not needed for default annotation coverage")
    }

    async fn check_pr_sync_state(
        &self,
        _working_dir: &Path,
        _pr_number: i64,
    ) -> AppResult<PrSyncState> {
        unimplemented!("not needed for default annotation coverage")
    }

    async fn push_branch(&self, _working_dir: &Path, _branch: &str) -> AppResult<()> {
        unimplemented!("not needed for default annotation coverage")
    }

    async fn close_pr(&self, _working_dir: &Path, _pr_number: i64) -> AppResult<()> {
        unimplemented!("not needed for default annotation coverage")
    }

    async fn delete_remote_branch(&self, _working_dir: &Path, _branch: &str) -> AppResult<()> {
        unimplemented!("not needed for default annotation coverage")
    }

    async fn fetch_remote(&self, _working_dir: &Path, _branch: &str) -> AppResult<()> {
        unimplemented!("not needed for default annotation coverage")
    }

    async fn find_pr_by_head_branch(
        &self,
        _working_dir: &Path,
        _head: &str,
    ) -> AppResult<Option<(i64, String)>> {
        unimplemented!("not needed for default annotation coverage")
    }

    async fn get_pr_diff_patch(
        &self,
        _working_dir: &Path,
        _pr_number: i64,
        _pr_url: Option<&str>,
    ) -> AppResult<String> {
        unimplemented!("not needed for default annotation coverage")
    }
}

#[tokio::test]
async fn default_pr_diff_annotations_are_empty_for_pr_number() {
    let service = DefaultOnlyGithubService;

    let annotations = service
        .fetch_pr_diff_annotations(Path::new("/tmp"), 123)
        .await
        .expect("default annotations should be empty");

    assert_eq!(annotations, PrDiffAnnotations::empty(123));
}

#[tokio::test]
async fn default_pr_auto_merge_state_is_unknown() {
    let service = DefaultOnlyGithubService;

    let state = service
        .fetch_pr_auto_merge_state(Path::new("/tmp"), 123)
        .await
        .expect("default auto-merge state should be conservative");

    assert_eq!(state, None);
}

#[tokio::test]
async fn default_metadata_patch_rejects_an_empty_patch_before_unsupported() {
    let service = DefaultOnlyGithubService;

    let result = service
        .patch_pr_metadata(Path::new("/tmp"), 123, None, None)
        .await;

    assert!(
        matches!(result, Err(AppError::Validation(message)) if message == EMPTY_PR_METADATA_PATCH_VALIDATION_ERROR)
    );
}
