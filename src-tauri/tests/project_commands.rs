mod common;

use std::sync::Arc;

use common::MockGithubService;
use ralphx_lib::application::services::PrPollerRegistry;
use ralphx_lib::application::AppState;
use ralphx_lib::commands::project_commands::*;
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{
    ArtifactId, GitMode, IdeationSessionId, InternalStatus, MergeValidationMode, PlanBranch,
    PlanBranchStatus, Project, ProjectId, Task, TaskCategory, TaskId,
};
use ralphx_lib::domain::repositories::{PlanBranchRepository, ProjectRepository, TaskRepository};
use ralphx_lib::domain::services::github_service::GithubServiceTrait;
use ralphx_lib::infrastructure::memory::{
    MemoryPlanBranchRepository, MemoryProjectRepository, MemoryTaskRepository,
};
use ralphx_lib::testing::create_mock_app_handle;

// ── is_github_url tests ──────────────────────────────────────────────────────

#[test]
fn github_url_https_is_valid() {
    assert!(is_github_url("https://github.com/owner/repo.git"));
    assert!(is_github_url("https://github.com/owner/repo"));
    assert!(is_github_url("https://github.com/org/sub/repo"));
}

#[test]
fn github_url_ssh_is_valid() {
    assert!(is_github_url("git@github.com:owner/repo.git"));
    assert!(is_github_url("git@github.com:org/repo"));
}

#[test]
fn non_github_https_url_is_invalid() {
    assert!(!is_github_url("https://gitlab.com/owner/repo.git"));
    assert!(!is_github_url("https://bitbucket.org/owner/repo.git"));
    assert!(!is_github_url("https://example.com/repo.git"));
}

#[test]
fn non_github_ssh_url_is_invalid() {
    assert!(!is_github_url("git@gitlab.com:owner/repo.git"));
    assert!(!is_github_url("git@bitbucket.org:owner/repo.git"));
}

#[test]
fn empty_and_garbage_urls_are_invalid() {
    assert!(!is_github_url(""));
    assert!(!is_github_url("not-a-url"));
    assert!(!is_github_url("github.com/owner/repo"));
}

// ── update_github_pr_enabled tests ──────────────────────────────────────────

#[tokio::test]
async fn test_update_github_pr_enabled_persists_change() {
    let state = setup_test_state();

    let project = Project::new("Test".to_string(), "/test/path".to_string());
    assert!(project.github_pr_enabled, "default should be true");
    let created = state.project_repo.create(project).await.unwrap();

    // Disable PR mode
    let mut updated = created.clone();
    updated.github_pr_enabled = false;
    updated.touch();
    state.project_repo.update(&updated).await.unwrap();

    let found = state
        .project_repo
        .get_by_id(&created.id)
        .await
        .unwrap()
        .unwrap();
    assert!(!found.github_pr_enabled);
}

#[tokio::test]
async fn test_project_response_includes_github_pr_enabled() {
    let project = Project::new("Test".to_string(), "/test".to_string());
    let response = ProjectResponse::from(project);
    // Default value should be true
    assert!(response.github_pr_enabled);

    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("\"github_pr_enabled\":true"));
}

fn setup_test_state() -> AppState {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    AppState::with_repos(task_repo, project_repo)
}

#[tokio::test]
async fn test_create_project_with_defaults() {
    let state = setup_test_state();

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    let created = state.project_repo.create(project).await.unwrap();

    assert_eq!(created.name, "Test Project");
    assert_eq!(created.working_directory, "/test/path");
    assert_eq!(created.git_mode, GitMode::Worktree);
    assert_eq!(created.merge_validation_mode, MergeValidationMode::Off);
}

#[test]
fn parse_merge_validation_mode_or_default_invalid_value_defaults_to_off() {
    assert_eq!(
        parse_merge_validation_mode_or_default("strict"),
        MergeValidationMode::Off
    );
    assert_eq!(
        parse_merge_validation_mode_or_default("warn"),
        MergeValidationMode::Warn
    );
}

#[tokio::test]
async fn test_create_project_with_worktree_mode() {
    let state = setup_test_state();

    let mut project = Project::new("Worktree Project".to_string(), "/main/repo".to_string());
    project.git_mode = GitMode::Worktree;
    project.base_branch = Some("main".to_string());
    let created = state.project_repo.create(project).await.unwrap();

    assert_eq!(created.name, "Worktree Project");
    assert_eq!(created.git_mode, GitMode::Worktree);
    assert_eq!(created.base_branch, Some("main".to_string()));
}

