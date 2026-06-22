use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use crate::application::git_service::GitService;
use crate::application::ticket_canonical_branch::{
    canonical_branch_name, ensure_ticket_canonical_branch, sanitize_issue_key_slug,
};
use crate::application::AppState;
use crate::domain::entities::{Project, ProjectId};
use crate::error::AppError;
use crate::tests::mock_github_service::MockGithubService;

fn git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Initialize a real git repo on branch `main` with one commit and an origin remote.
fn init_real_repo() -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().expect("tempdir");
    let origin = temp.path().join("origin.git");
    let repo = temp.path().join("repo");

    let output = Command::new("git")
        .args(["init", "--bare", origin.to_str().expect("origin path")])
        .output()
        .expect("git init bare should run");
    assert!(output.status.success());
    let output = Command::new("git")
        .args(["init", "-b", "main", repo.to_str().expect("repo path")])
        .output()
        .expect("git init should run");
    assert!(output.status.success());

    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test User"]);
    std::fs::write(repo.join("README.md"), "main\n").expect("write readme");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "initial"]);
    git(&repo, &["remote", "add", "origin", origin.to_str().unwrap()]);

    (temp, repo)
}

/// Build an `AppState` with a project pointing at `repo` and a mock GitHub service.
async fn state_with_project(repo: &Path) -> (AppState, ProjectId, Arc<MockGithubService>) {
    let mut state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    state.github_service = Some(github.clone());

    let mut project = Project::new(
        "Ticket Project".to_string(),
        repo.to_string_lossy().into_owned(),
    );
    project.base_branch = Some("main".to_string());
    let project_id = project.id.clone();
    state.project_repo.create(project).await.expect("seed project");

    (state, project_id, github)
}

// ── Pure helper tests ───────────────────────────────────────────────────────

#[test]
fn sanitize_produces_readable_lowercase_slug() {
    assert_eq!(
        sanitize_issue_key_slug("WISE-24"),
        Some("wise-24".to_string())
    );
    assert_eq!(
        sanitize_issue_key_slug("RX_42 Foo"),
        Some("rx-42-foo".to_string())
    );
}

#[test]
fn sanitize_collapses_and_trims_separators() {
    assert_eq!(
        sanitize_issue_key_slug("--a//b..c--"),
        Some("a-b-c".to_string())
    );
}

#[test]
fn sanitize_rejects_all_separator_input() {
    assert_eq!(sanitize_issue_key_slug("///"), None);
    assert_eq!(sanitize_issue_key_slug(""), None);
}

#[test]
fn canonical_branch_name_is_prefixed_and_readable() {
    assert_eq!(
        canonical_branch_name("linear", "WISE-24"),
        Some("ralphx/ticket/linear-wise-24".to_string())
    );
    assert_eq!(
        canonical_branch_name("jira", "RX-42"),
        Some("ralphx/ticket/jira-rx-42".to_string())
    );
}

// ── check-ref-format validation ─────────────────────────────────────────────

#[tokio::test]
async fn check_ref_format_accepts_normal_canonical_name() {
    let (_temp, repo) = init_real_repo();
    assert!(GitService::check_ref_format(&repo, "ralphx/ticket/linear-wise-24")
        .await
        .unwrap());
}

#[tokio::test]
async fn check_ref_format_rejects_traversal_and_malformed_names() {
    let (_temp, repo) = init_real_repo();
    for bad in [
        "ralphx/ticket/../escape",
        "-leading-dash",
        "ralphx/ticket/foo.lock",
        "ralphx/ticket/has space",
        "ralphx/ticket/a..b",
        "ralphx/ticket/trailing/",
        "",
    ] {
        assert!(
            !GitService::check_ref_format(&repo, bad).await.unwrap(),
            "expected reject for [{bad}]"
        );
    }
}

// ── ensure_ticket_canonical_branch flow ─────────────────────────────────────

#[tokio::test]
async fn first_call_creates_branch_pushes_once_and_marks_pushed() {
    let (_temp, repo) = init_real_repo();
    let (state, project_id, github) = state_with_project(&repo).await;

    let canonical = ensure_ticket_canonical_branch(&state, &project_id, "linear", "WISE-24")
        .await
        .expect("first call should establish the canonical branch");

    assert_eq!(canonical.branch_name, "ralphx/ticket/linear-wise-24");
    assert_eq!(canonical.base_branch, "main");
    assert!(canonical.base_commit.is_some());
    assert!(canonical.origin_pushed, "origin_pushed only true after push");
    assert!(!canonical.terminal);

    // Branch exists locally in the main checkout (and is NOT checked out).
    assert!(GitService::branch_exists(&repo, "ralphx/ticket/linear-wise-24")
        .await
        .unwrap());
    let current = GitService::get_current_branch(&repo).await.unwrap();
    assert_eq!(current, "main", "canonical branch must not be checked out");

    // Pushed exactly once with the canonical branch name.
    assert_eq!(github.state().push_branch_calls, 1);
    assert_eq!(
        github.state().last_push_branch_name.as_deref(),
        Some("ralphx/ticket/linear-wise-24")
    );

    // Persisted with origin_pushed = true.
    let stored = state
        .ticket_canonical_branch_repo
        .get(&project_id, "linear", "WISE-24")
        .await
        .unwrap()
        .unwrap();
    assert!(stored.origin_pushed);
}

