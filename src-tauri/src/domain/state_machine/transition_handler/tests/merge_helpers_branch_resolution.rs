// Tests for merge_helpers: resolve_task_base_branch + resolve_merge_branches
//
// Extracted from side_effects.rs (lines 5722–6106).

use super::super::merge_helpers::{
    discover_and_attach_task_branch, resolve_effective_base_branch, resolve_merge_branches,
    resolve_task_base_branch,
};
use super::helpers::*;
use crate::domain::entities::{ExecutionPlanId, PlanBranchStatus, TaskId};
use crate::domain::repositories::{PlanBranchRepository, TaskRepository};
use crate::infrastructure::memory::{MemoryPlanBranchRepository, MemoryTaskRepository};
use std::sync::Arc;

// ==================
// resolve_task_base_branch tests
// ==================

#[tokio::test]
async fn resolve_task_base_branch_returns_project_base_when_no_repo() {
    let project = make_project(Some("develop"));
    let task = make_task_with_session(Some("art-1"), None, Some("sess-1"));
    let repo: Option<Arc<dyn PlanBranchRepository>> = None;

    let result = resolve_task_base_branch(&task, &project, &repo, &None, &None, &None, &None).await;
    assert_eq!(result, "develop");
}

#[tokio::test]
async fn resolve_task_base_branch_defaults_to_main_when_no_base_branch() {
    let project = make_project(None);
    let task = make_task_with_session(Some("art-1"), None, Some("sess-1"));
    let repo: Option<Arc<dyn PlanBranchRepository>> = None;

    let result = resolve_task_base_branch(&task, &project, &repo, &None, &None, &None, &None).await;
    assert_eq!(result, "main");
}

#[tokio::test]
async fn resolve_task_base_branch_returns_default_when_task_has_no_session_id() {
    let project = make_project(Some("develop"));
    let task = make_task(None, None);
    let mem_repo = Arc::new(MemoryPlanBranchRepository::new());
    let repo: Option<Arc<dyn PlanBranchRepository>> = Some(mem_repo);

    let result = resolve_task_base_branch(&task, &project, &repo, &None, &None, &None, &None).await;
    assert_eq!(result, "develop");
}

#[tokio::test]
async fn resolve_task_base_branch_falls_back_when_branch_creation_fails() {
    // Project points to /tmp/test which is not a real git repo,
    // so lazy branch creation will fail → should fall back to "main"
    let project = make_project(Some("main"));
    let task = make_task_with_session(Some("art-1"), None, Some("sess-1"));

    let mem_repo = Arc::new(MemoryPlanBranchRepository::new());
    let pb = make_plan_branch(
        "art-1",
        "ralphx/test/plan-abc123",
        PlanBranchStatus::Active,
        None,
    );
    mem_repo.create(pb).await.unwrap();

    let repo: Option<Arc<dyn PlanBranchRepository>> = Some(mem_repo);
    let result = resolve_task_base_branch(&task, &project, &repo, &None, &None, &None, &None).await;
    assert_eq!(result, "main");
}

#[tokio::test]
async fn resolve_task_base_branch_returns_feature_branch_when_branch_exists() {
    // Set up a real git repo with the plan branch already created
    let tmp = tempfile::tempdir().unwrap();
    let repo_path = tmp.path();

    // Init git repo with an initial commit (needed for branch creation)
    let init = std::process::Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(repo_path)
        .output()
        .unwrap();
    assert!(
        init.status.success(),
        "git init should succeed: {}",
        String::from_utf8_lossy(&init.stderr)
    );

    let config_email = std::process::Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(repo_path)
        .output()
        .unwrap();
    assert!(
        config_email.status.success(),
        "git config user.email should succeed: {}",
        String::from_utf8_lossy(&config_email.stderr)
    );

    let config_name = std::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(repo_path)
        .output()
        .unwrap();
    assert!(
        config_name.status.success(),
        "git config user.name should succeed: {}",
        String::from_utf8_lossy(&config_name.stderr)
    );

    let commit = std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(repo_path)
        .output()
        .unwrap();
    assert!(
        commit.status.success(),
        "git commit should succeed: {}",
        String::from_utf8_lossy(&commit.stderr)
    );
    // Create the plan branch
    let branch = std::process::Command::new("git")
        .args(["branch", "ralphx/test/plan-abc123"])
        .current_dir(repo_path)
        .output()
        .unwrap();
    assert!(
        branch.status.success(),
        "git branch should succeed: {}",
        String::from_utf8_lossy(&branch.stderr)
    );

    let mut project = make_project(Some("main"));
    project.working_directory = repo_path.to_string_lossy().to_string();
    let task = make_task_with_session(Some("art-1"), None, Some("sess-1"));

    let mem_repo = Arc::new(MemoryPlanBranchRepository::new());
    let pb = make_plan_branch(
        "art-1",
        "ralphx/test/plan-abc123",
        PlanBranchStatus::Active,
        None,
    );
    mem_repo.create(pb).await.unwrap();

    let repo: Option<Arc<dyn PlanBranchRepository>> = Some(mem_repo);
    let result = resolve_task_base_branch(&task, &project, &repo, &None, &None, &None, &None).await;
    assert_eq!(result, "ralphx/test/plan-abc123");
}