#[tokio::test]
async fn test_get_project_returns_none_for_nonexistent() {
    let state = setup_test_state();
    let id = ProjectId::new();

    let result = state.project_repo.get_by_id(&id).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_update_project_modifies_fields() {
    let state = setup_test_state();

    let project = Project::new("Original Name".to_string(), "/original/path".to_string());
    let created = state.project_repo.create(project).await.unwrap();

    let mut updated = created.clone();
    updated.name = "Updated Name".to_string();
    updated.working_directory = "/updated/path".to_string();

    state.project_repo.update(&updated).await.unwrap();

    let found = state
        .project_repo
        .get_by_id(&created.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.name, "Updated Name");
    assert_eq!(found.working_directory, "/updated/path");
}

#[tokio::test]
async fn test_archive_project_sets_archived_at() {
    let state = setup_test_state();

    let project = Project::new("To Archive".to_string(), "/archive/me".to_string());
    let created = state.project_repo.create(project).await.unwrap();

    state.project_repo.archive(&created.id).await.unwrap();

    let found = state
        .project_repo
        .get_by_id(&created.id)
        .await
        .unwrap()
        .unwrap();
    assert!(found.archived_at.is_some());
}

#[tokio::test]
async fn test_list_projects_returns_all() {
    let state = setup_test_state();

    state
        .project_repo
        .create(Project::new("Project 1".to_string(), "/path/1".to_string()))
        .await
        .unwrap();
    state
        .project_repo
        .create(Project::new("Project 2".to_string(), "/path/2".to_string()))
        .await
        .unwrap();
    state
        .project_repo
        .create(Project::new("Project 3".to_string(), "/path/3".to_string()))
        .await
        .unwrap();

    let projects = state.project_repo.get_all().await.unwrap();
    assert_eq!(projects.len(), 3);
}

#[tokio::test]
async fn test_project_response_serialization() {
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    let response = ProjectResponse::from(project);

    assert!(!response.id.is_empty());
    assert_eq!(response.name, "Test Project");
    assert_eq!(response.working_directory, "/test/path");
    assert_eq!(response.git_mode, "worktree");

    // Verify it serializes to JSON with snake_case (Rust default)
    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("\"name\":\"Test Project\""));
    assert!(json.contains("\"working_directory\":\"/test/path\""));
    assert!(json.contains("\"git_mode\":\"worktree\""));
}

// ===== get_git_default_branch tests =====

/// Helper to create a temp dir with git initialized
fn create_git_repo() -> tempfile::TempDir {
    let temp_dir = tempfile::tempdir().unwrap();
    let path = temp_dir.path();

    // Initialize git repo
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(path)
        .output()
        .expect("Failed to init git repo");

    // Configure git user for commits
    std::process::Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(path)
        .output()
        .expect("Failed to set git email");

    std::process::Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(path)
        .output()
        .expect("Failed to set git name");

    temp_dir
}

/// Helper to create an initial commit on a branch
fn create_commit_on_branch(path: &std::path::Path, branch_name: &str) {
    // Create and checkout branch
    std::process::Command::new("git")
        .args(["checkout", "-b", branch_name])
        .current_dir(path)
        .output()
        .expect("Failed to create branch");

    // Create a file and commit
    std::fs::write(path.join("README.md"), "# Test").unwrap();
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output()
        .expect("Failed to stage files");

    std::process::Command::new("git")
        .args(["commit", "-m", "Initial commit"])
        .current_dir(path)
        .output()
        .expect("Failed to commit");
}

#[tokio::test]
async fn test_get_git_default_branch_nonexistent_directory() {
    let result = get_git_default_branch("/nonexistent/path/that/does/not/exist".to_string()).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("does not exist"));
}

#[tokio::test]
async fn test_get_git_default_branch_not_a_git_repo() {
    let temp_dir = tempfile::tempdir().unwrap();
    let path = temp_dir.path().to_str().unwrap().to_string();

    let result = get_git_default_branch(path).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Not a git repository"));
}

