use super::agent_workspace_review_base::resolve_agent_workspace_review_base;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceBranchMode,
    AgentConversationWorkspaceMode, ChatConversationId, IdeationAnalysisBaseRefKind, ProjectId,
};
use crate::error::AppError;
use std::path::Path;
use std::process::Command;

fn workspace(branch_mode: AgentConversationWorkspaceBranchMode) -> AgentConversationWorkspace {
    let mut workspace = AgentConversationWorkspace::new(
        ChatConversationId::from_string("conversation-review-base-test"),
        ProjectId::from_string("project-review-base-test".to_string()),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::CurrentBranch,
        "main".to_string(),
        Some("main".to_string()),
        Some("captured-base".to_string()),
        "feature".to_string(),
        "/tmp/review-base-workspace".to_string(),
    );
    workspace.branch_mode = branch_mode;
    workspace
}

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

fn init_diverged_repo() -> (tempfile::TempDir, String) {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo = temp.path();
    git(repo, &["init", "-b", "main"]);
    git(repo, &["config", "user.email", "test@example.com"]);
    git(repo, &["config", "user.name", "Test User"]);
    std::fs::write(repo.join("README.md"), "base\n").expect("base file should be written");
    git(repo, &["add", "README.md"]);
    git(repo, &["commit", "-m", "base"]);
    let branch_point = git(repo, &["rev-parse", "HEAD"]);

    git(repo, &["checkout", "-b", "feature"]);
    std::fs::write(repo.join("feature.rs"), "pub fn feature() {}\n")
        .expect("feature file should be written");
    git(repo, &["add", "feature.rs"]);
    git(repo, &["commit", "-m", "feature"]);

    git(repo, &["checkout", "main"]);
    std::fs::write(repo.join("main.rs"), "pub fn main_change() {}\n")
        .expect("main file should be written");
    git(repo, &["add", "main.rs"]);
    git(repo, &["commit", "-m", "main"]);

    (temp, branch_point)
}

#[tokio::test]
async fn isolated_workspace_review_base_uses_trimmed_captured_base() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let workspace = workspace(AgentConversationWorkspaceBranchMode::Isolated);

    let base =
        resolve_agent_workspace_review_base(temp.path(), &workspace, "ignored", " captured-sha ")
            .await
            .expect("isolated workspace should use captured base");

    assert_eq!(base, "captured-sha");
}

#[tokio::test]
async fn workspace_review_base_requires_captured_base() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let workspace = workspace(AgentConversationWorkspaceBranchMode::Isolated);

    let result = resolve_agent_workspace_review_base(temp.path(), &workspace, "feature", " ").await;

    assert!(
        matches!(result, Err(AppError::Validation(message)) if message.contains("captured base commit"))
    );
}

#[tokio::test]
async fn linked_workspace_review_base_requires_base_ref() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let mut workspace = workspace(AgentConversationWorkspaceBranchMode::Linked);
    workspace.base_ref = " ".to_string();

    let result =
        resolve_agent_workspace_review_base(temp.path(), &workspace, "feature", "captured").await;

    assert!(matches!(result, Err(AppError::Validation(message)) if message.contains("base ref")));
}

#[tokio::test]
async fn linked_workspace_review_base_requires_head_ref() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let workspace = workspace(AgentConversationWorkspaceBranchMode::Linked);

    let result =
        resolve_agent_workspace_review_base(temp.path(), &workspace, " ", "captured").await;

    assert!(matches!(result, Err(AppError::Validation(message)) if message.contains("head ref")));
}

#[tokio::test]
async fn linked_workspace_review_base_uses_branch_merge_base() {
    let (temp, branch_point) = init_diverged_repo();
    let mut workspace = workspace(AgentConversationWorkspaceBranchMode::Linked);
    workspace.base_ref = "main".to_string();

    let base = resolve_agent_workspace_review_base(temp.path(), &workspace, "feature", "captured")
        .await
        .expect("linked workspace should resolve merge base");

    assert_eq!(base, branch_point);
}
