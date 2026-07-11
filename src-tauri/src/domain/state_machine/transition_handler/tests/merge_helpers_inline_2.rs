// Tests extracted from merge_helpers.rs #[cfg(test)] mod tests — part 2 of 2
//
// Covers: resolve_task_base_branch (merged plan branch re-creation),
//         pre-merge validation, deferred merge timeout

use super::super::merge_helpers::*;
use crate::domain::entities::{
    ArtifactId, IdeationSessionId, PlanBranch, PlanBranchStatus, Project, ProjectId, Task,
    TaskCategory, TaskId,
};
use crate::domain::repositories::PlanBranchRepository;
use crate::infrastructure::memory::MemoryPlanBranchRepository;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use tempfile::TempDir;

/// Create a temporary git repository for testing
fn create_temp_git_repo() -> (TempDir, PathBuf) {
    let temp_dir = TempDir::new().unwrap();
    let repo_path = temp_dir.path().to_path_buf();

    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(&repo_path)
        .output()
        .unwrap();

    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(&repo_path)
        .output()
        .unwrap();

    Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(&repo_path)
        .output()
        .unwrap();

    fs::write(repo_path.join("README.md"), "test").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(&repo_path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "Initial commit"])
        .current_dir(&repo_path)
        .output()
        .unwrap();

    (temp_dir, repo_path)
}

/// Create a test project with the given name and working directory
fn create_test_project(name: &str, working_directory: String) -> Project {
    Project::new(name.to_string(), working_directory)
}

/// Create a test task for a project
fn create_test_task(project_id: ProjectId) -> Task {
    Task::new(project_id, "Test task".to_string())
}

// ===== resolve_task_base_branch: Merged plan branch re-creation tests =====

/// Helper: create a PlanBranch with the given status and branch_name pointing to a git branch
/// in the temp repo.
fn make_plan_branch_for_resolve(
    session_id: &IdeationSessionId,
    project_id: &ProjectId,
    branch_name: &str,
    source_branch: &str,
    status: PlanBranchStatus,
) -> PlanBranch {
    let mut pb = PlanBranch::new(
        ArtifactId::from_string("artifact-test"),
        session_id.clone(),
        project_id.clone(),
        branch_name.to_string(),
        source_branch.to_string(),
    );
    pb.status = status;
    pb
}

#[tokio::test]
async fn test_resolve_task_base_branch_merged_branch_returns_fallback() {
    let (_temp_dir, repo_path) = create_temp_git_repo();
    let project = create_test_project("test-plan-branch", repo_path.to_string_lossy().to_string());

    let session_id = IdeationSessionId::from_string("session-merged-test");
    let mut task = create_test_task(project.id.clone());
    task.ideation_session_id = Some(session_id.clone());

    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let pb = make_plan_branch_for_resolve(
        &session_id,
        &project.id,
        "ralphx/test-plan-branch/plan-session-merged-test",
        "main",
        PlanBranchStatus::Merged,
    );
    plan_branch_repo.create(pb).await.unwrap();

    let plan_branch_repo_opt: Option<Arc<dyn PlanBranchRepository>> =
        Some(plan_branch_repo.clone() as Arc<dyn PlanBranchRepository>);

    let result = resolve_task_base_branch(
        &task,
        &project,
        &plan_branch_repo_opt,
        &None,
        &None,
        &None,
        &None,
    )
    .await;

    // Fix D: merged branches now fall back to project base (no resurrection)
    assert_eq!(
        result, "main",
        "Should fall back to project base when plan branch is Merged"
    );

    // Verify branch status is NOT changed (no resurrection)
    let updated = plan_branch_repo
        .get_by_session_id(&session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated.status,
        PlanBranchStatus::Merged,
        "DB status should remain Merged (no resurrection)"
    );
}