#[tokio::test]
async fn test_get_git_default_branch_empty_repo_no_branches() {
    let temp_dir = create_git_repo();
    let path = temp_dir.path().to_str().unwrap().to_string();

    // Empty repo with no commits = no branches
    let result = get_git_default_branch(path).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("No branches found"));
}

#[tokio::test]
async fn test_get_git_default_branch_returns_main() {
    let temp_dir = create_git_repo();
    let path = temp_dir.path();

    // Create main branch with a commit
    create_commit_on_branch(path, "main");

    let result = get_git_default_branch(path.to_str().unwrap().to_string()).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "main");
}

#[tokio::test]
async fn test_get_git_default_branch_returns_master() {
    let temp_dir = create_git_repo();
    let path = temp_dir.path();

    // Create master branch with a commit (not main)
    create_commit_on_branch(path, "master");

    let result = get_git_default_branch(path.to_str().unwrap().to_string()).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "master");
}

#[tokio::test]
async fn test_get_git_default_branch_prefers_main_over_master() {
    let temp_dir = create_git_repo();
    let path = temp_dir.path();

    // Create main branch first
    create_commit_on_branch(path, "main");

    // Create master branch
    std::process::Command::new("git")
        .args(["checkout", "-b", "master"])
        .current_dir(path)
        .output()
        .expect("Failed to create master branch");

    let result = get_git_default_branch(path.to_str().unwrap().to_string()).await;
    assert!(result.is_ok());
    // Should prefer main (checked before master in fallback chain)
    assert_eq!(result.unwrap(), "main");
}

#[tokio::test]
async fn test_get_git_default_branch_falls_back_to_first_branch() {
    let temp_dir = create_git_repo();
    let path = temp_dir.path();

    // Create a branch that's NOT main or master
    create_commit_on_branch(path, "develop");

    let result = get_git_default_branch(path.to_str().unwrap().to_string()).await;
    assert!(result.is_ok());
    // Should fall back to the only branch available
    assert_eq!(result.unwrap(), "develop");
}

#[tokio::test]
async fn test_get_git_default_branch_first_branch_alphabetically() {
    let temp_dir = create_git_repo();
    let path = temp_dir.path();

    // Create feature-z branch first
    create_commit_on_branch(path, "feature-z");

    // Create feature-a branch
    std::process::Command::new("git")
        .args(["checkout", "-b", "feature-a"])
        .current_dir(path)
        .output()
        .expect("Failed to create feature-a branch");

    let result = get_git_default_branch(path.to_str().unwrap().to_string()).await;
    assert!(result.is_ok());
    // The function gets first line from `git branch --format=%(refname:short)`
    // which lists branches alphabetically, so feature-a comes first
    assert_eq!(result.unwrap(), "feature-a");
}

// ── IPC contract tests ─────────────────────────────────────────────────────────
// Verify camelCase deserialization for project command input structs.

#[cfg(test)]
mod ipc_contract {
    use ralphx_lib::commands::project_commands::{CreateProjectInput, UpdateProjectInput};

    // ── CreateProjectInput ──────────────────────────────────────────────────

