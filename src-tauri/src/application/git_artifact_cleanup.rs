use std::path::{Path, PathBuf};

use crate::application::agent_conversation_workspace::resolve_agent_conversation_workspace_path;
use crate::application::git_service::GitService;
use crate::domain::entities::{
    plan_branch::PrStatus, AgentConversationWorkspace, PlanBranch, PlanBranchStatus, Project,
};
use crate::domain::state_machine::transition_handler::resolve_plan_branch_pr_base;
use crate::error::{AppError, AppResult};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct LocalGitArtifactCleanupReport {
    pub worktree_removed: bool,
    pub branch_deleted: bool,
    pub skipped_reason: Option<String>,
}

pub(crate) async fn cleanup_merged_plan_branch_local_artifacts(
    project: &Project,
    plan_branch: &PlanBranch,
) -> AppResult<LocalGitArtifactCleanupReport> {
    if plan_branch.status != PlanBranchStatus::Merged
        && !matches!(plan_branch.pr_status, Some(PrStatus::Merged))
    {
        return Ok(LocalGitArtifactCleanupReport {
            skipped_reason: Some("plan_branch_not_merged".to_string()),
            ..LocalGitArtifactCleanupReport::default()
        });
    }

    let repo_path = Path::new(&project.working_directory);
    let target_ref = resolve_plan_branch_pr_base(project, plan_branch);
    delete_branch_when_merged_or_equivalent(repo_path, &plan_branch.branch_name, &target_ref).await
}

pub(crate) async fn cleanup_terminal_agent_workspace_local_artifacts(
    project: &Project,
    workspace: &AgentConversationWorkspace,
    delete_branch_if_merged: bool,
) -> AppResult<LocalGitArtifactCleanupReport> {
    let repo_path = Path::new(&project.working_directory);
    let expected_path =
        resolve_agent_conversation_workspace_path(project, &workspace.conversation_id)?;
    let stored_path = PathBuf::from(&workspace.worktree_path);
    if stored_path != expected_path {
        return Ok(LocalGitArtifactCleanupReport {
            skipped_reason: Some("workspace_path_mismatch".to_string()),
            ..LocalGitArtifactCleanupReport::default()
        });
    }

    let project_root = PathBuf::from(&project.working_directory);
    if expected_path == project_root {
        return Ok(LocalGitArtifactCleanupReport {
            skipped_reason: Some("workspace_points_to_project_root".to_string()),
            ..LocalGitArtifactCleanupReport::default()
        });
    }

    let mut report = LocalGitArtifactCleanupReport::default();
    remove_clean_agent_worktree(repo_path, &expected_path, &mut report).await?;

    if delete_branch_if_merged && report.skipped_reason.is_none() {
        let branch_report = delete_branch_when_merged_or_equivalent(
            repo_path,
            &workspace.branch_name,
            &workspace.base_ref,
        )
        .await?;
        report.branch_deleted = branch_report.branch_deleted;
        if report.skipped_reason.is_none() {
            report.skipped_reason = branch_report.skipped_reason;
        }
    }

    Ok(report)
}

async fn remove_clean_agent_worktree(
    repo_path: &Path,
    expected_path: &Path,
    report: &mut LocalGitArtifactCleanupReport,
) -> AppResult<()> {
    let safe_path = crate::utils::path_safety::validate_absolute_non_root_path(
        expected_path,
        "agent workspace cleanup",
    )?;
    if !safe_path.exists() {
        GitService::delete_worktree(repo_path, &safe_path).await?;
        return Ok(());
    }

    if !safe_path.is_dir() {
        report.skipped_reason = Some("workspace_path_not_directory".to_string());
        return Ok(());
    }

    if GitService::has_uncommitted_changes(&safe_path).await? {
        report.skipped_reason = Some("workspace_has_uncommitted_changes".to_string());
        return Ok(());
    }

    GitService::delete_worktree(repo_path, &safe_path).await?;
    report.worktree_removed = true;
    Ok(())
}

async fn delete_branch_when_merged_or_equivalent(
    repo_path: &Path,
    branch: &str,
    target_ref: &str,
) -> AppResult<LocalGitArtifactCleanupReport> {
    let mut report = LocalGitArtifactCleanupReport::default();

    if !GitService::branch_exists(repo_path, branch).await? {
        report.skipped_reason = Some("branch_missing".to_string());
        return Ok(report);
    }

    let Some(existing_target_ref) =
        resolve_existing_cleanup_target_ref(repo_path, target_ref).await?
    else {
        report.skipped_reason = Some(format!("target_ref_missing:{target_ref}"));
        return Ok(report);
    };

    let (safe_to_delete, reason) =
        GitService::is_branch_merged_or_content_equivalent(repo_path, branch, &existing_target_ref)
            .await;
    if !safe_to_delete {
        report.skipped_reason = Some(format!("branch_not_merged:{reason}"));
        return Ok(report);
    }

    GitService::delete_branch(repo_path, branch, true)
        .await
        .map_err(|error| {
            AppError::GitOperation(format!(
                "Failed to delete local branch '{branch}' after terminal PR cleanup: {error}"
            ))
        })?;
    report.branch_deleted = true;
    Ok(report)
}