#[tokio::test]
async fn test_resolve_task_base_branch_merged_branch_exists_in_git_returns_fallback() {
    let (_temp_dir, repo_path) = create_temp_git_repo();
    let project = create_test_project("test-plan-branch", repo_path.to_string_lossy().to_string());

    let session_id = IdeationSessionId::from_string("session-merged-exists");
    let mut task = create_test_task(project.id.clone());
    task.ideation_session_id = Some(session_id.clone());

    let plan_branch_name = "ralphx/test-plan-branch/plan-session-merged-exists";

    Command::new("git")
        .args(["branch", plan_branch_name])
        .current_dir(&repo_path)
        .output()
        .unwrap();

    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let pb = make_plan_branch_for_resolve(
        &session_id,
        &project.id,
        plan_branch_name,
        "main",
        PlanBranchStatus::Merged,
    );
    plan_branch_repo.create(pb).await.unwrap();

    let plan_branch_repo_opt: Option<Arc<dyn PlanBranchRepository>> =
        Some(plan_branch_repo.clone() as Arc<dyn PlanBranchRepository>);

    let result = resolve_task_base_branch(
        &task,
        &project,
        &plan_branch_repo_opt,
        &None,
        &None,
        &None,
        &None,
    )
    .await;

    // Fix D: merged branches now fall back to project base (no resurrection)
    assert_eq!(
        result, "main",
        "Should fall back to project base when plan branch is Merged"
    );

    // Verify branch status is NOT changed (no resurrection)
    let updated = plan_branch_repo
        .get_by_session_id(&session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated.status,
        PlanBranchStatus::Merged,
        "DB status should remain Merged (no resurrection)"
    );
}

#[tokio::test]
async fn test_resolve_task_base_branch_active_status_unchanged() {
    let (_temp_dir, repo_path) = create_temp_git_repo();
    let project = create_test_project("test-plan-branch", repo_path.to_string_lossy().to_string());

    let session_id = IdeationSessionId::from_string("session-active-test");
    let mut task = create_test_task(project.id.clone());
    task.ideation_session_id = Some(session_id.clone());

    let plan_branch_name = "ralphx/test-plan-branch/plan-session-active-test";

    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let pb = make_plan_branch_for_resolve(
        &session_id,
        &project.id,
        plan_branch_name,
        "main",
        PlanBranchStatus::Active,
    );
    plan_branch_repo.create(pb).await.unwrap();

    let plan_branch_repo_opt: Option<Arc<dyn PlanBranchRepository>> =
        Some(plan_branch_repo.clone() as Arc<dyn PlanBranchRepository>);

    let result = resolve_task_base_branch(
        &task,
        &project,
        &plan_branch_repo_opt,
        &None,
        &None,
        &None,
        &None,
    )
    .await;

    assert_eq!(
        result, plan_branch_name,
        "Active arm should return plan branch name (creating lazily if needed)"
    );
}

#[tokio::test]
async fn test_resolve_task_base_branch_no_session_id_returns_default() {
    let (_temp_dir, repo_path) = create_temp_git_repo();
    let project = create_test_project("test", repo_path.to_string_lossy().to_string());
    let task = create_test_task(project.id.clone());

    let plan_branch_repo_opt: Option<Arc<dyn PlanBranchRepository>> = None;

    let result = resolve_task_base_branch(
        &task,
        &project,
        &plan_branch_repo_opt,
        &None,
        &None,
        &None,
        &None,
    )
    .await;

    assert_eq!(result, "main", "No session_id should fall back to default");
}

// ===== Pre-merge validation tests =====

/// Helper: create a plan_merge task (category = "plan_merge")
fn create_plan_merge_task(project_id: ProjectId) -> Task {
    let mut task = Task::new(project_id, "Merge plan to main".to_string());
    task.category = TaskCategory::PlanMerge;
    task
}

/// Helper: create a PlanBranch with a specific merge_task_id and status
fn make_plan_branch_with_merge_task(
    task_id: &TaskId,
    project_id: &ProjectId,
    branch_name: &str,
    status: PlanBranchStatus,
) -> PlanBranch {
    let session_id = IdeationSessionId::from_string("session-pre-merge-test");
    let mut pb = PlanBranch::new(
        ArtifactId::from_string("artifact-pre-merge-test"),
        session_id,
        project_id.clone(),
        branch_name.to_string(),
        "main".to_string(),
    );
    pb.status = status;
    pb.merge_task_id = Some(task_id.clone());
    pb
}