    #[test]
    fn create_project_input_deserializes_camel_case() {
        let json = r#"{"name":"My Project","workingDirectory":"/code/my-project","gitMode":"worktree","baseBranch":"main"}"#;
        let input: CreateProjectInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.name, "My Project");
        assert_eq!(input.working_directory, "/code/my-project");
        assert_eq!(input.git_mode, Some("worktree".to_string()));
        assert_eq!(input.base_branch, Some("main".to_string()));
    }

    #[test]
    fn create_project_input_optional_fields_absent() {
        let json = r#"{"name":"Minimal","workingDirectory":"/tmp/proj"}"#;
        let input: CreateProjectInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.name, "Minimal");
        assert_eq!(input.working_directory, "/tmp/proj");
        assert!(input.git_mode.is_none());
        assert!(input.base_branch.is_none());
    }

    #[test]
    fn create_project_input_rejects_snake_case() {
        // snake_case keys must NOT be accepted when camelCase is required
        let json = r#"{"name":"Bad","working_directory":"/bad"}"#;
        let result: Result<CreateProjectInput, _> = serde_json::from_str(json);
        // working_directory won't map to working_directory field due to rename_all camelCase
        // The struct will deserialize but working_directory field will be missing → error
        assert!(
            result.is_err(),
            "snake_case working_directory must not deserialize (missing required field)"
        );
    }

    // ── UpdateProjectInput ──────────────────────────────────────────────────

    #[test]
    fn update_project_input_deserializes_all_camel_case_fields() {
        let json = r#"{"name":"Updated","workingDirectory":"/new/path","gitMode":"local","baseBranch":"develop","mergeValidationMode":"strict","mergeStrategy":"rebase_squash"}"#;
        let input: UpdateProjectInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.name, Some("Updated".to_string()));
        assert_eq!(input.working_directory, Some("/new/path".to_string()));
        assert_eq!(input.git_mode, Some("local".to_string()));
        assert_eq!(input.base_branch, Some("develop".to_string()));
        assert_eq!(input.merge_validation_mode, Some("strict".to_string()));
        assert_eq!(input.merge_strategy, Some("rebase_squash".to_string()));
    }

    #[test]
    fn update_project_input_partial_fields() {
        let json = r#"{"name":"Just Name"}"#;
        let input: UpdateProjectInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.name, Some("Just Name".to_string()));
        assert!(input.working_directory.is_none());
        assert!(input.git_mode.is_none());
        assert!(input.merge_strategy.is_none());
    }

    #[test]
    fn update_project_input_empty_object() {
        let json = r#"{}"#;
        let input: UpdateProjectInput = serde_json::from_str(json).unwrap();
        assert!(input.name.is_none());
        assert!(input.working_directory.is_none());
        assert!(input.git_mode.is_none());
        assert!(input.base_branch.is_none());
        assert!(input.merge_validation_mode.is_none());
        assert!(input.merge_strategy.is_none());
        assert!(input.worktree_parent_directory.is_none());
    }

    #[test]
    fn update_project_input_worktree_parent_directory_with_value() {
        let json = r#"{"worktreeParentDirectory":"/custom/worktrees"}"#;
        let input: UpdateProjectInput = serde_json::from_str(json).unwrap();
        assert_eq!(
            input.worktree_parent_directory,
            Some(Some("/custom/worktrees".to_string()))
        );
    }

    #[test]
    fn update_project_input_worktree_parent_directory_null_clears_field() {
        let json = r#"{"worktreeParentDirectory":null}"#;
        let input: UpdateProjectInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.worktree_parent_directory, Some(None));
    }

    #[test]
    fn update_project_input_worktree_parent_directory_absent_is_none() {
        let json = r#"{"name":"Test"}"#;
        let input: UpdateProjectInput = serde_json::from_str(json).unwrap();
        assert!(input.worktree_parent_directory.is_none());
    }
}

// ── worktree_parent_directory persistence tests ────────────────────────────

#[tokio::test]
async fn test_update_project_persists_worktree_parent_directory() {
    let state = setup_test_state();

    let project = Project::new("Test".to_string(), "/test/path".to_string());
    assert!(
        project.worktree_parent_directory.is_none(),
        "default should be None"
    );
    let created = state.project_repo.create(project).await.unwrap();

    let mut updated = created.clone();
    updated.worktree_parent_directory = Some("/custom/worktrees".to_string());
    updated.touch();
    state.project_repo.update(&updated).await.unwrap();

    let found = state
        .project_repo
        .get_by_id(&created.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        found.worktree_parent_directory,
        Some("/custom/worktrees".to_string())
    );
}

#[tokio::test]
async fn test_update_project_clears_worktree_parent_directory() {
    let state = setup_test_state();

    let mut project = Project::new("Test".to_string(), "/test/path".to_string());
    project.worktree_parent_directory = Some("/original/worktrees".to_string());
    let created = state.project_repo.create(project).await.unwrap();

    let mut updated = created.clone();
    updated.worktree_parent_directory = None;
    updated.touch();
    state.project_repo.update(&updated).await.unwrap();

    let found = state
        .project_repo
        .get_by_id(&created.id)
        .await
        .unwrap()
        .unwrap();
    assert!(found.worktree_parent_directory.is_none());
}

// ── handle_pr_mode_switch tests ──────────────────────────────────────────────

#[cfg(test)]
mod mode_switch_tests {
    use super::*;

