use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

use crate::application::agent_conversation_workspace::{
    agent_conversation_branch_name, resolve_agent_conversation_workspace_path,
};
use crate::application::git_service::GitService;
use crate::domain::entities::{
    plan_branch::PrStatus, AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspaceStatus, ChatConversationId, IdeationAnalysisBaseRefKind,
    IdeationSessionId, PlanBranch, PlanBranchStatus, Project, ProjectId,
};

use super::git_artifact_cleanup::{
    cleanup_merged_plan_branch_local_artifacts, cleanup_terminal_agent_workspace_local_artifacts,
    cleanup_terminal_agent_workspace_local_artifacts_with_known_local_branches,
    cleanup_terminal_linked_plan_branch_local_artifacts,
};

fn run_git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn init_repo_at(repo: &Path) {
    std::fs::create_dir_all(repo).expect("create repo path");
    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "test@example.com"]);
    run_git(repo, &["config", "user.name", "Test User"]);
    run_git(repo, &["checkout", "-b", "main"]);
    std::fs::write(repo.join("README.md"), "base\n").expect("write readme");
    run_git(repo, &["add", "."]);
    run_git(repo, &["commit", "-m", "initial"]);
}

fn init_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    init_repo_at(dir.path());
    dir
}

fn branch_exists(repo: &Path, branch: &str) -> bool {
    Command::new("git")
        .args(["rev-parse", "--verify", branch])
        .current_dir(repo)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn project_for(repo: &Path, worktree_parent: &Path) -> Project {
    let mut project = Project::new(
        "Cleanup Project".to_string(),
        repo.to_string_lossy().to_string(),
    );
    project.id = ProjectId::from_string("project-cleanup".to_string());
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    project
}

fn cleanup_conversation_id() -> ChatConversationId {
    ChatConversationId::from_string("33333333-3333-3333-3333-333333333333")
}

fn merged_pr_plan_branch(branch_name: &str, project_id: ProjectId) -> PlanBranch {
    let mut plan_branch = PlanBranch::new(
        crate::domain::entities::artifact::ArtifactId::from_string("artifact-1"),
        IdeationSessionId::from_string("session-1"),
        project_id,
        branch_name.to_string(),
        "main".to_string(),
    );
    plan_branch.status = PlanBranchStatus::Merged;
    plan_branch.pr_eligible = true;
    plan_branch.pr_number = Some(42);
    plan_branch.pr_status = Some(PrStatus::Merged);
    plan_branch
}

fn workspace_for(
    project: &Project,
    branch_name: &str,
    pr_status: &str,
) -> AgentConversationWorkspace {
    let conversation_id = cleanup_conversation_id();
    let worktree_path =
        resolve_agent_conversation_workspace_path(project, &conversation_id).unwrap();
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        branch_name.to_string(),
        worktree_path.to_string_lossy().to_string(),
    );
    workspace.publication_pr_number = Some(99);
    workspace.publication_pr_status = Some(pr_status.to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    workspace.status = AgentConversationWorkspaceStatus::Active;
    workspace
}

fn expected_workspace_branch(project: &Project) -> String {
    let conversation_id = cleanup_conversation_id();
    agent_conversation_branch_name(project, &conversation_id)
}

#[tokio::test]
async fn merged_pr_plan_branch_cleanup_deletes_local_branch_when_merged_to_base() {
    let repo = init_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = project_for(repo.path(), worktrees.path());
    let branch = "ralphx/cleanup/plan-merged";

    run_git(repo.path(), &["checkout", "-b", branch]);
    std::fs::write(repo.path().join("plan.txt"), "plan\n").expect("write plan");
    run_git(repo.path(), &["add", "."]);
    run_git(repo.path(), &["commit", "-m", "plan work"]);
    run_git(repo.path(), &["checkout", "main"]);
    run_git(
        repo.path(),
        &["merge", "--no-ff", branch, "-m", "merge plan"],
    );

    let plan_branch = merged_pr_plan_branch(branch, project.id.clone());
    let report = cleanup_merged_plan_branch_local_artifacts(&project, &plan_branch)
        .await
        .expect("cleanup should succeed");

    assert!(report.branch_deleted);
    assert!(!branch_exists(repo.path(), branch));
}

#[tokio::test]
async fn plan_branch_cleanup_skips_when_branch_is_not_terminal() {
    let repo = init_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = project_for(repo.path(), worktrees.path());
    let branch = "ralphx/cleanup/plan-active";
    run_git(repo.path(), &["checkout", "-b", branch]);
    run_git(repo.path(), &["checkout", "main"]);
    let mut plan_branch = merged_pr_plan_branch(branch, project.id.clone());
    plan_branch.status = PlanBranchStatus::Active;
    plan_branch.pr_status = Some(PrStatus::Open);

    let report = cleanup_merged_plan_branch_local_artifacts(&project, &plan_branch)
        .await
        .expect("cleanup should skip active branch");

    assert!(!report.branch_deleted);
    assert_eq!(
        report.skipped_reason.as_deref(),
        Some("plan_branch_not_merged")
    );
    assert!(branch_exists(repo.path(), branch));
}

#[tokio::test]
async fn merged_pr_plan_branch_cleanup_skips_missing_branch_and_target() {
    let repo = init_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = project_for(repo.path(), worktrees.path());
    let missing_branch = merged_pr_plan_branch("ralphx/cleanup/plan-missing", project.id.clone());

    let missing_branch_report =
        cleanup_merged_plan_branch_local_artifacts(&project, &missing_branch)
            .await
            .expect("cleanup should skip missing branch");

    assert_eq!(
        missing_branch_report.skipped_reason.as_deref(),
        Some("branch_missing")
    );

    let branch = "ralphx/cleanup/plan-missing-target";
    run_git(repo.path(), &["checkout", "-b", branch]);
    run_git(repo.path(), &["checkout", "main"]);
    let mut missing_target = merged_pr_plan_branch(branch, project.id.clone());
    missing_target.base_branch_override = Some("missing-base".to_string());

    let missing_target_report =
        cleanup_merged_plan_branch_local_artifacts(&project, &missing_target)
            .await
            .expect("cleanup should skip missing target");

    assert_eq!(
        missing_target_report.skipped_reason.as_deref(),
        Some("target_ref_missing:missing-base")
    );
    assert!(branch_exists(repo.path(), branch));
}

#[tokio::test]
async fn merged_pr_plan_branch_cleanup_preserves_non_ralphx_plan_branch() {
    let repo = init_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = project_for(repo.path(), worktrees.path());
    let branch = "feature/user-owned-plan";

    run_git(repo.path(), &["branch", branch, "main"]);
    let plan_branch = merged_pr_plan_branch(branch, project.id.clone());
    let report = cleanup_merged_plan_branch_local_artifacts(&project, &plan_branch)
        .await
        .expect("cleanup should skip non-RalphX plan branch");

    assert!(!report.branch_deleted);
    assert_eq!(
        report.skipped_reason.as_deref(),
        Some("branch_not_ralphx_owned")
    );
    assert!(branch_exists(repo.path(), branch));
}

#[tokio::test]
async fn merged_pr_plan_branch_cleanup_preserves_base_branch_even_when_equivalent() {
    let repo = init_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = project_for(repo.path(), worktrees.path());
    run_git(repo.path(), &["checkout", "-b", "scratch"]);

    let plan_branch = merged_pr_plan_branch("main", project.id.clone());
    let report = cleanup_merged_plan_branch_local_artifacts(&project, &plan_branch)
        .await
        .expect("cleanup should skip protected base branch");

    assert!(!report.branch_deleted);
    assert_eq!(
        report.skipped_reason.as_deref(),
        Some("branch_not_ralphx_owned")
    );
    assert!(branch_exists(repo.path(), "main"));
}

#[tokio::test]
async fn merged_pr_plan_branch_cleanup_keeps_unmerged_local_branch() {
    let repo = init_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = project_for(repo.path(), worktrees.path());
    let branch = "ralphx/cleanup/plan-unmerged";

    run_git(repo.path(), &["checkout", "-b", branch]);
    std::fs::write(repo.path().join("plan.txt"), "plan\n").expect("write plan");
    run_git(repo.path(), &["add", "."]);
    run_git(repo.path(), &["commit", "-m", "plan work"]);
    run_git(repo.path(), &["checkout", "main"]);

    let plan_branch = merged_pr_plan_branch(branch, project.id.clone());
    let report = cleanup_merged_plan_branch_local_artifacts(&project, &plan_branch)
        .await
        .expect("cleanup should succeed");

    assert!(!report.branch_deleted);
    assert!(report.skipped_reason.is_some());
    assert!(branch_exists(repo.path(), branch));
}

#[tokio::test]
async fn merged_pr_plan_branch_cleanup_uses_remote_base_when_local_base_is_stale() {
    let repo = init_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = project_for(repo.path(), worktrees.path());
    let branch = "ralphx/cleanup/plan-remote-merged";

    run_git(repo.path(), &["checkout", "-b", branch]);
    std::fs::write(repo.path().join("plan.txt"), "plan\n").expect("write plan");
    run_git(repo.path(), &["add", "."]);
    run_git(repo.path(), &["commit", "-m", "plan work"]);
    run_git(
        repo.path(),
        &["update-ref", "refs/remotes/origin/main", branch],
    );
    run_git(repo.path(), &["checkout", "main"]);

    let plan_branch = merged_pr_plan_branch(branch, project.id.clone());
    let report = cleanup_merged_plan_branch_local_artifacts(&project, &plan_branch)
        .await
        .expect("cleanup should succeed");

    assert!(report.branch_deleted);
    assert!(!branch_exists(repo.path(), branch));
}

#[tokio::test]
async fn merged_pr_plan_branch_cleanup_accepts_origin_base_ref() {
    let repo = init_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = project_for(repo.path(), worktrees.path());
    let branch = "ralphx/cleanup/plan-origin-base";

    run_git(repo.path(), &["checkout", "-b", branch]);
    std::fs::write(repo.path().join("plan.txt"), "plan\n").expect("write plan");
    run_git(repo.path(), &["add", "."]);
    run_git(repo.path(), &["commit", "-m", "plan work"]);
    run_git(
        repo.path(),
        &["update-ref", "refs/remotes/origin/main", branch],
    );
    run_git(repo.path(), &["checkout", "main"]);

    let mut plan_branch = merged_pr_plan_branch(branch, project.id.clone());
    plan_branch.base_branch_override = Some("origin/main".to_string());
    let report = cleanup_merged_plan_branch_local_artifacts(&project, &plan_branch)
        .await
        .expect("cleanup should succeed");

    assert!(report.branch_deleted);
    assert!(!branch_exists(repo.path(), branch));
}

#[tokio::test]
async fn merged_pr_plan_branch_cleanup_reports_delete_failure() {
    let repo = init_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = project_for(repo.path(), worktrees.path());
    let branch = "ralphx/cleanup/plan-delete-failure";

    run_git(repo.path(), &["checkout", "-b", branch]);
    std::fs::write(repo.path().join("plan.txt"), "plan\n").expect("write plan");
    run_git(repo.path(), &["add", "."]);
    run_git(repo.path(), &["commit", "-m", "plan work"]);
    run_git(repo.path(), &["checkout", "main"]);
    run_git(
        repo.path(),
        &["merge", "--no-ff", branch, "-m", "merge plan"],
    );
    run_git(repo.path(), &["checkout", branch]);

    let plan_branch = merged_pr_plan_branch(branch, project.id.clone());
    let error = cleanup_merged_plan_branch_local_artifacts(&project, &plan_branch)
        .await
        .expect_err("deleting the checked-out branch should fail");

    assert!(error.to_string().contains("Failed to delete local branch"));
    assert!(branch_exists(repo.path(), branch));
}

#[tokio::test]
async fn closed_agent_workspace_cleanup_force_removes_worktree_and_local_branch() {
    let repo = init_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = project_for(repo.path(), worktrees.path());
    let branch = expected_workspace_branch(&project);
    let workspace = workspace_for(&project, &branch, "closed");
    let worktree_path = Path::new(&workspace.worktree_path);

    GitService::create_worktree(repo.path(), worktree_path, &branch, "main")
        .await
        .expect("create worktree");
    std::fs::write(worktree_path.join("agent.txt"), "agent\n").expect("write agent");
    run_git(worktree_path, &["add", "."]);
    run_git(worktree_path, &["commit", "-m", "agent work"]);

    let report = cleanup_terminal_agent_workspace_local_artifacts(&project, &workspace, false)
        .await
        .expect("cleanup should succeed");

    assert!(report.worktree_removed);
    assert!(report.branch_deleted);
    assert!(!worktree_path.exists());
    assert!(!branch_exists(repo.path(), &branch));
}

#[tokio::test]
async fn merged_agent_workspace_cleanup_force_removes_dirty_and_ignored_artifacts() {
    let repo = init_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = project_for(repo.path(), worktrees.path());
    let branch = expected_workspace_branch(&project);
    let workspace = workspace_for(&project, &branch, "merged");
    let worktree_path = Path::new(&workspace.worktree_path);

    GitService::create_worktree(repo.path(), worktree_path, &branch, "main")
        .await
        .expect("create worktree");
    std::fs::write(worktree_path.join("dirty.txt"), "dirty\n").expect("write dirty file");
    let ignored_artifacts = worktree_path.join("target/llvm-cov-target");
    std::fs::create_dir_all(&ignored_artifacts).expect("create ignored artifact directory");
    std::fs::write(ignored_artifacts.join("coverage.profraw"), "large artifact")
        .expect("write ignored artifact");

    let report = cleanup_terminal_agent_workspace_local_artifacts(&project, &workspace, true)
        .await
        .expect("cleanup should succeed");

    assert!(report.worktree_removed);
    assert!(report.branch_deleted);
    assert_eq!(report.skipped_reason, None);
    assert!(!worktree_path.exists());
    assert!(!branch_exists(repo.path(), &branch));
}

#[tokio::test]
async fn merged_agent_workspace_cleanup_skips_mismatched_or_non_directory_path() {
    let repo = init_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = project_for(repo.path(), worktrees.path());

    let mut mismatched = workspace_for(&project, "ralphx/cleanup/mismatch", "merged");
    mismatched.worktree_path = worktrees
        .path()
        .join("unexpected-worktree")
        .to_string_lossy()
        .to_string();
    let mismatch_report =
        cleanup_terminal_agent_workspace_local_artifacts(&project, &mismatched, true)
            .await
            .expect("cleanup should skip mismatched path");
    assert_eq!(
        mismatch_report.skipped_reason.as_deref(),
        Some("workspace_path_mismatch")
    );

    let branch = expected_workspace_branch(&project);
    let workspace = workspace_for(&project, &branch, "merged");
    let worktree_path = Path::new(&workspace.worktree_path);
    std::fs::create_dir_all(worktree_path.parent().expect("workspace parent"))
        .expect("create workspace parent");
    std::fs::write(worktree_path, "not a directory\n").expect("write workspace file");

    let non_directory_report =
        cleanup_terminal_agent_workspace_local_artifacts(&project, &workspace, true)
            .await
            .expect("cleanup should skip non-directory path");
    assert_eq!(
        non_directory_report.skipped_reason.as_deref(),
        Some("workspace_path_not_directory")
    );
    assert!(worktree_path.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn merged_agent_workspace_cleanup_rejects_dangling_symlink_without_deleting_branch() {
    let repo = init_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = project_for(repo.path(), worktrees.path());
    let branch = expected_workspace_branch(&project);
    let workspace = workspace_for(&project, &branch, "merged");
    let worktree_path = Path::new(&workspace.worktree_path);
    std::fs::create_dir_all(worktree_path.parent().expect("workspace parent"))
        .expect("create workspace parent");
    run_git(repo.path(), &["branch", &branch, "main"]);
    std::os::unix::fs::symlink(worktrees.path().join("missing-target"), worktree_path)
        .expect("create dangling workspace symlink");

    let report = cleanup_terminal_agent_workspace_local_artifacts(&project, &workspace, true)
        .await
        .expect("cleanup should reject dangling symlink");

    assert_eq!(
        report.skipped_reason.as_deref(),
        Some("workspace_path_symlink")
    );
    assert!(std::fs::symlink_metadata(worktree_path).is_ok());
    assert!(branch_exists(repo.path(), &branch));
}

#[tokio::test]
async fn merged_agent_workspace_cleanup_tolerates_missing_worktree_path() {
    let repo = init_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = project_for(repo.path(), worktrees.path());
    let workspace = workspace_for(&project, "ralphx/cleanup/missing-worktree", "merged");
    let worktree_path = Path::new(&workspace.worktree_path);

    let report = cleanup_terminal_agent_workspace_local_artifacts(&project, &workspace, false)
        .await
        .expect("cleanup should tolerate missing worktree path");

    assert!(!report.worktree_removed);
    assert!(!report.branch_deleted);
    assert!(!worktree_path.exists());
}

#[tokio::test]
async fn merged_agent_workspace_cleanup_skips_project_root_path() {
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let mut project = project_for(Path::new("/placeholder"), worktrees.path());
    let conversation_id = cleanup_conversation_id();
    let project_root =
        resolve_agent_conversation_workspace_path(&project, &conversation_id).unwrap();
    init_repo_at(&project_root);
    project.working_directory = project_root.to_string_lossy().to_string();
    let workspace = workspace_for(&project, "ralphx/cleanup/project-root", "merged");

    let report = cleanup_terminal_agent_workspace_local_artifacts(&project, &workspace, true)
        .await
        .expect("cleanup should skip project root");

    assert!(!report.worktree_removed);
    assert!(!report.branch_deleted);
    assert_eq!(
        report.skipped_reason.as_deref(),
        Some("workspace_points_to_project_root")
    );
    assert!(project_root.exists());
}

#[tokio::test]
async fn merged_agent_workspace_cleanup_deletes_branch_when_worktree_missing() {
    let repo = init_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = project_for(repo.path(), worktrees.path());
    let branch = expected_workspace_branch(&project);
    let workspace = workspace_for(&project, &branch, "merged");

    run_git(repo.path(), &["checkout", "-b", &branch]);
    std::fs::write(repo.path().join("agent.txt"), "agent\n").expect("write agent");
    run_git(repo.path(), &["add", "."]);
    run_git(repo.path(), &["commit", "-m", "agent work"]);
    run_git(repo.path(), &["checkout", "main"]);
    run_git(
        repo.path(),
        &["merge", "--no-ff", &branch, "-m", "merge agent"],
    );

    let report = cleanup_terminal_agent_workspace_local_artifacts(&project, &workspace, true)
        .await
        .expect("cleanup should delete merged branch without a worktree");

    assert!(!report.worktree_removed);
    assert!(report.branch_deleted);
    assert!(!branch_exists(repo.path(), &branch));
    assert!(!Path::new(&workspace.worktree_path).exists());
}

#[tokio::test]
async fn merged_agent_workspace_cleanup_preserves_unexpected_branch_name() {
    let repo = init_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = project_for(repo.path(), worktrees.path());
    let user_branch = "feature/user-owned-agent";
    run_git(repo.path(), &["branch", user_branch, "main"]);

    let workspace = workspace_for(&project, user_branch, "merged");
    let report = cleanup_terminal_agent_workspace_local_artifacts(&project, &workspace, true)
        .await
        .expect("cleanup should skip unexpected branch name");

    assert!(!report.branch_deleted);
    assert_eq!(
        report.skipped_reason.as_deref(),
        Some("branch_not_ralphx_owned")
    );
    assert!(branch_exists(repo.path(), user_branch));
}

#[tokio::test]
async fn merged_agent_workspace_cleanup_deletes_owned_continuation_branch() {
    let repo = init_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = project_for(repo.path(), worktrees.path());
    let branch = format!("{}-1712345678901", expected_workspace_branch(&project));
    let workspace = workspace_for(&project, &branch, "merged");

    run_git(repo.path(), &["checkout", "-b", &branch]);
    std::fs::write(repo.path().join("agent.txt"), "agent\n").expect("write agent");
    run_git(repo.path(), &["add", "."]);
    run_git(repo.path(), &["commit", "-m", "agent work"]);
    run_git(repo.path(), &["checkout", "main"]);
    run_git(
        repo.path(),
        &["merge", "--no-ff", &branch, "-m", "merge agent"],
    );

    let report = cleanup_terminal_agent_workspace_local_artifacts(&project, &workspace, true)
        .await
        .expect("cleanup should delete owned continuation branch");

    assert!(report.branch_deleted);
    assert!(!branch_exists(repo.path(), &branch));
}

#[tokio::test]
async fn merged_agent_workspace_cleanup_deletes_provider_ticket_branch() {
    let repo = init_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = project_for(repo.path(), worktrees.path());
    let branch = "ralphx/cleanup-project/agent-jira-PROJ-123-33333333";
    let workspace = workspace_for(&project, branch, "merged");

    run_git(repo.path(), &["checkout", "-b", branch]);
    std::fs::write(repo.path().join("agent.txt"), "agent\n").expect("write agent");
    run_git(repo.path(), &["add", "."]);
    run_git(repo.path(), &["commit", "-m", "agent work"]);
    run_git(repo.path(), &["checkout", "main"]);
    run_git(
        repo.path(),
        &["merge", "--no-ff", branch, "-m", "merge agent"],
    );

    let report = cleanup_terminal_agent_workspace_local_artifacts(&project, &workspace, true)
        .await
        .expect("cleanup should delete provider-aware ticket branch");

    assert!(report.branch_deleted);
    assert!(!branch_exists(repo.path(), branch));
}

#[tokio::test]
async fn merged_agent_workspace_cleanup_deletes_provider_ticket_continuation_branch() {
    let repo = init_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = project_for(repo.path(), worktrees.path());
    let branch = "ralphx/cleanup-project/agent-linear-ENG-99-33333333-1712345678901";
    let workspace = workspace_for(&project, branch, "merged");

    run_git(repo.path(), &["checkout", "-b", branch]);
    std::fs::write(repo.path().join("agent.txt"), "agent\n").expect("write agent");
    run_git(repo.path(), &["add", "."]);
    run_git(repo.path(), &["commit", "-m", "agent work"]);
    run_git(repo.path(), &["checkout", "main"]);
    run_git(
        repo.path(),
        &["merge", "--no-ff", branch, "-m", "merge agent"],
    );

    let report = cleanup_terminal_agent_workspace_local_artifacts(&project, &workspace, true)
        .await
        .expect("cleanup should delete provider-aware ticket continuation branch");

    assert!(report.branch_deleted);
    assert!(!branch_exists(repo.path(), branch));
}

#[tokio::test]
async fn merged_agent_workspace_cleanup_preserves_non_numeric_continuation_like_branch() {
    let repo = init_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = project_for(repo.path(), worktrees.path());
    let branch = format!("{}-user-owned", expected_workspace_branch(&project));
    run_git(repo.path(), &["branch", &branch, "main"]);

    let workspace = workspace_for(&project, &branch, "merged");
    let report = cleanup_terminal_agent_workspace_local_artifacts(&project, &workspace, true)
        .await
        .expect("cleanup should skip non-numeric continuation-like branch");

    assert!(!report.branch_deleted);
    assert_eq!(
        report.skipped_reason.as_deref(),
        Some("branch_not_ralphx_owned")
    );
    assert!(branch_exists(repo.path(), &branch));
}

#[tokio::test]
async fn merged_agent_workspace_cleanup_removes_clean_worktree_and_merged_branch() {
    let repo = init_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = project_for(repo.path(), worktrees.path());
    let branch = expected_workspace_branch(&project);
    let workspace = workspace_for(&project, &branch, "merged");
    let worktree_path = Path::new(&workspace.worktree_path);

    GitService::create_worktree(repo.path(), worktree_path, &branch, "main")
        .await
        .expect("create worktree");
    std::fs::write(worktree_path.join("agent.txt"), "agent\n").expect("write agent");
    run_git(worktree_path, &["add", "."]);
    run_git(worktree_path, &["commit", "-m", "agent work"]);
    run_git(
        repo.path(),
        &["merge", "--no-ff", &branch, "-m", "merge agent"],
    );

    let report = cleanup_terminal_agent_workspace_local_artifacts(&project, &workspace, true)
        .await
        .expect("cleanup should succeed");

    assert!(report.worktree_removed);
    assert!(report.branch_deleted);
    assert!(!worktree_path.exists());
    assert!(!branch_exists(repo.path(), &branch));
}

#[tokio::test]
async fn terminal_linked_plan_cleanup_preserves_non_ralphx_branch() {
    let repo = init_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = project_for(repo.path(), worktrees.path());
    let branch = "feature/user-plan-branch";
    run_git(repo.path(), &["branch", branch, "main"]);

    let mut plan_branch = merged_pr_plan_branch(branch, project.id.clone());
    plan_branch.session_id = IdeationSessionId::from_string("linked-session".to_string());
    let mut workspace = workspace_for(&project, branch, "merged");
    workspace.mode = AgentConversationWorkspaceMode::Ideation;
    workspace.linked_ideation_session_id = Some(plan_branch.session_id.clone());
    workspace.linked_plan_branch_id = Some(plan_branch.id.clone());

    let report =
        cleanup_terminal_linked_plan_branch_local_artifacts(&project, &workspace, &plan_branch)
            .await
            .expect("cleanup should skip user-owned linked branch");

    assert!(!report.branch_deleted);
    assert_eq!(
        report.skipped_reason.as_deref(),
        Some("branch_not_ralphx_owned")
    );
    assert!(branch_exists(repo.path(), branch));
}

#[tokio::test]
async fn terminal_workspace_cleanup_removes_unregistered_directory() {
    let repo = init_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = project_for(repo.path(), worktrees.path());
    let branch = expected_workspace_branch(&project);
    let workspace = workspace_for(&project, &branch, "merged");
    let worktree_path = Path::new(&workspace.worktree_path);
    std::fs::create_dir_all(worktree_path).expect("create unregistered workspace directory");
    std::fs::write(worktree_path.join("artifact.txt"), "generated\n")
        .expect("write generated artifact");

    let report = cleanup_terminal_agent_workspace_local_artifacts(&project, &workspace, true)
        .await
        .expect("cleanup should remove unregistered directory");

    assert!(report.worktree_removed);
    assert!(!report.branch_deleted);
    assert_eq!(report.skipped_reason, None);
    assert!(!worktree_path.exists());
}

#[tokio::test]
async fn terminal_workspace_cleanup_reports_force_delete_failure_for_checked_out_branch() {
    let repo = init_repo();
    let worktrees = tempfile::tempdir().expect("worktree parent");
    let project = project_for(repo.path(), worktrees.path());
    let branch = expected_workspace_branch(&project);
    let workspace = workspace_for(&project, &branch, "merged");
    run_git(repo.path(), &["checkout", "-b", &branch]);
    let known_branches = HashSet::from([branch.clone()]);

    let error = cleanup_terminal_agent_workspace_local_artifacts_with_known_local_branches(
        &project,
        &workspace,
        true,
        Some(&known_branches),
    )
    .await
    .expect_err("deleting the checked-out branch should fail");

    assert!(error
        .to_string()
        .contains("Failed to force-delete verified terminal local branch"));
    assert!(branch_exists(repo.path(), &branch));
}