#[tokio::test]
async fn second_call_is_idempotent_and_does_not_push_again() {
    let (_temp, repo) = init_real_repo();
    let (state, project_id, github) = state_with_project(&repo).await;

    let first = ensure_ticket_canonical_branch(&state, &project_id, "linear", "WISE-24")
        .await
        .unwrap();
    assert_eq!(github.state().push_branch_calls, 1);

    let second = ensure_ticket_canonical_branch(&state, &project_id, "linear", "WISE-24")
        .await
        .unwrap();

    assert_eq!(second.branch_name, first.branch_name);
    assert_eq!(
        github.state().push_branch_calls,
        1,
        "idempotent reuse must not push again"
    );
}

#[tokio::test]
async fn terminal_row_returns_blocking_error() {
    let (_temp, repo) = init_real_repo();
    let (state, project_id, github) = state_with_project(&repo).await;

    ensure_ticket_canonical_branch(&state, &project_id, "linear", "WISE-24")
        .await
        .unwrap();
    state
        .ticket_canonical_branch_repo
        .mark_terminal(&project_id, "linear", "WISE-24")
        .await
        .unwrap();

    let calls_before = github.state().push_branch_calls;
    let error = ensure_ticket_canonical_branch(&state, &project_id, "linear", "WISE-24")
        .await
        .expect_err("terminal canonical branch must block");

    assert!(
        matches!(error, AppError::ExecutionBlocked(_)),
        "expected ExecutionBlocked, got {error:?}"
    );
    assert_eq!(
        github.state().push_branch_calls,
        calls_before,
        "terminal block must not push"
    );
}

#[tokio::test]
async fn unpushed_row_completes_push_on_next_call() {
    let (_temp, repo) = init_real_repo();
    let (state, project_id, github) = state_with_project(&repo).await;

    // First attempt: push fails, so origin_pushed must stay false.
    github.state().push_branch_result = Some(Err(AppError::GitOperation("network down".to_string())));
    let error = ensure_ticket_canonical_branch(&state, &project_id, "linear", "WISE-24")
        .await
        .expect_err("failed push should surface as error");
    assert!(matches!(error, AppError::GitOperation(_)));

    // The push failure happens before upsert, so no row is persisted yet; the
    // next call re-establishes from scratch and pushes successfully.
    let recovered = ensure_ticket_canonical_branch(&state, &project_id, "linear", "WISE-24")
        .await
        .expect("retry should establish and push");
    assert!(recovered.origin_pushed);
    assert_eq!(
        github.state().last_push_branch_name.as_deref(),
        Some("ralphx/ticket/linear-wise-24")
    );
}

#[tokio::test]
async fn invalid_issue_key_is_rejected_before_git_use() {
    let (_temp, repo) = init_real_repo();
    let (state, project_id, github) = state_with_project(&repo).await;

    // An all-separator issue key yields no slug, so no branch name is derivable.
    let error = ensure_ticket_canonical_branch(&state, &project_id, "linear", "///")
        .await
        .expect_err("invalid issue key must be rejected");

    assert!(
        matches!(error, AppError::Validation(_)),
        "expected Validation, got {error:?}"
    );
    assert_eq!(
        github.state().push_branch_calls,
        0,
        "invalid key must never reach push"
    );
}

#[tokio::test]
async fn missing_project_returns_typed_error() {
    let mut state = AppState::new_test();
    state.github_service = Some(Arc::new(MockGithubService::new()));
    let project_id = ProjectId::from_string("does-not-exist".to_string());

    let error = ensure_ticket_canonical_branch(&state, &project_id, "linear", "WISE-24")
        .await
        .expect_err("missing project must error");

    assert!(
        matches!(error, AppError::ProjectNotFound(_)),
        "expected ProjectNotFound, got {error:?}"
    );
}

// ── complete_unpushed_canonical_branch recovery path ─────────────────────────

use crate::domain::entities::TicketCanonicalBranch;