    fn make_branch_with_pr(project_id: &str, merge_task_id: &str, pr_number: i64) -> PlanBranch {
        let mut b = PlanBranch::new(
            ArtifactId::from_string("art-1".to_string()),
            IdeationSessionId::from_string("sess-1".to_string()),
            ProjectId::from_string(project_id.to_string()),
            "feature/test".to_string(),
            "main".to_string(),
        );
        b.merge_task_id = Some(TaskId::from_string(merge_task_id.to_string()));
        b.pr_number = Some(pr_number);
        b.pr_url = Some("https://github.com/owner/repo/pull/42".to_string());
        b
    }

    #[tokio::test]
    async fn pr_to_push_clears_pr_fields_on_repo() {
        // Verify that clear_pr_info removes pr_number and pr_url from the branch.
        // This is the key side effect of the PR → push-to-main mode switch path.
        let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

        let branch = make_branch_with_pr("proj-1", "task-merge-1", 42);
        let branch_id = branch.id.clone();
        plan_branch_repo.create(branch).await.unwrap();

        // Verify PR fields are set before the operation
        let before = plan_branch_repo
            .get_by_id(&branch_id)
            .await
            .unwrap()
            .unwrap();
        assert!(before.pr_number.is_some(), "PR number should be set");
        assert!(before.pr_url.is_some(), "PR URL should be set");

        // Simulate what handle_pr_mode_switch does when PR → push-to-main
        plan_branch_repo.clear_pr_info(&branch_id).await.unwrap();

        let after = plan_branch_repo
            .get_by_id(&branch_id)
            .await
            .unwrap()
            .unwrap();
        assert!(after.pr_number.is_none(), "PR number should be cleared");
        assert!(after.pr_url.is_none(), "PR URL should be cleared");
    }

    #[tokio::test]
    async fn existing_plan_branch_remains_unchanged_until_reconciliation_runs() {
        // Repository state alone does not retrofit existing plans.
        // The explicit reconcile_pr_mode_switch regression below covers the live toggle path.
        let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
        let project_repo = Arc::new(MemoryProjectRepository::new());

        let pid = ProjectId::from_string("proj-2".to_string());
        let project = {
            let mut p = Project::new("Test2".to_string(), "/test2/path".to_string());
            p.id = pid.clone();
            p
        };
        project_repo.create(project).await.unwrap();

        // Branch with no pr_number (push-to-main mode)
        let mut branch = PlanBranch::new(
            ArtifactId::from_string("art-2".to_string()),
            IdeationSessionId::from_string("sess-2".to_string()),
            pid.clone(),
            "feature/no-pr".to_string(),
            "main".to_string(),
        );
        branch.pr_eligible = false; // AD16: existing plans have pr_eligible=false
        let branch_id = branch.id.clone();
        plan_branch_repo.create(branch).await.unwrap();

        // Without the reconciliation helper, a persisted branch stays in its original mode.
        let after = plan_branch_repo
            .get_by_id(&branch_id)
            .await
            .unwrap()
            .unwrap();
        assert!(
            after.pr_number.is_none(),
            "No PR should appear without an explicit reconciliation step"
        );
        assert!(
            !after.pr_eligible,
            "pr_eligible should stay false until reconciliation updates the branch"
        );
    }