#[tokio::test]
async fn resolve_task_base_branch_returns_default_when_branch_merged() {
    let project = make_project(Some("main"));
    let task = make_task_with_session(Some("art-1"), None, Some("sess-1"));

    let mem_repo = Arc::new(MemoryPlanBranchRepository::new());
    let pb = make_plan_branch(
        "art-1",
        "ralphx/test/plan-abc123",
        PlanBranchStatus::Merged,
        None,
    );
    mem_repo.create(pb).await.unwrap();

    let repo: Option<Arc<dyn PlanBranchRepository>> = Some(mem_repo);
    let result = resolve_task_base_branch(&task, &project, &repo, &None, &None, &None, &None).await;
    assert_eq!(result, "main");
}

#[tokio::test]
async fn resolve_task_base_branch_returns_default_when_branch_abandoned() {
    let project = make_project(Some("main"));
    let task = make_task_with_session(Some("art-1"), None, Some("sess-1"));

    let mem_repo = Arc::new(MemoryPlanBranchRepository::new());
    let pb = make_plan_branch(
        "art-1",
        "ralphx/test/plan-abc123",
        PlanBranchStatus::Abandoned,
        None,
    );
    mem_repo.create(pb).await.unwrap();

    let repo: Option<Arc<dyn PlanBranchRepository>> = Some(mem_repo);
    let result = resolve_task_base_branch(&task, &project, &repo, &None, &None, &None, &None).await;
    assert_eq!(result, "main");
}

#[tokio::test]
async fn resolve_task_base_branch_returns_default_when_no_matching_branch() {
    let project = make_project(Some("main"));
    // Task has session_id "sess-nonexistent" which won't match "sess-1" in plan branch
    let task = make_task_with_session(Some("art-nonexistent"), None, Some("sess-nonexistent"));

    let mem_repo = Arc::new(MemoryPlanBranchRepository::new());
    let pb = make_plan_branch(
        "art-other",
        "ralphx/test/plan-abc123",
        PlanBranchStatus::Active,
        None,
    );
    mem_repo.create(pb).await.unwrap();

    let repo: Option<Arc<dyn PlanBranchRepository>> = Some(mem_repo);
    let result = resolve_task_base_branch(&task, &project, &repo, &None, &None, &None, &None).await;
    assert_eq!(result, "main");
}

#[tokio::test]
async fn resolve_task_base_branch_uses_execution_plan_id_without_session_id() {
    let tmp = tempfile::tempdir().unwrap();
    let repo_path = tmp.path();

    std::process::Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(repo_path)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(repo_path)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(repo_path)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(repo_path)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["branch", "ralphx/test/plan-exec-id"])
        .current_dir(repo_path)
        .output()
        .unwrap();

    let mut project = make_project(Some("main"));
    project.working_directory = repo_path.to_string_lossy().to_string();

    let mut task = make_task(None, None);
    task.execution_plan_id = Some(ExecutionPlanId::from_string("exec-123"));
    task.ideation_session_id = None;

    let mem_repo = Arc::new(MemoryPlanBranchRepository::new());
    let mut pb = make_plan_branch(
        "art-exec-id",
        "ralphx/test/plan-exec-id",
        PlanBranchStatus::Active,
        None,
    );
    pb.execution_plan_id = Some(ExecutionPlanId::from_string("exec-123"));
    mem_repo.create(pb).await.unwrap();

    let repo: Option<Arc<dyn PlanBranchRepository>> = Some(mem_repo);
    let result = resolve_task_base_branch(&task, &project, &repo, &None, &None, &None, &None).await;
    assert_eq!(result, "ralphx/test/plan-exec-id");
}