#[tokio::test]
async fn test_validate_plan_merge_passes_when_all_conditions_met() {
    let (_temp_dir, repo_path) = create_temp_git_repo();
    let project = create_test_project("test-pre-merge", repo_path.to_string_lossy().to_string());
    let task = create_plan_merge_task(project.id.clone());

    let feature_branch = "ralphx/test-pre-merge/plan-session-pre-merge-test";
    Command::new("git")
        .args(["branch", feature_branch])
        .current_dir(&repo_path)
        .output()
        .unwrap();

    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let pb = make_plan_branch_with_merge_task(
        &task.id,
        &project.id,
        feature_branch,
        PlanBranchStatus::Active,
    );
    plan_branch_repo.create(pb).await.unwrap();

    let plan_branch_repo_opt: Option<Arc<dyn PlanBranchRepository>> =
        Some(plan_branch_repo as Arc<dyn PlanBranchRepository>);

    let result = validate_plan_merge_preconditions(&task, &project, &plan_branch_repo_opt).await;

    assert!(
        result.is_ok(),
        "Validation should pass when all conditions are met"
    );
}

#[tokio::test]
async fn test_validate_plan_merge_passes_for_non_plan_merge_task() {
    let (_temp_dir, repo_path) = create_temp_git_repo();
    let project = create_test_project("test", repo_path.to_string_lossy().to_string());
    let task = create_test_task(project.id.clone());

    let plan_branch_repo_opt: Option<Arc<dyn PlanBranchRepository>> = None;

    let result = validate_plan_merge_preconditions(&task, &project, &plan_branch_repo_opt).await;

    assert!(
        result.is_ok(),
        "Non-plan_merge tasks should always pass validation"
    );
}

#[tokio::test]
async fn test_validate_plan_merge_fails_when_repo_not_wired() {
    let (_temp_dir, repo_path) = create_temp_git_repo();
    let project = create_test_project("test", repo_path.to_string_lossy().to_string());
    let task = create_plan_merge_task(project.id.clone());

    let plan_branch_repo_opt: Option<Arc<dyn PlanBranchRepository>> = None;

    let result = validate_plan_merge_preconditions(&task, &project, &plan_branch_repo_opt).await;

    assert_eq!(
        result,
        Err(PreMergeValidationError::PlanBranchRepoNotWired),
        "Should fail with PlanBranchRepoNotWired when repo is None"
    );
}

#[tokio::test]
async fn test_validate_plan_merge_fails_when_no_plan_branch_record() {
    let (_temp_dir, repo_path) = create_temp_git_repo();
    let project = create_test_project("test", repo_path.to_string_lossy().to_string());
    let task = create_plan_merge_task(project.id.clone());

    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let plan_branch_repo_opt: Option<Arc<dyn PlanBranchRepository>> =
        Some(plan_branch_repo as Arc<dyn PlanBranchRepository>);

    let result = validate_plan_merge_preconditions(&task, &project, &plan_branch_repo_opt).await;

    assert_eq!(
        result,
        Err(PreMergeValidationError::PlanBranchNotActive {
            status: "not_found".to_string()
        }),
        "Should fail with PlanBranchNotActive when no PlanBranch record exists"
    );
}

#[tokio::test]
async fn test_validate_plan_merge_fails_when_plan_branch_not_active() {
    let (_temp_dir, repo_path) = create_temp_git_repo();
    let project = create_test_project("test", repo_path.to_string_lossy().to_string());
    let task = create_plan_merge_task(project.id.clone());

    let feature_branch = "ralphx/test/plan-inactive";
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let pb = make_plan_branch_with_merge_task(
        &task.id,
        &project.id,
        feature_branch,
        PlanBranchStatus::Merged,
    );
    plan_branch_repo.create(pb).await.unwrap();

    let plan_branch_repo_opt: Option<Arc<dyn PlanBranchRepository>> =
        Some(plan_branch_repo as Arc<dyn PlanBranchRepository>);

    let result = validate_plan_merge_preconditions(&task, &project, &plan_branch_repo_opt).await;

    assert_eq!(
        result,
        Err(PreMergeValidationError::PlanBranchNotActive {
            status: "merged".to_string()
        }),
        "Should fail with PlanBranchNotActive when branch status is Merged"
    );
}