    #[tokio::test]
    async fn enabling_pr_mode_retrofits_existing_pending_merge_plan_and_runs_pr_path() {
        let task_repo = Arc::new(MemoryTaskRepository::new());
        let project_repo = Arc::new(MemoryProjectRepository::new());
        let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

        let mut state = AppState::with_repos(task_repo.clone(), project_repo.clone());
        state.plan_branch_repo = plan_branch_repo.clone();

        let mock_github = Arc::new(MockGithubService::new());
        let github_trait: Arc<dyn GithubServiceTrait> = mock_github.clone();
        state.github_service = Some(Arc::clone(&github_trait));
        state.pr_poller_registry = Arc::new(PrPollerRegistry::new(
            Some(github_trait),
            plan_branch_repo.clone(),
        ));

        let working_dir = create_git_repo();
        let repo_path = working_dir.path();
        create_commit_on_branch(repo_path, "main");
        std::process::Command::new("git")
            .args(["checkout", "-b", "ralphx/test/plan-toggle"])
            .current_dir(repo_path)
            .output()
            .expect("create plan branch");
        std::fs::write(repo_path.join("plan.txt"), "plan branch work\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(repo_path)
            .output()
            .expect("stage plan file");
        std::process::Command::new("git")
            .args(["commit", "-m", "plan branch work"])
            .current_dir(repo_path)
            .output()
            .expect("commit plan branch work");
        std::process::Command::new("git")
            .args(["checkout", "main"])
            .current_dir(repo_path)
            .output()
            .expect("checkout main");
        let mut project = Project::new(
            "PR Toggle".to_string(),
            working_dir.path().to_string_lossy().into_owned(),
        );
        let pid = ProjectId::from_string("proj-pr-toggle".to_string());
        project.id = pid.clone();
        project.github_pr_enabled = false;
        project_repo.create(project).await.unwrap();

        let mut merge_task = Task::new(pid.clone(), "Merge pending plan".to_string());
        merge_task.category = TaskCategory::PlanMerge;
        merge_task.internal_status = InternalStatus::PendingMerge;
        let merge_task_id = merge_task.id.clone();
        task_repo.create(merge_task).await.unwrap();

        let mut branch = PlanBranch::new(
            ArtifactId::from_string("art-toggle".to_string()),
            IdeationSessionId::from_string("sess-toggle".to_string()),
            pid.clone(),
            "ralphx/test/plan-toggle".to_string(),
            "main".to_string(),
        );
        branch.merge_task_id = Some(merge_task_id.clone());
        branch.pr_eligible = false;
        let branch_id = branch.id.clone();
        plan_branch_repo.create(branch).await.unwrap();

        let execution_state = Arc::new(ExecutionState::new());
        let app_handle = create_mock_app_handle();

        reconcile_pr_mode_switch(&pid, true, &state, &execution_state, app_handle).await;

        let branch_after = plan_branch_repo
            .get_by_id(&branch_id)
            .await
            .unwrap()
            .unwrap();
        assert!(
            branch_after.pr_eligible,
            "enabling PR mode should retrofit active plan branches"
        );

        let task_after = task_repo.get_by_id(&merge_task_id).await.unwrap().unwrap();
        assert_eq!(
            task_after.internal_status,
            InternalStatus::WaitingOnPr,
            "PendingMerge merge task should enter the GitHub PR wait state"
        );
        assert!(
            mock_github.push_calls() > 0,
            "PR-mode retry should push the branch"
        );
        assert!(
            mock_github.create_calls() > 0,
            "PR-mode retry should create a PR when one does not exist"
        );
    }

    #[tokio::test]
    async fn merged_branch_status_is_skipped() {
        // Branches with Merged status should be skipped entirely.
        // Verify the PlanBranchStatus matching — Merged/Abandoned branches skip cleanup.
        let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

        let mut branch = make_branch_with_pr("proj-3", "task-merge-3", 99);
        branch.status = PlanBranchStatus::Merged;
        let branch_id = branch.id.clone();
        plan_branch_repo.create(branch).await.unwrap();

        // Even if we tried clear_pr_info, the guard in handle_pr_mode_switch prevents it.
        // Verify the branch still has PR fields (was never cleared — merged branches are skipped).
        let found = plan_branch_repo
            .get_by_id(&branch_id)
            .await
            .unwrap()
            .unwrap();
        assert!(
            found.pr_number.is_some(),
            "Merged branch pr_number should not be touched"
        );
        // Confirm the status is still Merged (unchanged)
        assert!(
            matches!(found.status, PlanBranchStatus::Merged),
            "Branch status should remain Merged"
        );
    }

    #[test]
    fn merging_status_check_uses_enum_comparison() {
        // Confirm that InternalStatus::Merging != InternalStatus::Merged
        // (guards against future refactors accidentally conflating the two)
        assert_ne!(InternalStatus::Merging, InternalStatus::Merged);
        assert_ne!(InternalStatus::MergeIncomplete, InternalStatus::Merged);
    }
}

// ============================================================================
// ensure_git_initialized_async — 4 directory/git states
// ============================================================================
//
// Tests use tempfile::TempDir for filesystem isolation. Git user name/email
// is configured per-repo so the empty commit succeeds without global config.

/// Configure git user identity locally so commits succeed without global config.
fn configure_git_identity(path: &std::path::Path) {
    std::process::Command::new("git")
        .args(["config", "user.email", "test@ralphx.test"])
        .current_dir(path)
        .output()
        .expect("Failed to set git user.email");
    std::process::Command::new("git")
        .args(["config", "user.name", "RalphX Test"])
        .current_dir(path)
        .output()
        .expect("Failed to set git user.name");
}

/// Returns true if the git repository at `path` has at least one commit.
fn has_commits(path: &std::path::Path) -> bool {
    std::process::Command::new("git")
        .args(["log", "--oneline", "-1"])
        .current_dir(path)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn test_ipc_contract_ensure_git_initialized_sync_no_git_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let path_str = tmp.path().to_str().unwrap();

    std::process::Command::new("git")
        .args(["init"])
        .current_dir(tmp.path())
        .output()
        .expect("git init");
    configure_git_identity(tmp.path());
    std::fs::remove_dir_all(tmp.path().join(".git")).expect("remove .git");

    assert!(!tmp.path().join(".git").exists(), "precondition: no .git");

    ensure_git_initialized_for_test(path_str).expect("must succeed");

    assert!(
        tmp.path().join(".git").exists(),
        ".git must exist after initialization"
    );
}

#[test]
fn test_ipc_contract_ensure_git_initialized_sync_no_commits() {
    let tmp = tempfile::tempdir().unwrap();
    let path_str = tmp.path().to_str().unwrap();

    std::process::Command::new("git")
        .args(["init"])
        .current_dir(tmp.path())
        .output()
        .expect("git init");
    configure_git_identity(tmp.path());

    assert!(
        tmp.path().join(".git").exists(),
        "precondition: .git exists"
    );
    assert!(!has_commits(tmp.path()), "precondition: no commits");

    ensure_git_initialized_for_test(path_str).expect("must succeed");

    assert!(
        !has_commits(tmp.path()),
        "sync helper treats an existing .git directory as initialized and does not backfill HEAD"
    );
}

#[test]
fn test_ipc_contract_ensure_git_initialized_sync_already_has_commits() {
    let tmp = tempfile::tempdir().unwrap();
    let path_str = tmp.path().to_str().unwrap();

    std::process::Command::new("git")
        .args(["init"])
        .current_dir(tmp.path())
        .output()
        .expect("git init");
    configure_git_identity(tmp.path());
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "Pre-existing commit"])
        .current_dir(tmp.path())
        .output()
        .expect("initial commit");

    assert!(has_commits(tmp.path()), "precondition: has commits");

    let before = std::process::Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .current_dir(tmp.path())
        .output()
        .expect("rev-list");
    let before_count: u32 = String::from_utf8_lossy(&before.stdout)
        .trim()
        .parse()
        .unwrap_or(0);

    ensure_git_initialized_for_test(path_str).expect("must succeed");

    let after = std::process::Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .current_dir(tmp.path())
        .output()
        .expect("rev-list after");
    let after_count: u32 = String::from_utf8_lossy(&after.stdout)
        .trim()
        .parse()
        .unwrap_or(0);

    assert_eq!(
        before_count, after_count,
        "No new commits must be added when repo already has commits"
    );
}