// ==================
// stale merge_task_id cleanup tests
// ==================

#[tokio::test]
async fn resolve_task_base_branch_clears_stale_merge_task_id_when_task_deleted() {
    // Set up a real git repo so the Merged arm succeeds (branch exists in git)
    let tmp = tempfile::tempdir().unwrap();
    let repo_path = tmp.path();

    std::process::Command::new("git")
        .args(["init"])
        .current_dir(repo_path)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(repo_path)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["branch", "ralphx/test/plan-stale"])
        .current_dir(repo_path)
        .output()
        .unwrap();

    let mut project = make_project(Some("main"));
    project.working_directory = repo_path.to_string_lossy().to_string();
    let task = make_task_with_session(Some("art-1"), None, Some("sess-1"));

    // Plan branch with merge_task_id pointing to a non-existent task
    let plan_repo = Arc::new(MemoryPlanBranchRepository::new());
    let pb = make_plan_branch(
        "art-1",
        "ralphx/test/plan-stale",
        PlanBranchStatus::Merged,
        Some("deleted-merge-task-id"),
    );
    plan_repo.create(pb).await.unwrap();

    // Task repo is empty — the merge task has been deleted
    let task_repo: Arc<dyn TaskRepository> = Arc::new(MemoryTaskRepository::new());

    let plan_opt: Option<Arc<dyn PlanBranchRepository>> = Some(plan_repo.clone());
    let task_opt: Option<Arc<dyn TaskRepository>> = Some(task_repo);

    let result =
        resolve_task_base_branch(&task, &project, &plan_opt, &task_opt, &None, &None, &None).await;
    // Fix D: merged branches now fall back to project base (no resurrection)
    assert_eq!(result, "main");
}

#[tokio::test]
async fn resolve_task_base_branch_keeps_valid_merge_task_id() {
    // Set up a real git repo
    let tmp = tempfile::tempdir().unwrap();
    let repo_path = tmp.path();

    std::process::Command::new("git")
        .args(["init"])
        .current_dir(repo_path)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(repo_path)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["branch", "ralphx/test/plan-valid"])
        .current_dir(repo_path)
        .output()
        .unwrap();

    let mut project = make_project(Some("main"));
    project.working_directory = repo_path.to_string_lossy().to_string();
    let task = make_task_with_session(Some("art-1"), None, Some("sess-1"));

    let plan_repo = Arc::new(MemoryPlanBranchRepository::new());
    let pb = make_plan_branch(
        "art-1",
        "ralphx/test/plan-valid",
        PlanBranchStatus::Merged,
        Some("existing-merge-task"),
    );
    plan_repo.create(pb).await.unwrap();

    // Create the merge task in task repo so it exists
    let task_repo: Arc<dyn TaskRepository> = Arc::new(MemoryTaskRepository::new());
    let merge_task = make_task_with_status(
        "existing-merge-task",
        crate::domain::entities::InternalStatus::Blocked,
    );
    task_repo.create(merge_task).await.unwrap();

    let plan_opt: Option<Arc<dyn PlanBranchRepository>> = Some(plan_repo.clone());
    let task_opt: Option<Arc<dyn TaskRepository>> = Some(task_repo);

    let result =
        resolve_task_base_branch(&task, &project, &plan_opt, &task_opt, &None, &None, &None).await;
    // Fix D: merged branches now fall back to project base (no resurrection)
    assert_eq!(result, "main");
}

// ==================
// resolve_merge_branches tests
// ==================

#[tokio::test]
async fn resolve_merge_branches_returns_default_when_no_repo() {
    let project = make_project(Some("main"));
    let mut task = make_task(None, Some("ralphx/test/task-123"));
    task.id = TaskId::from_string("task-123".to_string());

    let repo: Option<Arc<dyn PlanBranchRepository>> = None;
    let (source, target) = resolve_merge_branches(&task, &project, &repo).await;
    assert_eq!(source, "ralphx/test/task-123");
    assert_eq!(target, "main");
}