#[tokio::test]
async fn test_validate_plan_merge_fails_when_feature_branch_missing_in_git() {
    let (_temp_dir, repo_path) = create_temp_git_repo();
    let project = create_test_project("test", repo_path.to_string_lossy().to_string());
    let task = create_plan_merge_task(project.id.clone());

    let feature_branch = "ralphx/test/plan-deleted-branch";
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let pb = make_plan_branch_with_merge_task(
        &task.id,
        &project.id,
        feature_branch,
        PlanBranchStatus::Active,
    );
    plan_branch_repo.create(pb).await.unwrap();

    let plan_branch_repo_opt: Option<Arc<dyn PlanBranchRepository>> =
        Some(plan_branch_repo as Arc<dyn PlanBranchRepository>);

    let result = validate_plan_merge_preconditions(&task, &project, &plan_branch_repo_opt).await;

    assert_eq!(
        result,
        Err(PreMergeValidationError::FeatureBranchMissing {
            branch_name: feature_branch.to_string()
        }),
        "Should fail with FeatureBranchMissing when git branch does not exist"
    );
}

#[test]
fn test_pre_merge_error_message_not_empty() {
    let errors = vec![
        PreMergeValidationError::PlanBranchRepoNotWired,
        PreMergeValidationError::PlanBranchNotActive {
            status: "merged".to_string(),
        },
        PreMergeValidationError::FeatureBranchMissing {
            branch_name: "feature/foo".to_string(),
        },
    ];
    for err in &errors {
        assert!(
            !err.message().is_empty(),
            "Error message should not be empty: {err:?}"
        );
        assert!(
            !err.error_code().is_empty(),
            "Error code should not be empty: {err:?}"
        );
    }
}