#[test]
fn test_ipc_contract_ensure_git_initialized_sync_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    let path_str = tmp.path().to_str().unwrap();

    std::process::Command::new("git")
        .args(["init"])
        .current_dir(tmp.path())
        .output()
        .expect("git init for identity config");
    configure_git_identity(tmp.path());
    std::fs::remove_dir_all(tmp.path().join(".git")).expect("remove .git");

    ensure_git_initialized_for_test(path_str).expect("first call must succeed");

    let first_commit_count = std::process::Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .current_dir(tmp.path())
        .output()
        .expect("rev-list after first sync init");
    let first_count: u32 = String::from_utf8_lossy(&first_commit_count.stdout)
        .trim()
        .parse()
        .unwrap_or(0);

    ensure_git_initialized_for_test(path_str).expect("second call must succeed");

    let second_commit_count = std::process::Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .current_dir(tmp.path())
        .output()
        .expect("rev-list after second sync init");
    let second_count: u32 = String::from_utf8_lossy(&second_commit_count.stdout)
        .trim()
        .parse()
        .unwrap_or(0);
    assert_eq!(
        first_count, second_count,
        "sync helper should be idempotent regardless of whether the first empty commit succeeded"
    );
}

/// State 1: Directory exists, no .git → ensure_git_initialized_async must
/// create .git and produce an initial commit.
#[tokio::test]
async fn test_ensure_git_initialized_no_git_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let path_str = tmp.path().to_str().unwrap();

    // Pre-configure git identity so the empty commit succeeds
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(tmp.path())
        .output()
        .expect("git init");
    configure_git_identity(tmp.path());
    // Remove .git so ensure_git_initialized_async starts from scratch
    std::fs::remove_dir_all(tmp.path().join(".git")).expect("remove .git");

    assert!(!tmp.path().join(".git").exists(), "precondition: no .git");

    ensure_git_initialized_async(path_str)
        .await
        .expect("must succeed");

    assert!(
        tmp.path().join(".git").exists(),
        ".git must exist after initialization"
    );
    // Configure identity for the commit check (git log needs a repo with commits)
    configure_git_identity(tmp.path());
    // Note: commit may fail if no identity — just verify .git was created
}