/// Seed a persisted-but-unpushed canonical branch row (origin_pushed = false),
/// optionally also creating the local branch in the main checkout.
async fn seed_unpushed_row(
    state: &AppState,
    project_id: &ProjectId,
    repo: &Path,
    branch_name: &str,
    create_local_branch: bool,
) {
    if create_local_branch {
        // Create the branch without checking it out so the recovery path's
        // `branch_exists` check is satisfied.
        git(repo, &["branch", branch_name, "main"]);
    }
    let row = TicketCanonicalBranch::new(
        project_id.clone(),
        "linear",
        "WISE-24",
        branch_name,
        "main",
        None,
        chrono::Utc::now(),
    );
    // origin_pushed defaults to false on `new`, which is exactly the
    // partially-established state we want to recover from.
    assert!(!row.origin_pushed);
    state
        .ticket_canonical_branch_repo
        .upsert(row)
        .await
        .expect("seed unpushed row");
}

#[tokio::test]
async fn unpushed_existing_row_with_local_branch_completes_push_and_marks_pushed() {
    let (_temp, repo) = init_real_repo();
    let (state, project_id, github) = state_with_project(&repo).await;
    let branch_name = "ralphx/ticket/linear-wise-24";
    seed_unpushed_row(&state, &project_id, &repo, branch_name, true).await;

    let recovered = ensure_ticket_canonical_branch(&state, &project_id, "linear", "WISE-24")
        .await
        .expect("recovery should push and mark the row");

    assert!(recovered.origin_pushed, "row must be flagged pushed after recovery");
    assert_eq!(recovered.branch_name, branch_name);
    assert_eq!(github.state().push_branch_calls, 1);
    assert_eq!(
        github.state().last_push_branch_name.as_deref(),
        Some(branch_name)
    );

    // The persisted row is now marked pushed, so a follow-up call is idempotent.
    let stored = state
        .ticket_canonical_branch_repo
        .get(&project_id, "linear", "WISE-24")
        .await
        .unwrap()
        .unwrap();
    assert!(stored.origin_pushed);
}

#[tokio::test]
async fn unpushed_existing_row_recreates_missing_local_branch_before_push() {
    let (_temp, repo) = init_real_repo();
    let (state, project_id, github) = state_with_project(&repo).await;
    let branch_name = "ralphx/ticket/linear-wise-24";
    // Row exists but the local branch is gone (never created).
    seed_unpushed_row(&state, &project_id, &repo, branch_name, false).await;
    assert!(!GitService::branch_exists(&repo, branch_name).await.unwrap());

    let recovered = ensure_ticket_canonical_branch(&state, &project_id, "linear", "WISE-24")
        .await
        .expect("recovery should recreate the branch and push");

    assert!(recovered.origin_pushed);
    // The branch was recreated in the main checkout from its recorded base.
    assert!(GitService::branch_exists(&repo, branch_name).await.unwrap());
    assert_eq!(github.state().push_branch_calls, 1);
}

#[tokio::test]
async fn unpushed_existing_row_keeps_unpushed_when_push_fails() {
    let (_temp, repo) = init_real_repo();
    let (state, project_id, github) = state_with_project(&repo).await;
    let branch_name = "ralphx/ticket/linear-wise-24";
    seed_unpushed_row(&state, &project_id, &repo, branch_name, true).await;

    // Force the recovery push to fail; the row must stay unpushed and the error
    // surfaces to the caller.
    github.state().push_branch_result = Some(Err(AppError::GitOperation("offline".to_string())));
    let error = ensure_ticket_canonical_branch(&state, &project_id, "linear", "WISE-24")
        .await
        .expect_err("a failed recovery push surfaces an error");
    assert!(matches!(error, AppError::GitOperation(_)), "got {error:?}");

    // The persisted row is still unpushed because mark_origin_pushed never ran.
    let stored = state
        .ticket_canonical_branch_repo
        .get(&project_id, "linear", "WISE-24")
        .await
        .unwrap()
        .unwrap();
    assert!(!stored.origin_pushed, "row must remain unpushed after failed push");
}

#[tokio::test]
async fn missing_github_service_blocks_canonical_branch_push() {
    let (_temp, repo) = init_real_repo();
    let (mut state, project_id, _github) = state_with_project(&repo).await;
    // Drop the GitHub service: the canonical-branch contract requires a push, so
    // its absence is a hard error rather than a silent local-only branch.
    state.github_service = None;

    let error = ensure_ticket_canonical_branch(&state, &project_id, "linear", "WISE-24")
        .await
        .expect_err("missing GitHub service must error");

    assert!(matches!(error, AppError::GitOperation(_)), "got {error:?}");
}