#[tokio::test]
async fn resolve_merge_branches_merge_task_returns_feature_into_base() {
    let project = make_project(Some("main"));
    let mut task = make_task(None, None);
    task.id = TaskId::from_string("merge-task-1".to_string());

    let mem_repo = Arc::new(MemoryPlanBranchRepository::new());
    let pb = make_plan_branch(
        "art-1",
        "ralphx/test/plan-abc123",
        PlanBranchStatus::Active,
        Some("merge-task-1"),
    );
    mem_repo.create(pb).await.unwrap();

    let repo: Option<Arc<dyn PlanBranchRepository>> = Some(mem_repo);
    let (source, target) = resolve_merge_branches(&task, &project, &repo).await;
    assert_eq!(source, "ralphx/test/plan-abc123");
    assert_eq!(target, "main");
}

#[tokio::test]
async fn resolve_merge_branches_plan_task_returns_task_into_feature() {
    let project = make_project(Some("main"));
    let mut task =
        make_task_with_session(Some("art-1"), Some("ralphx/test/task-456"), Some("sess-1"));
    task.id = TaskId::from_string("task-456".to_string());

    let mem_repo = Arc::new(MemoryPlanBranchRepository::new());
    let pb = make_plan_branch(
        "art-1",
        "ralphx/test/plan-abc123",
        PlanBranchStatus::Active,
        None,
    );
    mem_repo.create(pb).await.unwrap();

    let repo: Option<Arc<dyn PlanBranchRepository>> = Some(mem_repo);
    let (source, target) = resolve_merge_branches(&task, &project, &repo).await;
    assert_eq!(source, "ralphx/test/task-456");
    assert_eq!(target, "ralphx/test/plan-abc123");
}

#[tokio::test]
async fn resolve_merge_branches_regular_task_returns_task_into_base() {
    let project = make_project(Some("develop"));
    let mut task = make_task(None, Some("ralphx/test/task-789"));
    task.id = TaskId::from_string("task-789".to_string());

    let mem_repo = Arc::new(MemoryPlanBranchRepository::new());
    let repo: Option<Arc<dyn PlanBranchRepository>> = Some(mem_repo);

    let (source, target) = resolve_merge_branches(&task, &project, &repo).await;
    assert_eq!(source, "ralphx/test/task-789");
    assert_eq!(target, "develop");
}

#[tokio::test]
async fn resolve_merge_branches_merge_task_with_merged_branch_returns_default() {
    let project = make_project(Some("main"));
    let mut task = make_task(None, Some("ralphx/test/task-merge"));
    task.id = TaskId::from_string("merge-task-2".to_string());

    let mem_repo = Arc::new(MemoryPlanBranchRepository::new());
    let pb = make_plan_branch(
        "art-2",
        "ralphx/test/plan-def456",
        PlanBranchStatus::Merged,
        Some("merge-task-2"),
    );
    mem_repo.create(pb).await.unwrap();

    let repo: Option<Arc<dyn PlanBranchRepository>> = Some(mem_repo);
    let (source, target) = resolve_merge_branches(&task, &project, &repo).await;
    // Merged branch is not Active, but still used as source to avoid incorrect merge direction
    assert_eq!(source, "ralphx/test/plan-def456");
    assert_eq!(target, "main");
}

#[tokio::test]
async fn resolve_merge_branches_plan_task_with_abandoned_branch_returns_default() {
    let project = make_project(Some("main"));
    let mut task = make_task_with_session(
        Some("art-3"),
        Some("ralphx/test/task-abandoned"),
        Some("sess-1"),
    );
    task.id = TaskId::from_string("task-abandoned".to_string());

    let mem_repo = Arc::new(MemoryPlanBranchRepository::new());
    let pb = make_plan_branch(
        "art-3",
        "ralphx/test/plan-ghi789",
        PlanBranchStatus::Abandoned,
        None,
    );
    mem_repo.create(pb).await.unwrap();

    let repo: Option<Arc<dyn PlanBranchRepository>> = Some(mem_repo);
    let (source, target) = resolve_merge_branches(&task, &project, &repo).await;
    // Abandoned branch is not Active, but still used as target to avoid incorrect task→main merge
    assert_eq!(source, "ralphx/test/task-abandoned");
    assert_eq!(target, "ralphx/test/plan-ghi789");
}