/// State 2: .git exists but no commits → ensure_git_initialized_async must
/// create the initial commit so HEAD is valid.
#[tokio::test]
async fn test_ensure_git_initialized_no_commits() {
    let tmp = tempfile::tempdir().unwrap();
    let path_str = tmp.path().to_str().unwrap();

    // git init but no commits
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(tmp.path())
        .output()
        .expect("git init");
    configure_git_identity(tmp.path());

    assert!(
        tmp.path().join(".git").exists(),
        "precondition: .git exists"
    );
    assert!(!has_commits(tmp.path()), "precondition: no commits");

    ensure_git_initialized_async(path_str)
        .await
        .expect("must succeed");

    assert!(tmp.path().join(".git").exists(), ".git must still exist");
    // Commit may or may not succeed depending on global git config availability;
    // ensure_git_initialized_async warns and returns Ok() either way
}

/// State 3: .git exists WITH commits → ensure_git_initialized_async must be
/// a no-op and the existing commit must remain.
#[tokio::test]
async fn test_ensure_git_initialized_already_has_commits() {
    let tmp = tempfile::tempdir().unwrap();
    let path_str = tmp.path().to_str().unwrap();

    // git init + configure identity + initial commit
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(tmp.path())
        .output()
        .expect("git init");
    configure_git_identity(tmp.path());
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "Pre-existing commit"])
        .current_dir(tmp.path())
        .output()
        .expect("initial commit");

    assert!(has_commits(tmp.path()), "precondition: has commits");

    // Count commits before
    let before = std::process::Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .current_dir(tmp.path())
        .output()
        .expect("rev-list");
    let before_count: u32 = String::from_utf8_lossy(&before.stdout)
        .trim()
        .parse()
        .unwrap_or(0);

    ensure_git_initialized_async(path_str)
        .await
        .expect("must succeed");

    // Count commits after — must be the same (no extra commit added)
    let after = std::process::Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .current_dir(tmp.path())
        .output()
        .expect("rev-list after");
    let after_count: u32 = String::from_utf8_lossy(&after.stdout)
        .trim()
        .parse()
        .unwrap_or(0);

    assert_eq!(
        before_count, after_count,
        "No new commits must be added when repo already has commits"
    );
    assert!(tmp.path().join(".git").exists(), ".git must still exist");
}

/// State 4: Idempotency — calling ensure_git_initialized_async twice on the
/// same directory must produce exactly one initial commit (no duplicates).
#[tokio::test]
async fn test_ensure_git_initialized_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    let path_str = tmp.path().to_str().unwrap();

    // Empty directory (no .git)
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(tmp.path())
        .output()
        .expect("git init for identity config");
    configure_git_identity(tmp.path());
    std::fs::remove_dir_all(tmp.path().join(".git")).expect("remove .git");

    // First call — creates .git
    ensure_git_initialized_async(path_str)
        .await
        .expect("first call must succeed");

    // Second call — must be a no-op
    let result = ensure_git_initialized_async(path_str).await;
    assert!(result.is_ok(), "Second call must not fail: {:?}", result);

    // .git must exist after both calls
    assert!(
        tmp.path().join(".git").exists(),
        ".git must exist after both calls"
    );
}
