use super::agent_conversation_workspace_base::{
    apply_workspace_base_resolution, resolve_workspace_base,
    resolve_workspace_base_from_local_snapshot, resolve_workspace_base_with_github,
    BaseResolutionResult, BaseStatus, BLOCK_REASON_MISSING_BASE_COMMIT, BLOCK_REASON_NOT_CONTAINED,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentWorkspaceSourcePullRequest,
    ChatConversationId, IdeationAnalysisBaseRefKind, Project,
};
use crate::domain::services::PrStatus;
use crate::error::AppError;
use crate::tests::mock_github_service::MockGithubService;
use std::path::Path;
use std::process::Command;

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command should spawn");
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn setup_remote_repo() -> (tempfile::TempDir, Project, String) {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let remote = temp.path().join("origin.git");
    let repo = temp.path().join("repo");

    Command::new("git")
        .args(["init", "--bare", remote.to_str().unwrap()])
        .output()
        .expect("bare origin should be created");
    Command::new("git")
        .args(["clone", remote.to_str().unwrap(), repo.to_str().unwrap()])
        .output()
        .expect("repo should clone");
    git(&repo, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test User"]);
    std::fs::write(repo.join("README.md"), "base\n").expect("base file should be written");
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-m", "base"]);
    git(&repo, &["push", "-u", "origin", "main"]);
    let main_sha = git(&repo, &["rev-parse", "HEAD"]);

    let mut project = Project::new(
        "Workspace Base".to_string(),
        repo.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    (temp, project, main_sha)
}

fn workspace(base_ref: &str, base_commit: Option<String>) -> AgentConversationWorkspace {
    AgentConversationWorkspace::new(
        ChatConversationId::from_string("conversation-base-test"),
        crate::domain::entities::ProjectId::from_string("project-base-test".to_string()),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::CurrentBranch,
        base_ref.to_string(),
        Some(format!("Current branch ({base_ref})")),
        base_commit,
        "ralphx/test/agent".to_string(),
        "/tmp/agent-workspace".to_string(),
    )
}

fn source_pull_request(
    number: i64,
    head_ref_name: &str,
    base_ref_name: &str,
) -> AgentWorkspaceSourcePullRequest {
    AgentWorkspaceSourcePullRequest {
        number,
        url: Some(format!("https://github.com/acme/demo/pull/{number}")),
        title: Some(format!("PR {number}")),
        head_ref_name: head_ref_name.to_string(),
        base_ref_name: Some(base_ref_name.to_string()),
        head_ref_oid: None,
    }
}

#[test]
fn base_status_strings_match_api_contract() {
    assert_eq!(BaseStatus::Valid.as_str(), "valid");
    assert_eq!(BaseStatus::Retargeted.as_str(), "retargeted");
    assert_eq!(BaseStatus::Blocked.as_str(), "blocked");
}

#[test]
fn blocked_base_resolution_accessors_return_validation_errors() {
    let blocked = BaseResolutionResult {
        status: BaseStatus::Blocked,
        old_base_ref: "feature/deleted-base".to_string(),
        effective_base_ref: None,
        effective_checkout_ref: None,
        effective_base_commit: None,
        display_name: None,
        block_reason: Some("custom block reason".to_string()),
        merged_source_pull_request_number: None,
    };

    assert!(blocked
        .effective_base_ref()
        .expect_err("blocked base ref should error")
        .to_string()
        .contains("custom block reason"));
    assert!(blocked
        .effective_checkout_ref()
        .expect_err("blocked checkout ref should error")
        .to_string()
        .contains("custom block reason"));
    assert!(blocked
        .effective_base_commit()
        .expect_err("blocked base commit should error")
        .to_string()
        .contains("custom block reason"));

    let blocked_without_reason = BaseResolutionResult {
        block_reason: None,
        ..blocked
    };
    assert!(blocked_without_reason
        .effective_base_ref()
        .expect_err("blocked base ref should use fallback reason")
        .to_string()
        .contains("Agent workspace base is blocked"));
}

#[test]
fn apply_workspace_base_resolution_updates_retargeted_metadata() {
    let mut target = workspace("feature/deleted-base", Some("old-base".to_string()));
    let resolution = BaseResolutionResult {
        status: BaseStatus::Retargeted,
        old_base_ref: "feature/deleted-base".to_string(),
        effective_base_ref: Some("main".to_string()),
        effective_checkout_ref: Some("origin/main".to_string()),
        effective_base_commit: Some("new-main-sha".to_string()),
        display_name: Some("Project default (main)".to_string()),
        block_reason: None,
        merged_source_pull_request_number: None,
    };

    let changed = apply_workspace_base_resolution(&mut target, &resolution)
        .expect("retargeted metadata should apply");

    assert!(changed);
    assert_eq!(
        target.base_ref_kind,
        IdeationAnalysisBaseRefKind::ProjectDefault
    );
    assert_eq!(target.base_ref, "main");
    assert_eq!(
        target.base_display_name.as_deref(),
        Some("Project default (main)")
    );
    assert_eq!(target.base_commit.as_deref(), Some("new-main-sha"));
}

#[test]
fn apply_workspace_base_resolution_clears_merged_source_pull_request() {
    let mut target = workspace("feature/source-pr", Some("old-base".to_string()));
    target.source_pull_request = Some(source_pull_request(42, "feature/source-pr", "main"));
    let resolution = BaseResolutionResult::retargeted_merged_source_pull_request(
        "feature/source-pr".to_string(),
        "main".to_string(),
        "origin/main".to_string(),
        "new-main-sha".to_string(),
        42,
    );

    apply_workspace_base_resolution(&mut target, &resolution)
        .expect("merged source PR retarget should apply");

    assert!(target.source_pull_request.is_none());
}

#[test]
fn apply_workspace_base_resolution_keeps_valid_and_rejects_blocked() {
    let mut target = workspace("main", Some("main-sha".to_string()));
    let valid = BaseResolutionResult::valid(
        "main".to_string(),
        "origin/main".to_string(),
        "main-sha".to_string(),
        Some("Project default (main)".to_string()),
    );
    let changed = apply_workspace_base_resolution(&mut target, &valid)
        .expect("valid metadata should be accepted");
    assert!(!changed);
    assert_eq!(target.base_ref, "main");

    let blocked = BaseResolutionResult {
        status: BaseStatus::Blocked,
        old_base_ref: "feature/deleted-base".to_string(),
        effective_base_ref: None,
        effective_checkout_ref: None,
        effective_base_commit: None,
        display_name: None,
        block_reason: Some("cannot verify base".to_string()),
        merged_source_pull_request_number: None,
    };
    let error = apply_workspace_base_resolution(&mut target, &blocked)
        .expect_err("blocked metadata should not apply");
    assert!(error.to_string().contains("cannot verify base"));
}

#[tokio::test]
async fn resolve_workspace_base_blocks_empty_saved_base_ref() {
    let (_temp, project, main_sha) = setup_remote_repo();
    let mut workspace = workspace(" origin/ ", Some(main_sha));
    workspace.project_id = project.id.clone();

    let resolution = resolve_workspace_base(&project, &workspace)
        .await
        .expect("base should classify as blocked");

    assert_eq!(resolution.status, BaseStatus::Blocked);
    assert_eq!(
        resolution.block_reason.as_deref(),
        Some("Agent conversation workspace base ref is empty")
    );
}

#[tokio::test]
async fn resolve_workspace_base_blocks_when_configured_default_branch_is_missing() {
    let (_temp, mut project, main_sha) = setup_remote_repo();
    project.base_branch = Some("missing-default".to_string());
    let mut workspace = workspace("feature/deleted-base", Some(main_sha));
    workspace.project_id = project.id.clone();

    let resolution = resolve_workspace_base(&project, &workspace)
        .await
        .expect("base should classify as blocked");

    assert_eq!(resolution.status, BaseStatus::Blocked);
    assert_eq!(
        resolution.block_reason.as_deref(),
        Some("Project default branch 'missing-default' cannot be resolved")
    );
}

#[tokio::test]
async fn resolve_workspace_base_keeps_existing_remote_base_valid() {
    let (_temp, project, main_sha) = setup_remote_repo();
    let repo = Path::new(&project.working_directory);
    git(repo, &["checkout", "-b", "feature/existing-base"]);
    git(repo, &["push", "-u", "origin", "feature/existing-base"]);
    git(repo, &["checkout", "main"]);

    let mut workspace = workspace("feature/existing-base", Some(main_sha));
    workspace.project_id = project.id.clone();

    let resolution = resolve_workspace_base(&project, &workspace)
        .await
        .expect("base should resolve");

    assert_eq!(resolution.status, BaseStatus::Valid);
    assert_eq!(
        resolution.effective_base_ref.as_deref(),
        Some("feature/existing-base")
    );
    assert_eq!(
        resolution.effective_checkout_ref.as_deref(),
        Some("origin/feature/existing-base")
    );
}

#[tokio::test]
async fn resolve_workspace_base_retargets_merged_source_pull_request_to_merge_target() {
    let (_temp, project, main_sha) = setup_remote_repo();
    let repo = Path::new(&project.working_directory);
    git(repo, &["checkout", "-b", "feature/source-pr"]);
    git(repo, &["push", "-u", "origin", "feature/source-pr"]);
    git(repo, &["checkout", "main"]);
    let mut target = workspace("origin/feature/source-pr", Some(main_sha));
    target.project_id = project.id.clone();
    target.source_pull_request = Some(source_pull_request(42, "feature/source-pr", "main"));
    let github = MockGithubService::new();
    github.will_return_status(PrStatus::Merged {
        merge_commit_sha: Some("merge-sha".to_string()),
        merged_at: Some("2026-08-01T00:00:00Z".to_string()),
    });

    let resolution = resolve_workspace_base_with_github(&project, &target, Some(&github))
        .await
        .expect("merged source PR should retarget");

    assert_eq!(resolution.status, BaseStatus::Retargeted);
    assert_eq!(resolution.old_base_ref, "feature/source-pr");
    assert_eq!(resolution.effective_base_ref.as_deref(), Some("main"));
    assert_eq!(
        resolution.effective_checkout_ref.as_deref(),
        Some("origin/main")
    );
    assert_eq!(
        resolution.display_name.as_deref(),
        Some("main (PR #42 merged)")
    );
    assert!(resolution.retargeted_from_merged_source_pull_request());
}

#[tokio::test]
async fn resolve_workspace_base_keeps_open_source_pull_request_unchanged() {
    let (_temp, project, main_sha) = setup_remote_repo();
    let repo = Path::new(&project.working_directory);
    git(repo, &["checkout", "-b", "feature/source-pr"]);
    git(repo, &["push", "-u", "origin", "feature/source-pr"]);
    git(repo, &["checkout", "main"]);
    let mut target = workspace("feature/source-pr", Some(main_sha));
    target.project_id = project.id.clone();
    target.source_pull_request = Some(source_pull_request(42, "feature/source-pr", "main"));
    let github = MockGithubService::new();
    github.will_return_status(PrStatus::Open);

    let resolution = resolve_workspace_base_with_github(&project, &target, Some(&github))
        .await
        .expect("open source PR should keep its selected base");

    assert_eq!(resolution.status, BaseStatus::Valid);
    assert_eq!(
        resolution.effective_base_ref.as_deref(),
        Some("feature/source-pr")
    );
    assert_eq!(github.state().check_pr_status_calls, 1);
}

#[tokio::test]
async fn resolve_workspace_base_keeps_source_pull_request_unchanged_when_status_read_fails() {
    let (_temp, project, main_sha) = setup_remote_repo();
    let repo = Path::new(&project.working_directory);
    git(repo, &["checkout", "-b", "feature/source-pr"]);
    git(repo, &["push", "-u", "origin", "feature/source-pr"]);
    git(repo, &["checkout", "main"]);
    let mut target = workspace("feature/source-pr", Some(main_sha));
    target.project_id = project.id.clone();
    target.source_pull_request = Some(source_pull_request(42, "feature/source-pr", "main"));
    let github = MockGithubService::new();
    github.state().check_pr_status_result =
        Some(Err(AppError::Infrastructure("offline".to_string())));

    let resolution = resolve_workspace_base_with_github(&project, &target, Some(&github))
        .await
        .expect("GitHub status errors must fail open");

    assert_eq!(resolution.status, BaseStatus::Valid);
    assert_eq!(
        resolution.effective_base_ref.as_deref(),
        Some("feature/source-pr")
    );
}

#[tokio::test]
async fn resolve_workspace_base_keeps_source_pull_request_unchanged_without_github_service() {
    let (_temp, project, main_sha) = setup_remote_repo();
    let repo = Path::new(&project.working_directory);
    git(repo, &["checkout", "-b", "feature/source-pr"]);
    git(repo, &["push", "-u", "origin", "feature/source-pr"]);
    git(repo, &["checkout", "main"]);
    let mut target = workspace("feature/source-pr", Some(main_sha));
    target.project_id = project.id.clone();
    target.source_pull_request = Some(source_pull_request(42, "feature/source-pr", "main"));

    let resolution = resolve_workspace_base_with_github(&project, &target, None)
        .await
        .expect("missing GitHub service must fail open");

    assert_eq!(resolution.status, BaseStatus::Valid);
    assert_eq!(
        resolution.effective_base_ref.as_deref(),
        Some("feature/source-pr")
    );
}

#[tokio::test]
async fn resolve_workspace_base_keeps_local_only_automation_base_valid() {
    // Regression: automation run publish base must stay on the automation branch.
    // The automation base branch (base_ref_kind = local_branch) is created as an
    // isolated local worktree branch and never pushed to origin. The resolver must
    // recognize the local branch instead of retargeting the run to the project
    // default (main), which would break automation chaining (merged_base /
    // pr_head_stacked → PR into the automation branch, not main).
    let (_temp, project, main_sha) = setup_remote_repo();
    let repo = Path::new(&project.working_directory);
    let automation_branch = "ralphx/ralphx/automation-3228e70e";
    // Create a local-only branch with an extra commit — like an automation setup
    // branch that carries setup work and is NOT contained in origin/main.
    git(repo, &["checkout", "-b", automation_branch]);
    std::fs::write(repo.join("setup.txt"), "automation setup\n").expect("setup file");
    git(repo, &["add", "setup.txt"]);
    git(repo, &["commit", "-m", "automation setup"]);
    let automation_sha = git(repo, &["rev-parse", "HEAD"]);
    git(repo, &["checkout", "main"]);

    let mut workspace = workspace(automation_branch, Some(automation_sha.clone()));
    workspace.base_ref_kind = IdeationAnalysisBaseRefKind::LocalBranch;
    workspace.project_id = project.id.clone();

    let resolution = resolve_workspace_base(&project, &workspace)
        .await
        .expect("local-only automation base should resolve");

    assert_eq!(
        resolution.status,
        BaseStatus::Valid,
        "automation base must not retarget to project default"
    );
    assert_eq!(resolution.old_base_ref, automation_branch);
    assert_eq!(
        resolution.effective_base_ref.as_deref(),
        Some(automation_branch)
    );
    assert_eq!(
        resolution.effective_checkout_ref.as_deref(),
        Some(automation_branch)
    );
    assert_eq!(
        resolution.effective_base_commit.as_deref(),
        Some(automation_sha.as_str())
    );
    // Guard against the wrong-base symptom explicitly.
    assert_ne!(resolution.effective_base_ref.as_deref(), Some("main"));
    // main_sha is only referenced to prove the automation head diverged from it.
    assert_ne!(automation_sha, main_sha);
}

#[tokio::test]
async fn resolve_workspace_base_retargets_when_base_absent_locally_and_remotely() {
    // Fallback path must still fire when the base branch genuinely exists nowhere:
    // no origin ref AND no local branch. This keeps the "base branch deleted"
    // retarget behavior for non-automation workspaces intact.
    let (_temp, project, main_sha) = setup_remote_repo();
    let mut workspace = workspace("feature/never-existed", Some(main_sha.clone()));
    workspace.project_id = project.id.clone();

    let resolution = resolve_workspace_base(&project, &workspace)
        .await
        .expect("absent base should resolve");

    assert_eq!(resolution.status, BaseStatus::Retargeted);
    assert_eq!(resolution.effective_base_ref.as_deref(), Some("main"));
}

#[tokio::test]
async fn resolve_workspace_base_from_local_snapshot_does_not_fetch_origin() {
    let (temp, project, main_sha) = setup_remote_repo();
    let repo = Path::new(&project.working_directory);
    let remote_url = git(repo, &["remote", "get-url", "origin"]);
    let other_repo = temp.path().join("other");
    Command::new("git")
        .args(["clone", remote_url.as_str(), other_repo.to_str().unwrap()])
        .output()
        .expect("second clone should be created");
    git(&other_repo, &["config", "user.email", "test@example.com"]);
    git(&other_repo, &["config", "user.name", "Test User"]);
    git(&other_repo, &["checkout", "-b", "feature/remote-only"]);
    std::fs::write(other_repo.join("remote-only.txt"), "remote\n")
        .expect("remote-only file should be written");
    git(&other_repo, &["add", "remote-only.txt"]);
    git(&other_repo, &["commit", "-m", "remote-only branch"]);
    git(
        &other_repo,
        &["push", "-u", "origin", "feature/remote-only"],
    );

    let mut target = workspace("feature/remote-only", Some(main_sha));
    target.project_id = project.id.clone();

    let local_resolution = resolve_workspace_base_from_local_snapshot(&project, &target)
        .await
        .expect("local snapshot should resolve without fetching");

    assert_eq!(local_resolution.status, BaseStatus::Retargeted);
    assert_eq!(local_resolution.old_base_ref, "feature/remote-only");
    assert_eq!(local_resolution.effective_base_ref.as_deref(), Some("main"));

    let fresh_resolution = resolve_workspace_base(&project, &target)
        .await
        .expect("fresh resolution should fetch remote refs");

    assert_eq!(fresh_resolution.status, BaseStatus::Valid);
    assert_eq!(
        fresh_resolution.effective_checkout_ref.as_deref(),
        Some("origin/feature/remote-only")
    );
}

#[tokio::test]
async fn resolve_workspace_base_retargets_missing_base_when_commit_is_in_default() {
    let (_temp, project, main_sha) = setup_remote_repo();
    let mut workspace = workspace("feature/deleted-base", Some(main_sha.clone()));
    workspace.project_id = project.id.clone();

    let resolution = resolve_workspace_base(&project, &workspace)
        .await
        .expect("base should resolve");

    assert_eq!(resolution.status, BaseStatus::Retargeted);
    assert_eq!(resolution.old_base_ref, "feature/deleted-base");
    assert_eq!(resolution.effective_base_ref.as_deref(), Some("main"));
    assert_eq!(
        resolution.effective_checkout_ref.as_deref(),
        Some("origin/main")
    );
    assert_eq!(
        resolution.effective_base_commit.as_deref(),
        Some(main_sha.as_str())
    );
    assert_eq!(
        resolution.display_name.as_deref(),
        Some("Project default (main)")
    );
}

#[tokio::test]
async fn resolve_workspace_base_blocks_missing_base_without_captured_commit() {
    let (_temp, project, _main_sha) = setup_remote_repo();
    let mut workspace = workspace("feature/deleted-base", None);
    workspace.project_id = project.id.clone();

    let resolution = resolve_workspace_base(&project, &workspace)
        .await
        .expect("base should classify as blocked");

    assert_eq!(resolution.status, BaseStatus::Blocked);
    assert_eq!(
        resolution.block_reason.as_deref(),
        Some(BLOCK_REASON_MISSING_BASE_COMMIT)
    );
    assert!(resolution.effective_base_ref.is_none());
}

#[tokio::test]
async fn resolve_workspace_base_blocks_missing_base_when_commit_not_in_default() {
    let (_temp, project, _main_sha) = setup_remote_repo();
    let repo = Path::new(&project.working_directory);
    git(repo, &["checkout", "--orphan", "unmerged-base"]);
    std::fs::write(repo.join("README.md"), "diverged\n").expect("diverged file");
    git(repo, &["add", "README.md"]);
    git(repo, &["commit", "-m", "diverged"]);
    let divergent_sha = git(repo, &["rev-parse", "HEAD"]);
    git(repo, &["checkout", "main"]);

    let mut workspace = workspace("feature/deleted-base", Some(divergent_sha));
    workspace.project_id = project.id.clone();

    let resolution = resolve_workspace_base(&project, &workspace)
        .await
        .expect("base should classify as blocked");

    assert_eq!(resolution.status, BaseStatus::Blocked);
    assert_eq!(
        resolution.block_reason.as_deref(),
        Some(BLOCK_REASON_NOT_CONTAINED)
    );
    assert!(resolution.effective_base_ref.is_none());
}