#[tokio::test]
async fn resolve_merge_branches_defaults_to_main_when_no_base_branch() {
    let project = make_project(None);
    let mut task = make_task(None, Some("ralphx/test/task-no-base"));
    task.id = TaskId::from_string("task-no-base".to_string());

    let repo: Option<Arc<dyn PlanBranchRepository>> = None;
    let (source, target) = resolve_merge_branches(&task, &project, &repo).await;
    assert_eq!(source, "ralphx/test/task-no-base");
    assert_eq!(target, "main");
}

#[tokio::test]
async fn resolve_merge_branches_merge_task_checked_before_plan_task() {
    // If a task is both a merge task AND has ideation_session_id,
    // merge task check should take precedence
    let project = make_project(Some("main"));
    let mut task =
        make_task_with_session(Some("art-1"), Some("ralphx/test/task-dual"), Some("sess-1"));
    task.id = TaskId::from_string("dual-task".to_string());

    let mem_repo = Arc::new(MemoryPlanBranchRepository::new());
    let pb = make_plan_branch(
        "art-1",
        "ralphx/test/plan-dual",
        PlanBranchStatus::Active,
        Some("dual-task"),
    );
    mem_repo.create(pb).await.unwrap();

    let repo: Option<Arc<dyn PlanBranchRepository>> = Some(mem_repo);
    let (source, target) = resolve_merge_branches(&task, &project, &repo).await;
    // Merge task path wins: feature branch into base
    assert_eq!(source, "ralphx/test/plan-dual");
    assert_eq!(target, "main");
}

#[tokio::test]
async fn resolve_merge_branches_after_branch_discovery_returns_valid_source() {
    // Set up a real git repo with a task branch
    let tmp = tempfile::tempdir().unwrap();
    let repo_path = tmp.path();

    // Init git repo with an initial commit
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(repo_path)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(repo_path)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(repo_path)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(repo_path)
        .output()
        .unwrap();

    // Create a task branch (simulating orphaned branch from failed task)
    let mut task = make_task(None, None); // task_branch is None
    task.id = TaskId::from_string("orphaned-123".to_string());
    let expected_branch = format!("ralphx/test-project/task-{}", task.id.as_str());

    std::process::Command::new("git")
        .args(["branch", &expected_branch])
        .current_dir(repo_path)
        .output()
        .unwrap();

    // Create project pointing to real git repo
    let mut project = make_project(Some("main"));
    project.working_directory = repo_path.to_string_lossy().to_string();

    // Create task repo and save task
    let task_repo: Arc<dyn TaskRepository> = Arc::new(MemoryTaskRepository::new());
    task_repo.create(task.clone()).await.unwrap();

    // Call discover_and_attach_task_branch (this simulates what happens in attempt_programmatic_merge)
    let mut task_mut = task.clone();
    let discovered = discover_and_attach_task_branch(&mut task_mut, &project, &task_repo)
        .await
        .unwrap();

    // Branch should have been discovered and attached
    assert!(discovered, "Branch should have been discovered");
    assert_eq!(task_mut.task_branch, Some(expected_branch.clone()));

    // Now verify that resolve_merge_branches returns the correct source branch
    let repo: Option<Arc<dyn PlanBranchRepository>> = None;
    let (source, target) = resolve_merge_branches(&task_mut, &project, &repo).await;

    assert_eq!(
        source, expected_branch,
        "Source branch should match the discovered branch"
    );
    assert_eq!(
        target, "main",
        "Target branch should be project base branch"
    );
}

// ==================
// base_branch_override tests
// ==================

#[tokio::test]
async fn resolve_merge_branches_merge_task_uses_override_as_target() {
    // When PlanBranch has base_branch_override set, merge-task path returns override as target
    let project = make_project(Some("main"));
    let mut task = make_task(None, None);
    task.id = TaskId::from_string("merge-task-override".to_string());

    let mem_repo = Arc::new(MemoryPlanBranchRepository::new());
    let mut pb = make_plan_branch(
        "art-override",
        "ralphx/test/plan-override",
        PlanBranchStatus::Active,
        Some("merge-task-override"),
    );
    pb.base_branch_override = Some("develop".to_string());
    mem_repo.create(pb).await.unwrap();

    let repo: Option<Arc<dyn PlanBranchRepository>> = Some(mem_repo);
    let (source, target) = resolve_merge_branches(&task, &project, &repo).await;
    assert_eq!(source, "ralphx/test/plan-override");
    // Override takes precedence over project base_branch ("main")
    assert_eq!(target, "develop");
}

