// Integration tests for merge pipeline reliability round 2 fixes.
//
// Tests:
//   1. Config defaults for new timeout fields (pre_merge_cleanup_timeout_secs, step_0b_kill_timeout_secs)
//   2. State freshness guard: complete_merge_internal aborts on stale task state (ghost merge prevention)
//   3. State freshness guard: complete_merge_internal proceeds when task is in PendingMerge
//   4. State freshness guard: complete_merge_internal proceeds when task is in Merging

use super::helpers::*;
use crate::domain::entities::{InternalStatus, ProjectId, Task};
use crate::domain::state_machine::transition_handler::complete_merge_internal;
use crate::infrastructure::agents::claude::{GitRuntimeConfig, ReconciliationConfig};

// ──────────────────────────────────────────────────────────────────────────────
// Config default tests (Fix #1 + Fix #4)
// ──────────────────────────────────────────────────────────────────────────────

/// ReconciliationConfig::default() includes pre_merge_cleanup_timeout_secs = 60.
#[test]
fn test_pre_merge_cleanup_timeout_default() {
    let cfg = ReconciliationConfig::default();
    assert_eq!(
        cfg.pre_merge_cleanup_timeout_secs, 60,
        "Default pre_merge_cleanup timeout should be 60 seconds"
    );
}

/// GitRuntimeConfig::default() includes step_0b_kill_timeout_secs = 5 (merge speed overhaul).
#[test]
fn test_step_0b_kill_timeout_default() {
    let cfg = GitRuntimeConfig::default();
    assert_eq!(
        cfg.step_0b_kill_timeout_secs, 5,
        "Default step 0b kill timeout should be 5 seconds"
    );
}

/// YAML deserialization requires pre_merge_cleanup_timeout_secs.
#[test]
fn test_yaml_requires_pre_merge_cleanup_timeout() {
    // YAML with all ReconciliationConfig fields EXCEPT pre_merge_cleanup_timeout_secs
    let yaml = r#"
merger_timeout_secs: 1200
merging_max_retries: 3
pending_merge_stale_minutes: 2
qa_stale_minutes: 5
merge_incomplete_retry_base_secs: 30
merge_incomplete_retry_max_secs: 1800
merge_incomplete_max_retries: 5
validation_revert_max_count: 2
merge_conflict_retry_base_secs: 60
merge_conflict_retry_max_secs: 600
merge_conflict_max_retries: 3
executing_max_retries: 5
reviewing_max_retries: 3
qa_max_retries: 3
executing_max_wall_clock_minutes: 60
reviewing_max_wall_clock_minutes: 30
qa_max_wall_clock_minutes: 15
attempt_merge_deadline_secs: 120
validation_deadline_secs: 1200
merge_registry_grace_period_secs: 60
validation_retry_min_cooldown_secs: 120
validation_failure_circuit_breaker_count: 3
merge_starvation_guard_secs: 60
branch_freshness_timeout_secs: 60
merge_watcher_grace_secs: 30
merge_watcher_poll_secs: 15
"#;
    let result: Result<ReconciliationConfig, _> = serde_yaml::from_str(yaml);
    assert!(
        result.is_err(),
        "Missing pre_merge_cleanup_timeout_secs should fail YAML deserialization"
    );
}

/// YAML deserialization requires step_0b_kill_timeout_secs in GitRuntimeConfig.
#[test]
fn test_yaml_requires_step_0b_kill_timeout() {
    // YAML with all GitRuntimeConfig fields EXCEPT step_0b_kill_timeout_secs
    let yaml = r#"
cmd_timeout_secs: 60
max_retries: 3
retry_backoff_secs: [1, 2, 4]
index_lock_stale_secs: 5
workspace_freshness_cache_ttl_ms: 2000
workspace_review_cache_ttl_ms: 2000
workspace_pr_description_cache_ttl_ms: 300000
workspace_pr_annotations_cache_ttl_ms: 30000
workspace_pr_annotations_check_run_fetch_limit: 10
orphan_worktree_cleanup_marker_retry_secs: 86400
agent_kill_settle_secs: 1
agent_stop_timeout_secs: 10
cleanup_worktree_timeout_secs: 10
cleanup_git_op_timeout_secs: 30
worktree_lsof_timeout_secs: 10
"#;
    let result: Result<GitRuntimeConfig, _> = serde_yaml::from_str(yaml);
    assert!(
        result.is_err(),
        "Missing step_0b_kill_timeout_secs should fail YAML deserialization"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Fix #2: State freshness guard — ghost merge prevention
// ──────────────────────────────────────────────────────────────────────────────

/// When the reconciler has already moved a task to MergeIncomplete while
/// `attempt_programmatic_merge` was running, `complete_merge_internal` must
/// NOT overwrite the status to Merged. It should return Ok(()) and leave
/// the task in its current state.
///
/// This is the core "ghost merge" race condition guard.
#[tokio::test]
async fn test_merge_completion_aborts_on_stale_task_state() {
    let git_repo = setup_real_git_repo();
    let repo_path = git_repo.path();

    // Merge the task branch into main to get a real commit SHA
    let merge_output = std::process::Command::new("git")
        .args([
            "merge",
            "--no-ff",
            &git_repo.task_branch,
            "-m",
            "merge feature",
        ])
        .current_dir(repo_path)
        .output()
        .expect("git merge");
    assert!(
        merge_output.status.success(),
        "git merge should succeed: {}",
        String::from_utf8_lossy(&merge_output.stderr)
    );

    // Get the merge commit SHA
    let rev_output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_path)
        .output()
        .expect("git rev-parse");
    let commit_sha = String::from_utf8_lossy(&rev_output.stdout)
        .trim()
        .to_string();

    // Set up task and project in memory repos
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "Ghost merge test".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some(git_repo.task_branch.clone());
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    // Simulate reconciler race: change task status to MergeIncomplete in the DB
    // (this is what the reconciler does when it detects staleness)
    let mut stale_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    stale_task.internal_status = InternalStatus::MergeIncomplete;
    task_repo.update(&stale_task).await.unwrap();

    // Create project pointing to the real git repo
    let project = make_real_git_project(&git_repo.path_string());

    // Now call complete_merge_internal — it should detect the stale state and abort
    let mut task_for_merge = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    // Pretend the pipeline still thinks it's PendingMerge (the mutable ref was from before the race)
    task_for_merge.internal_status = InternalStatus::PendingMerge;

    let task_repo_dyn: Arc<dyn TaskRepository> = Arc::clone(&task_repo) as Arc<dyn TaskRepository>;
    let result = complete_merge_internal(
        &mut task_for_merge,
        &project,
        &commit_sha,
        "",
        "main",
        &task_repo_dyn,
        None,
        None,
        None,
        None,
    )
    .await;

    // Should return Ok(()) — not an error
    assert!(
        result.is_ok(),
        "complete_merge_internal should return Ok on stale state, got: {:?}",
        result
    );

    // The task in DB should still be MergeIncomplete — NOT overwritten to Merged
    let final_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        final_task.internal_status,
        InternalStatus::MergeIncomplete,
        "Task status should remain MergeIncomplete (reconciler's transition), not Merged. Got: {:?}",
        final_task.internal_status,
    );
}