async fn resolve_existing_cleanup_target_ref(
    repo_path: &Path,
    target_ref: &str,
) -> AppResult<Option<String>> {
    if !target_ref.starts_with("origin/") {
        let remote_ref = format!("origin/{target_ref}");
        if GitService::ref_exists(repo_path, &remote_ref).await? {
            return Ok(Some(remote_ref));
        }
    }

    if GitService::ref_exists(repo_path, target_ref).await? {
        return Ok(Some(target_ref.to_string()));
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use crate::application::agent_conversation_workspace::resolve_agent_conversation_workspace_path;
    use crate::application::git_service::GitService;
    use crate::domain::entities::{
        plan_branch::PrStatus, AgentConversationWorkspace, AgentConversationWorkspaceMode,
        AgentConversationWorkspaceStatus, ChatConversationId, IdeationAnalysisBaseRefKind,
        IdeationSessionId, PlanBranch, PlanBranchStatus, Project, ProjectId,
    };

    use super::{
        cleanup_merged_plan_branch_local_artifacts,
        cleanup_terminal_agent_workspace_local_artifacts,
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

    fn init_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        run_git(dir.path(), &["init"]);
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test User"]);
        run_git(dir.path(), &["checkout", "-b", "main"]);
        std::fs::write(dir.path().join("README.md"), "base\n").expect("write readme");
        run_git(dir.path(), &["add", "."]);
        run_git(dir.path(), &["commit", "-m", "initial"]);
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
        let conversation_id = ChatConversationId::from_string("conversation-cleanup");
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
        let missing_branch = merged_pr_plan_branch("ralphx/cleanup/missing", project.id.clone());

        let missing_branch_report =
            cleanup_merged_plan_branch_local_artifacts(&project, &missing_branch)
                .await
                .expect("cleanup should skip missing branch");

        assert_eq!(
            missing_branch_report.skipped_reason.as_deref(),
            Some("branch_missing")
        );

        let branch = "ralphx/cleanup/missing-target";
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
    async fn closed_agent_workspace_cleanup_removes_clean_worktree_but_keeps_branch() {
        let repo = init_repo();
        let worktrees = tempfile::tempdir().expect("worktree parent");
        let project = project_for(repo.path(), worktrees.path());
        let branch = "ralphx/cleanup/agent-closed";
        let workspace = workspace_for(&project, branch, "closed");
        let worktree_path = Path::new(&workspace.worktree_path);

        GitService::create_worktree(repo.path(), worktree_path, branch, "main")
            .await
            .expect("create worktree");
        std::fs::write(worktree_path.join("agent.txt"), "agent\n").expect("write agent");
        run_git(worktree_path, &["add", "."]);
        run_git(worktree_path, &["commit", "-m", "agent work"]);

        let report = cleanup_terminal_agent_workspace_local_artifacts(&project, &workspace, false)
            .await
            .expect("cleanup should succeed");

        assert!(report.worktree_removed);
        assert!(!report.branch_deleted);
        assert!(!worktree_path.exists());
        assert!(branch_exists(repo.path(), branch));
    }

    #[tokio::test]
    async fn merged_agent_workspace_cleanup_keeps_dirty_worktree_and_branch() {
        let repo = init_repo();
        let worktrees = tempfile::tempdir().expect("worktree parent");
        let project = project_for(repo.path(), worktrees.path());
        let branch = "ralphx/cleanup/agent-dirty";
        let workspace = workspace_for(&project, branch, "merged");
        let worktree_path = Path::new(&workspace.worktree_path);

        GitService::create_worktree(repo.path(), worktree_path, branch, "main")
            .await
            .expect("create worktree");
        std::fs::write(worktree_path.join("dirty.txt"), "dirty\n").expect("write dirty file");

        let report = cleanup_terminal_agent_workspace_local_artifacts(&project, &workspace, true)
            .await
            .expect("cleanup should succeed");

        assert!(!report.worktree_removed);
        assert!(!report.branch_deleted);
        assert_eq!(
            report.skipped_reason.as_deref(),
            Some("workspace_has_uncommitted_changes")
        );
        assert!(worktree_path.exists());
        assert!(branch_exists(repo.path(), branch));
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

        let branch = "ralphx/cleanup/not-directory";
        let workspace = workspace_for(&project, branch, "merged");
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

    #[tokio::test]
    async fn merged_agent_workspace_cleanup_removes_clean_worktree_and_merged_branch() {
        let repo = init_repo();
        let worktrees = tempfile::tempdir().expect("worktree parent");
        let project = project_for(repo.path(), worktrees.path());
        let branch = "ralphx/cleanup/agent-merged";
        let workspace = workspace_for(&project, branch, "merged");
        let worktree_path = Path::new(&workspace.worktree_path);

        GitService::create_worktree(repo.path(), worktree_path, branch, "main")
            .await
            .expect("create worktree");
        std::fs::write(worktree_path.join("agent.txt"), "agent\n").expect("write agent");
        run_git(worktree_path, &["add", "."]);
        run_git(worktree_path, &["commit", "-m", "agent work"]);
        run_git(
            repo.path(),
            &["merge", "--no-ff", branch, "-m", "merge agent"],
        );

        let report = cleanup_terminal_agent_workspace_local_artifacts(&project, &workspace, true)
            .await
            .expect("cleanup should succeed");

        assert!(report.worktree_removed);
        assert!(report.branch_deleted);
        assert!(!worktree_path.exists());
        assert!(!branch_exists(repo.path(), branch));
    }
}