#[test]
fn test_pre_merge_error_message_contains_actionable_info() {
    let status_err = PreMergeValidationError::PlanBranchNotActive {
        status: "abandoned".to_string(),
    };
    assert!(
        status_err.message().contains("abandoned"),
        "Error message should include the actual status"
    );

    let branch_err = PreMergeValidationError::FeatureBranchMissing {
        branch_name: "ralphx/my-project/plan-abc".to_string(),
    };
    assert!(
        branch_err.message().contains("ralphx/my-project/plan-abc"),
        "Error message should include the missing branch name"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Deferred Merge Timeout Tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_is_merge_deferred_timed_out_returns_false_when_no_metadata() {
    let project = Project::new("test".to_string(), "/tmp".to_string());
    let task = Task::new(project.id, "Test task".to_string());
    assert!(!is_merge_deferred_timed_out(&task));
}

#[test]
fn test_is_merge_deferred_timed_out_returns_false_when_timestamp_missing() {
    let project = Project::new("test".to_string(), "/tmp".to_string());
    let mut task = Task::new(project.id, "Test task".to_string());
    task.metadata = Some(r#"{"merge_deferred": true}"#.to_string());
    assert!(!is_merge_deferred_timed_out(&task));
}

#[test]
fn test_is_merge_deferred_timed_out_returns_false_when_timestamp_invalid() {
    let project = Project::new("test".to_string(), "/tmp".to_string());
    let mut task = Task::new(project.id, "Test task".to_string());
    task.metadata =
        Some(r#"{"merge_deferred": true, "merge_deferred_at": "not-a-timestamp"}"#.to_string());
    assert!(!is_merge_deferred_timed_out(&task));
}

#[test]
fn test_is_merge_deferred_timed_out_returns_false_when_recent() {
    let project = Project::new("test".to_string(), "/tmp".to_string());
    let mut task = Task::new(project.id, "Test task".to_string());
    let recent = (chrono::Utc::now() - chrono::Duration::seconds(10)).to_rfc3339();
    task.metadata = Some(format!(
        r#"{{"merge_deferred": true, "merge_deferred_at": "{}"}}"#,
        recent
    ));
    assert!(!is_merge_deferred_timed_out(&task));
}

#[test]
fn test_is_merge_deferred_timed_out_returns_true_when_expired() {
    let project = Project::new("test".to_string(), "/tmp".to_string());
    let mut task = Task::new(project.id, "Test task".to_string());
    let old = (chrono::Utc::now() - chrono::Duration::seconds(300)).to_rfc3339();
    task.metadata = Some(format!(
        r#"{{"merge_deferred": true, "merge_deferred_at": "{}"}}"#,
        old
    ));
    assert!(is_merge_deferred_timed_out(&task));
}

#[test]
fn test_is_main_merge_deferred_timed_out_returns_false_when_no_metadata() {
    let project = Project::new("test".to_string(), "/tmp".to_string());
    let task = Task::new(project.id, "Test task".to_string());
    assert!(!is_main_merge_deferred_timed_out(&task));
}

#[test]
fn test_is_main_merge_deferred_timed_out_returns_false_when_timestamp_missing() {
    let project = Project::new("test".to_string(), "/tmp".to_string());
    let mut task = Task::new(project.id, "Test task".to_string());
    task.metadata = Some(r#"{"main_merge_deferred": true}"#.to_string());
    assert!(!is_main_merge_deferred_timed_out(&task));
}

#[test]
fn test_is_main_merge_deferred_timed_out_returns_false_when_recent() {
    let project = Project::new("test".to_string(), "/tmp".to_string());
    let mut task = Task::new(project.id, "Test task".to_string());
    let recent = (chrono::Utc::now() - chrono::Duration::seconds(5)).to_rfc3339();
    task.metadata = Some(format!(
        r#"{{"main_merge_deferred": true, "main_merge_deferred_at": "{}"}}"#,
        recent
    ));
    assert!(!is_main_merge_deferred_timed_out(&task));
}

#[test]
fn test_is_main_merge_deferred_timed_out_returns_true_when_expired() {
    let project = Project::new("test".to_string(), "/tmp".to_string());
    let mut task = Task::new(project.id, "Test task".to_string());
    let old = (chrono::Utc::now() - chrono::Duration::seconds(200)).to_rfc3339();
    task.metadata = Some(format!(
        r#"{{"main_merge_deferred": true, "main_merge_deferred_at": "{}"}}"#,
        old
    ));
    assert!(is_main_merge_deferred_timed_out(&task));
}

#[test]
fn test_deferred_merge_timeout_constant_is_positive() {
    assert!(
        DEFERRED_MERGE_TIMEOUT_SECONDS > 0,
        "DEFERRED_MERGE_TIMEOUT_SECONDS must be a positive number"
    );
}

#[test]
fn test_is_merge_deferred_timed_out_boundary_just_before_timeout() {
    let project = Project::new("test".to_string(), "/tmp".to_string());
    let mut task = Task::new(project.id, "Test task".to_string());
    let just_before = (chrono::Utc::now()
        - chrono::Duration::seconds(DEFERRED_MERGE_TIMEOUT_SECONDS - 1))
    .to_rfc3339();
    task.metadata = Some(format!(
        r#"{{"merge_deferred": true, "merge_deferred_at": "{}"}}"#,
        just_before
    ));
    assert!(
        !is_merge_deferred_timed_out(&task),
        "Task just before timeout should not be considered timed out"
    );
}

#[test]
fn test_is_merge_deferred_timed_out_boundary_at_timeout() {
    let project = Project::new("test".to_string(), "/tmp".to_string());
    let mut task = Task::new(project.id, "Test task".to_string());
    let at_timeout = (chrono::Utc::now()
        - chrono::Duration::seconds(DEFERRED_MERGE_TIMEOUT_SECONDS))
    .to_rfc3339();
    task.metadata = Some(format!(
        r#"{{"merge_deferred": true, "merge_deferred_at": "{}"}}"#,
        at_timeout
    ));
    assert!(
        is_merge_deferred_timed_out(&task),
        "Task exactly at timeout boundary should be considered timed out"
    );
}
