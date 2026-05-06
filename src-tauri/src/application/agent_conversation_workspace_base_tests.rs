use super::agent_conversation_workspace_base::{
    resolve_workspace_base, BaseStatus, BLOCK_REASON_MISSING_BASE_COMMIT,
    BLOCK_REASON_NOT_CONTAINED,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatConversationId,
    IdeationAnalysisBaseRefKind, Project,
};
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