/// complete_merge_internal proceeds normally when task is in PendingMerge state
/// (the expected happy path — no concurrent transition).
#[tokio::test]
async fn test_merge_completion_proceeds_on_pending_merge() {
    let git_repo = setup_real_git_repo();
    let repo_path = git_repo.path();

    // Merge the task branch into main
    let merge_output = std::process::Command::new("git")
        .args([
            "merge",
            "--no-ff",
            &git_repo.task_branch,
            "-m",
            "merge feature",
        ])
        .current_dir(repo_path)
        .output()
        .expect("git merge");
    assert!(merge_output.status.success());

    let rev_output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_path)
        .output()
        .expect("git rev-parse");
    let commit_sha = String::from_utf8_lossy(&rev_output.stdout)
        .trim()
        .to_string();

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "Happy path merge test".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some(git_repo.task_branch.clone());
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut project = make_real_git_project(&git_repo.path_string());
    project.id = project_id;
    project.base_branch = Some("main".to_string());

    let mut task_for_merge = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    let task_repo_dyn: Arc<dyn TaskRepository> = Arc::clone(&task_repo) as Arc<dyn TaskRepository>;
    let result = complete_merge_internal(
        &mut task_for_merge,
        &project,
        &commit_sha,
        "",
        "main",
        &task_repo_dyn,
        None,
        None,
        None,
        None,
    )
    .await;

    assert!(
        result.is_ok(),
        "complete_merge_internal should succeed: {:?}",
        result
    );

    let final_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        final_task.internal_status,
        InternalStatus::Merged,
        "Task should be Merged after successful complete_merge_internal"
    );
    assert!(
        final_task.merge_commit_sha.is_some(),
        "merge_commit_sha should be set"
    );
}

/// complete_merge_internal proceeds when task is in Merging state
/// (agent-assisted merge path — also a valid state).
#[tokio::test]
async fn test_merge_completion_proceeds_on_merging() {
    let git_repo = setup_real_git_repo();
    let repo_path = git_repo.path();

    let merge_output = std::process::Command::new("git")
        .args([
            "merge",
            "--no-ff",
            &git_repo.task_branch,
            "-m",
            "merge feature",
        ])
        .current_dir(repo_path)
        .output()
        .expect("git merge");
    assert!(merge_output.status.success());

    let rev_output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_path)
        .output()
        .expect("git rev-parse");
    let commit_sha = String::from_utf8_lossy(&rev_output.stdout)
        .trim()
        .to_string();

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "Merging state test".to_string());
    task.internal_status = InternalStatus::Merging;
    task.task_branch = Some(git_repo.task_branch.clone());
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut project = make_real_git_project(&git_repo.path_string());
    project.id = project_id;
    project.base_branch = Some("main".to_string());

    let mut task_for_merge = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    let task_repo_dyn: Arc<dyn TaskRepository> = Arc::clone(&task_repo) as Arc<dyn TaskRepository>;
    let result = complete_merge_internal(
        &mut task_for_merge,
        &project,
        &commit_sha,
        "",
        "main",
        &task_repo_dyn,
        None,
        None,
        None,
        None,
    )
    .await;

    assert!(
        result.is_ok(),
        "complete_merge_internal should succeed from Merging state: {:?}",
        result
    );

    let final_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        final_task.internal_status,
        InternalStatus::Merged,
        "Task should be Merged from Merging state"
    );
}