#[tokio::test]
async fn resolve_effective_base_branch_uses_plan_source_branch_for_execution_freshness() {
    let project = make_project(Some("main"));
    let mut task = make_task_with_session(
        Some("art-source"),
        Some("ralphx/test/task-source"),
        Some("sess-1"),
    );
    task.execution_plan_id = Some(ExecutionPlanId::from_string("exec-source"));

    let mem_repo = Arc::new(MemoryPlanBranchRepository::new());
    let mut pb = make_plan_branch(
        "art-source",
        "ralphx/test/plan-source",
        PlanBranchStatus::Active,
        None,
    );
    pb.execution_plan_id = Some(ExecutionPlanId::from_string("exec-source"));
    pb.source_branch = "release/2026-04".to_string();
    pb.base_branch_override = Some("release/2026-04".to_string());
    mem_repo.create(pb).await.unwrap();

    let repo: Option<Arc<dyn PlanBranchRepository>> = Some(mem_repo);
    let result = resolve_effective_base_branch(&task, &project, &repo, None).await;
    assert_eq!(result, "release/2026-04");
}

#[tokio::test]
async fn resolve_effective_base_branch_uses_override_for_merge_task() {
    let project = make_project(Some("main"));
    let mut task = make_task(None, None);
    task.id = TaskId::from_string("merge-task-effective-base".to_string());
    task.execution_plan_id = Some(ExecutionPlanId::from_string("exec-effective-base"));

    let mem_repo = Arc::new(MemoryPlanBranchRepository::new());
    let mut pb = make_plan_branch(
        "art-effective-base",
        "ralphx/test/plan-effective-base",
        PlanBranchStatus::Active,
        Some("merge-task-effective-base"),
    );
    pb.execution_plan_id = Some(ExecutionPlanId::from_string("exec-effective-base"));
    pb.source_branch = "release/2026-04".to_string();
    pb.base_branch_override = Some("develop".to_string());
    mem_repo.create(pb).await.unwrap();

    let repo: Option<Arc<dyn PlanBranchRepository>> = Some(mem_repo);
    let result = resolve_effective_base_branch(&task, &project, &repo, Some("develop")).await;
    assert_eq!(result, "develop");
}

#[tokio::test]
async fn resolve_merge_branches_plan_task_returns_feature_branch_not_override() {
    // Plan-task path always merges into the feature branch, regardless of override
    let project = make_project(Some("main"));
    let mut task = make_task_with_session(
        Some("art-override"),
        Some("ralphx/test/task-plan"),
        Some("sess-1"), // must match the session_id used by make_plan_branch
    );
    task.id = TaskId::from_string("plan-task-override".to_string());

    let mem_repo = Arc::new(MemoryPlanBranchRepository::new());
    let mut pb = make_plan_branch(
        "art-override",
        "ralphx/test/plan-override",
        PlanBranchStatus::Active,
        None,
    );
    pb.base_branch_override = Some("develop".to_string());
    mem_repo.create(pb).await.unwrap();

    let repo: Option<Arc<dyn PlanBranchRepository>> = Some(mem_repo);
    let (source, target) = resolve_merge_branches(&task, &project, &repo).await;
    assert_eq!(source, "ralphx/test/task-plan");
    // Plan-task always merges into feature branch, NOT override
    assert_eq!(target, "ralphx/test/plan-override");
}

#[tokio::test]
async fn resolve_merge_branches_merge_task_fallback_when_no_override() {
    // When no base_branch_override, merge-task uses project base_branch
    let project = make_project(Some("staging"));
    let mut task = make_task(None, None);
    task.id = TaskId::from_string("merge-task-no-override".to_string());

    let mem_repo = Arc::new(MemoryPlanBranchRepository::new());
    let pb = make_plan_branch(
        "art-no-override",
        "ralphx/test/plan-no-override",
        PlanBranchStatus::Active,
        Some("merge-task-no-override"),
    );
    // base_branch_override is None (default from make_plan_branch)
    mem_repo.create(pb).await.unwrap();

    let repo: Option<Arc<dyn PlanBranchRepository>> = Some(mem_repo);
    let (source, target) = resolve_merge_branches(&task, &project, &repo).await;
    assert_eq!(source, "ralphx/test/plan-no-override");
    // Falls back to project base_branch
    assert_eq!(target, "staging");
}
