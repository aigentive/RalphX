use super::project_commands::*;
use crate::application::AppState;
use crate::commands::ExecutionState;
use crate::domain::entities::{Project, ProjectId};
use crate::domain::services::github_service::GithubConnectionState;
use crate::domain::services::{GithubServiceTrait, PrSearchResult};
use crate::infrastructure::git_auth::GitRemoteAuthConfig;
use crate::infrastructure::tool_paths::resolve_git_cli_path;
use crate::tests::mock_github_service::MockGithubService;
use std::process::Command;
use std::sync::Arc;
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri::Manager;

#[test]
fn browser_login_is_only_allowed_for_real_credential_failures() {
    assert_eq!(
        browser_login_required(GithubConnectionState::ProviderUnavailable),
        Ok(false)
    );
    assert_eq!(
        browser_login_required(GithubConnectionState::Unauthenticated),
        Ok(true)
    );
    assert_eq!(
        browser_login_required(GithubConnectionState::CredentialRejected),
        Ok(true)
    );
    assert!(browser_login_required(GithubConnectionState::ProbeFailed).is_err());
    assert!(browser_login_required(GithubConnectionState::CliUnavailable).is_err());
}

#[tokio::test]
async fn search_github_pull_requests_trims_query_clamps_limit_and_maps_results() {
    let github = Arc::new(MockGithubService::new());
    github.will_return_pull_request_search(vec![PrSearchResult {
        number: 42,
        title: "Add PR picker".to_string(),
        url: "https://github.com/owner/repo/pull/42".to_string(),
        head_ref_name: "feature/pr-picker".to_string(),
        head_ref_oid: Some("abc123".to_string()),
        base_ref_name: "main".to_string(),
        is_draft: true,
        state: Some("MERGED".to_string()),
        merged_at: Some("2026-05-22T10:00:00Z".to_string()),
        updated_at: Some("2026-05-21T10:00:00Z".to_string()),
        author_login: Some("dev".to_string()),
        assignee_logins: vec!["ops".to_string()],
        review_decision: Some("APPROVED".to_string()),
        latest_review_author_logins: vec!["reviewer".to_string()],
        review_request_logins: Vec::new(),
        is_cross_repository: false,
    }]);

    let mut state = AppState::new_test();
    state.github_service = Some(github.clone() as Arc<dyn GithubServiceTrait>);
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let mut project = Project::new(
        "PR Search".to_string(),
        temp.path().to_string_lossy().to_string(),
    );
    project.id = ProjectId::from_string("project-pr-search".to_string());
    state.project_repo.create(project).await.unwrap();
    let app = mock_builder()
        .manage(state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    let results = search_github_pull_requests(
        SearchGithubPullRequestsInput {
            project_id: "project-pr-search".to_string(),
            query: Some("  picker  ".to_string()),
            limit: Some(99),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("search should succeed");

    let state = github.state();
    assert_eq!(state.search_pull_requests_calls, 1);
    assert_eq!(
        state.last_search_pull_requests_args,
        Some((Some("picker".to_string()), 50))
    );
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].number, 42);
    assert_eq!(results[0].head_ref_name, "feature/pr-picker");
    assert_eq!(results[0].head_ref_oid.as_deref(), Some("abc123"));
    assert_eq!(results[0].base_ref_name, "main");
    assert!(results[0].is_draft);
    assert_eq!(results[0].state.as_deref(), Some("MERGED"));
    assert_eq!(
        results[0].merged_at.as_deref(),
        Some("2026-05-22T10:00:00Z")
    );
    assert_eq!(results[0].author_login.as_deref(), Some("dev"));
    assert_eq!(results[0].assignee_logins, vec!["ops"]);
    assert_eq!(results[0].review_decision.as_deref(), Some("APPROVED"));
    assert_eq!(results[0].latest_review_author_logins, vec!["reviewer"]);
    assert!(results[0].review_request_logins.is_empty());
    assert!(!results[0].is_cross_repository);
}

#[test]
fn diagnostics_response_marks_mixed_https_fetch_and_ssh_push() {
    let response = GitAuthDiagnosticsResponse::from(GitRemoteAuthConfig {
        fetch_url: Some("https://github.com/owner/repo.git".to_string()),
        push_url: Some("git@github.com:owner/repo.git".to_string()),
        github_https_credential_helper_configured: false,
    });

    assert_eq!(response.fetch_kind.as_deref(), Some("HTTPS"));
    assert_eq!(response.push_kind.as_deref(), Some("SSH"));
    assert!(response.mixed_auth_modes);
    assert!(!response.github_https_credential_helper_configured);
    assert!(response.can_switch_to_ssh);
    assert_eq!(
        response.suggested_ssh_url.as_deref(),
        Some("git@github.com:owner/repo.git")
    );
}

#[test]
fn diagnostics_response_has_no_repair_for_non_github_remote() {
    let response = GitAuthDiagnosticsResponse::from(GitRemoteAuthConfig {
        fetch_url: Some("https://gitlab.com/owner/repo.git".to_string()),
        push_url: None,
        github_https_credential_helper_configured: false,
    });

    assert_eq!(response.fetch_kind.as_deref(), Some("HTTPS"));
    assert_eq!(response.push_kind.as_deref(), Some("HTTPS"));
    assert!(!response.mixed_auth_modes);
    assert!(!response.can_switch_to_ssh);
    assert!(response.suggested_ssh_url.is_none());
}

#[test]
fn diagnostics_response_exposes_github_https_credential_helper_state() {
    let response = GitAuthDiagnosticsResponse::from(GitRemoteAuthConfig {
        fetch_url: Some("https://github.com/owner/repo.git".to_string()),
        push_url: Some("https://github.com/owner/repo.git".to_string()),
        github_https_credential_helper_configured: true,
    });

    assert_eq!(response.fetch_kind.as_deref(), Some("HTTPS"));
    assert_eq!(response.push_kind.as_deref(), Some("HTTPS"));
    assert!(response.github_https_credential_helper_configured);
}

#[tokio::test]
async fn enabling_github_pr_mode_requires_current_github_repository_capability() {
    let temporary = tempfile::tempdir().expect("temporary repository");
    assert!(Command::new(resolve_git_cli_path())
        .args(["init", "--initial-branch", "main"])
        .current_dir(temporary.path())
        .output()
        .expect("git init should run")
        .status
        .success());
    let state = AppState::new_test();
    let mut project = Project::new(
        "Local only".to_string(),
        temporary.path().to_string_lossy().to_string(),
    );
    project.id = ProjectId::from_string("project-local-only".to_string());
    let project = state.project_repo.create(project).await.unwrap();
    let app = mock_builder()
        .manage(state)
        .manage(Arc::new(ExecutionState::new()))
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    let error = update_github_pr_enabled_with_app(
        project.id.as_str().to_string(),
        true,
        app.state::<AppState>(),
        app.state::<Arc<ExecutionState>>(),
        app.handle().clone(),
    )
    .await
    .expect_err("local-only projects cannot enable GitHub PR mode");

    assert!(error.contains("supported GitHub origin push URL"));
    let persisted = app
        .state::<AppState>()
        .project_repo
        .get_by_id(&project.id)
        .await
        .unwrap()
        .unwrap();
    assert!(!persisted.github_pr_enabled);

    update_github_pr_enabled_with_app(
        project.id.as_str().to_string(),
        false,
        app.state::<AppState>(),
        app.state::<Arc<ExecutionState>>(),
        app.handle().clone(),
    )
    .await
    .expect("disabling remains allowed for a local-only project");
}

#[tokio::test]
async fn create_project_persists_worktree_parent_and_resolved_local_only_contract() {
    let temporary = tempfile::tempdir().expect("temporary project directory");
    let app = mock_builder()
        .manage(AppState::new_test())
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    let response = create_project(
        CreateProjectInput {
            name: "Picker project".to_string(),
            working_directory: temporary.path().to_string_lossy().to_string(),
            git_mode: Some("worktree".to_string()),
            base_branch: Some("main".to_string()),
            worktree_parent_directory: Some("/custom/worktrees".to_string()),
        },
        app.state::<AppState>(),
    )
    .await
    .expect("GUI project creation should bootstrap before persistence");

    assert_eq!(response.base_branch.as_deref(), Some("main"));
    assert_eq!(
        response.worktree_parent_directory.as_deref(),
        Some("/custom/worktrees")
    );
    assert!(!response.github_pr_enabled);
    let response_json = serde_json::to_value(&response).expect("response serializes");
    assert_eq!(response_json["repository_capability"]["kind"], "local_only");
    let persisted = app
        .state::<AppState>()
        .project_repo
        .get_by_id(&ProjectId::from_string(response.id))
        .await
        .expect("project lookup should succeed")
        .expect("project should persist");
    assert_eq!(persisted.base_branch.as_deref(), Some("main"));
    assert_eq!(
        persisted.worktree_parent_directory.as_deref(),
        Some("/custom/worktrees")
    );
    assert!(
        !persisted.github_pr_enabled,
        "fresh local-only GUI projects must remain opted out of PR mode"
    );
}

#[tokio::test]
async fn updating_project_preserves_the_bootstrap_resolved_unborn_branch() {
    let temporary = tempfile::tempdir().expect("temporary project directory");
    assert!(Command::new(resolve_git_cli_path())
        .args(["init", "--initial-branch", "develop"])
        .current_dir(temporary.path())
        .output()
        .expect("git init should run")
        .status
        .success());
    let state = AppState::new_test();
    let mut project = Project::new(
        "Unborn project".to_string(),
        temporary.path().to_string_lossy().to_string(),
    );
    project.id = ProjectId::from_string("project-unborn-update".to_string());
    project.base_branch = Some("develop".to_string());
    state.project_repo.create(project).await.unwrap();
    let app = mock_builder()
        .manage(state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    let response = update_project(
        "project-unborn-update".to_string(),
        UpdateProjectInput {
            name: None,
            working_directory: Some(temporary.path().to_string_lossy().to_string()),
            git_mode: None,
            base_branch: Some("main".to_string()),
            merge_validation_mode: None,
            merge_strategy: None,
            worktree_parent_directory: None,
        },
        app.state::<AppState>(),
    )
    .await
    .expect("unborn repository should preserve its symbolic branch");

    assert_eq!(response.base_branch.as_deref(), Some("develop"));
    let persisted = app
        .state::<AppState>()
        .project_repo
        .get_by_id(&ProjectId::from_string("project-unborn-update".to_string()))
        .await
        .expect("project lookup should succeed")
        .expect("project should persist");
    assert_eq!(persisted.base_branch.as_deref(), Some("develop"));
}

#[tokio::test]
async fn updating_only_the_base_branch_validates_the_current_repository() {
    let temporary = tempfile::tempdir().expect("temporary project directory");
    assert!(Command::new(resolve_git_cli_path())
        .args(["init", "--initial-branch", "main"])
        .current_dir(temporary.path())
        .output()
        .expect("git init should run")
        .status
        .success());
    assert!(Command::new(resolve_git_cli_path())
        .args([
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test User",
            "commit",
            "--allow-empty",
            "-m",
            "initial",
        ])
        .current_dir(temporary.path())
        .output()
        .expect("initial commit should run")
        .status
        .success());
    let state = AppState::new_test();
    let mut project = Project::new(
        "Validated project".to_string(),
        temporary.path().to_string_lossy().to_string(),
    );
    project.id = ProjectId::from_string("project-base-only-update".to_string());
    project.base_branch = Some("main".to_string());
    state.project_repo.create(project).await.unwrap();
    let app = mock_builder()
        .manage(state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    let error = update_project(
        "project-base-only-update".to_string(),
        UpdateProjectInput {
            name: None,
            working_directory: None,
            git_mode: None,
            base_branch: Some("missing".to_string()),
            merge_validation_mode: None,
            merge_strategy: None,
            worktree_parent_directory: None,
        },
        app.state::<AppState>(),
    )
    .await
    .expect_err("a base-only update must validate the existing repository");

    assert!(error.contains("does not exist"));
    let persisted = app
        .state::<AppState>()
        .project_repo
        .get_by_id(&ProjectId::from_string(
            "project-base-only-update".to_string(),
        ))
        .await
        .expect("project lookup should succeed")
        .expect("project should remain");
    assert_eq!(persisted.base_branch.as_deref(), Some("main"));
}

#[tokio::test]
async fn create_project_enables_pr_mode_for_a_github_capable_repository() {
    let temporary = tempfile::tempdir().expect("temporary project directory");
    let project_path = temporary.path();
    assert!(Command::new(resolve_git_cli_path())
        .args(["init", "--initial-branch", "main"])
        .current_dir(project_path)
        .output()
        .expect("git init should run")
        .status
        .success());
    assert!(Command::new(resolve_git_cli_path())
        .args([
            "remote",
            "add",
            "origin",
            "git@github.com:owner/repository.git",
        ])
        .current_dir(project_path)
        .output()
        .expect("git remote add should run")
        .status
        .success());
    let app = mock_builder()
        .manage(AppState::new_test())
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    let response = create_project(
        CreateProjectInput {
            name: "GitHub project".to_string(),
            working_directory: project_path.to_string_lossy().to_string(),
            git_mode: None,
            base_branch: None,
            worktree_parent_directory: None,
        },
        app.state::<AppState>(),
    )
    .await
    .expect("GUI project creation should retain GitHub PR capability");

    assert!(response.github_pr_enabled);
    let persisted = app
        .state::<AppState>()
        .project_repo
        .get_by_id(&ProjectId::from_string(response.id))
        .await
        .expect("project lookup should succeed")
        .expect("project should persist");
    assert!(persisted.github_pr_enabled);
}

#[test]
fn gh_web_login_args_use_browser_flow_and_ssh_protocol() {
    assert_eq!(
        gh_web_login_args(),
        [
            "auth",
            "login",
            "--hostname",
            "github.com",
            "--git-protocol",
            "ssh",
            "--web",
            "--skip-ssh-key"
        ]
    );
}

#[test]
fn parses_gh_web_login_code_and_url_lines() {
    let code = parse_gh_auth_login_prompt("! First copy your one-time code: F308-C82B")
        .expect("code line should parse");
    assert_eq!(code.code.as_deref(), Some("F308-C82B"));
    assert!(code.url.is_none());

    let url = parse_gh_auth_login_prompt(
        "Open this URL to continue in your web browser: https://github.com/login/device",
    )
    .expect("url line should parse");
    assert!(url.code.is_none());
    assert_eq!(url.url.as_deref(), Some("https://github.com/login/device"));
}

#[tokio::test]
async fn git_branch_commands_use_async_git_service_paths() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let repo = temp_dir.path();
    Command::new(resolve_git_cli_path())
        .args(["init", "-b", "main"])
        .current_dir(repo)
        .output()
        .expect("git init should run");
    Command::new(resolve_git_cli_path())
        .args(["config", "user.email", "test@example.com"])
        .current_dir(repo)
        .output()
        .expect("git config should run");
    Command::new(resolve_git_cli_path())
        .args(["config", "user.name", "Test User"])
        .current_dir(repo)
        .output()
        .expect("git config should run");
    std::fs::write(repo.join("README.md"), "base\n").expect("fixture should be written");
    Command::new(resolve_git_cli_path())
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .expect("git add should run");
    Command::new(resolve_git_cli_path())
        .args(["commit", "-m", "base"])
        .current_dir(repo)
        .output()
        .expect("git commit should run");
    Command::new(resolve_git_cli_path())
        .args(["branch", "feature/current"])
        .current_dir(repo)
        .output()
        .expect("git branch should run");

    let current = get_git_current_branch(repo.to_string_lossy().to_string())
        .await
        .expect("current branch should load");
    let branches = get_git_branches(repo.to_string_lossy().to_string())
        .await
        .expect("branches should load");

    assert_eq!(current, "main");
    assert_eq!(branches.first().map(String::as_str), Some("main"));
    assert!(branches.contains(&"feature/current".to_string()));

    Command::new(resolve_git_cli_path())
        .args(["checkout", "--detach", "HEAD"])
        .current_dir(repo)
        .output()
        .expect("git detached checkout should run");
    assert!(get_git_current_branch(repo.to_string_lossy().to_string())
        .await
        .expect_err("detached HEAD should not return a local branch")
        .contains("Repository is not currently on a local branch"));
}

#[tokio::test]
async fn git_branch_commands_report_missing_directory() {
    let missing = tempfile::tempdir()
        .expect("tempdir should be created")
        .path()
        .join("missing");
    let missing = missing.to_string_lossy().to_string();

    assert!(get_git_current_branch(missing.clone())
        .await
        .expect_err("missing current branch directory should fail")
        .contains("Directory does not exist"));
    assert!(get_git_branches(missing)
        .await
        .expect_err("missing branches directory should fail")
        .contains("Directory does not exist"));
}

#[tokio::test]
async fn get_git_remote_url_skips_missing_and_non_github_remotes() {
    let tmp = tempfile::tempdir().expect("tempdir should be created");
    let repo = tmp.path();
    Command::new(resolve_git_cli_path())
        .args(["init"])
        .current_dir(repo)
        .output()
        .expect("git init should run");

    let state = AppState::new_test();
    let mut project = Project::new("Remote".to_string(), repo.to_string_lossy().to_string());
    project.id = ProjectId::from_string("project-remote-test".to_string());
    state.project_repo.create(project).await.unwrap();

    let app = mock_builder()
        .manage(state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    let result = get_git_remote_url("project-remote-test".to_string(), app.state::<AppState>())
        .await
        .expect("get_git_remote_url should succeed with missing remote");
    assert!(result.is_none());

    Command::new(resolve_git_cli_path())
        .args([
            "remote",
            "add",
            "origin",
            "https://gitlab.com/aigentive/test-repo.git",
        ])
        .current_dir(repo)
        .output()
        .expect("git remote add should run");

    let gitlab = get_git_remote_url("project-remote-test".to_string(), app.state::<AppState>())
        .await
        .expect("get_git_remote_url should succeed for non-github remote");
    assert!(gitlab.is_none());

    Command::new(resolve_git_cli_path())
        .args([
            "remote",
            "set-url",
            "origin",
            "https://github.com/aigentive/test-repo.git",
        ])
        .current_dir(repo)
        .output()
        .expect("git remote set-url should run");

    let github = get_git_remote_url("project-remote-test".to_string(), app.state::<AppState>())
        .await
        .expect("get_git_remote_url should succeed for github remote");
    assert_eq!(
        github,
        Some("https://github.com/aigentive/test-repo.git".to_string())
    );
}

#[tokio::test]
async fn switch_git_origin_to_ssh_rejects_non_convertible_origin() {
    let tmp = tempfile::tempdir().expect("tempdir should be created");
    let repo = tmp.path();
    Command::new(resolve_git_cli_path())
        .args(["init"])
        .current_dir(repo)
        .output()
        .expect("git init should run");

    Command::new(resolve_git_cli_path())
        .args([
            "remote",
            "add",
            "origin",
            "https://bitbucket.org/aigentive/test-repo.git",
        ])
        .current_dir(repo)
        .output()
        .expect("git remote add should run");

    let state = AppState::new_test();
    let mut project = Project::new("Remote".to_string(), repo.to_string_lossy().to_string());
    project.id = ProjectId::from_string("project-remote-switch".to_string());
    state.project_repo.create(project).await.unwrap();

    let app = mock_builder()
        .manage(state)
        .build(mock_context(noop_assets()))
        .expect("mock app should build");

    let result =
        switch_git_origin_to_ssh("project-remote-switch".to_string(), app.state::<AppState>())
            .await;

    let error = result.expect_err("non-github remote should not be convertible");
    assert!(
        error.contains("Origin is not a convertible GitHub HTTPS remote"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn run_git_config_command_fails_if_directory_is_missing() {
    let missing = tempfile::tempdir()
        .expect("tempdir should be created")
        .path()
        .join("missing-dir");
    let error = run_git_config_command(&missing, &["remote", "get-url", "origin"])
        .await
        .expect_err("command should fail when current dir is missing");
    assert!(
        error.contains("Failed to spawn git") || error.contains("git config command timed out"),
        "expected spawn or timeout failure, got {error}"
    );
}

#[test]
fn gh_auth_login_prompt_exposes_code_and_url_together() {
    let prompt = parse_gh_auth_login_prompt(
            "Open this URL to continue: one-time code: ABCD-EFGH\nweb browser: https://github.com/login/device",
        )
        .expect("prompt should parse");
    assert_eq!(prompt.code, Some("ABCD-EFGH".to_string()));
    assert_eq!(
        prompt.url,
        Some("https://github.com/login/device".to_string())
    );
}
