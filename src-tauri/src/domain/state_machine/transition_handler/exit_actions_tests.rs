use super::*;
use crate::application::AppState;
use crate::domain::entities::{
    ExecutionFailureSource, ExecutionRecoveryEvent, ExecutionRecoveryEventKind,
    ExecutionRecoveryMetadata, ExecutionRecoveryReasonCode, ExecutionRecoverySource,
    ExecutionRecoveryState, Project, Task,
};
use std::process::Command;
use tempfile::TempDir;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_exit_ctx(app_state: &AppState, task_id: &str, project_id: &str) -> ExitContext {
    ExitContext {
        task_id: task_id.to_string(),
        project_id: project_id.to_string(),
        task_repo: Some(std::sync::Arc::clone(&app_state.task_repo)),
        project_repo: Some(std::sync::Arc::clone(&app_state.project_repo)),
        task_scheduler: None,
    }
}

fn make_transient_recovery(source: ExecutionFailureSource) -> ExecutionRecoveryMetadata {
    let mut recovery = ExecutionRecoveryMetadata::new();
    recovery.append_event_with_state(
        ExecutionRecoveryEvent::new(
            ExecutionRecoveryEventKind::Failed,
            ExecutionRecoverySource::System,
            ExecutionRecoveryReasonCode::Timeout,
            "transient failure",
        )
        .with_failure_source(source),
        ExecutionRecoveryState::Retrying,
    );
    recovery
}

fn make_retrying_recovery_with_auto_retry() -> ExecutionRecoveryMetadata {
    // Recovery in Retrying state with last event = AutoRetryTriggered (not Failed)
    // This simulates a task that was auto-retried and is now completing successfully.
    let mut recovery = ExecutionRecoveryMetadata::new();
    recovery.append_event_with_state(
        ExecutionRecoveryEvent::new(
            ExecutionRecoveryEventKind::AutoRetryTriggered,
            ExecutionRecoverySource::Auto,
            ExecutionRecoveryReasonCode::Timeout,
            "auto retry triggered",
        )
        .with_attempt(1),
        ExecutionRecoveryState::Retrying,
    );
    recovery
}

fn setup_git_repo() -> TempDir {
    let dir = TempDir::new().expect("create temporary git repo");
    for args in [
        vec!["init", "-b", "main"],
        vec!["config", "user.email", "test@example.com"],
        vec!["config", "user.name", "Test User"],
    ] {
        let status = Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .status()
            .expect("run git setup command");
        assert!(status.success());
    }
    std::fs::write(dir.path().join("tracked.txt"), "initial\n").expect("write tracked file");
    let status = Command::new("git")
        .args(["add", "tracked.txt"])
        .current_dir(dir.path())
        .status()
        .expect("stage tracked file");
    assert!(status.success());
    let status = Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(dir.path())
        .status()
        .expect("commit initial file");
    assert!(status.success());
    dir
}

fn git_output(repo: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("run git command");
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .expect("git output is utf8")
        .trim()
        .to_string()
}

#[tokio::test]
async fn pr_mode_execution_exit_does_not_commit_primary_checkout() {
    let repo = setup_git_repo();
    let app_state = AppState::new_test();
    let mut project = Project::new(
        "Protected Project".into(),
        repo.path().to_string_lossy().into_owned(),
    );
    project.github_pr_enabled = true;
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let recovery = make_retrying_recovery_with_auto_retry();
    let mut task = Task::new(project.id.clone(), "Protected task".into());
    task.metadata = Some(recovery.update_task_metadata(None).unwrap());
    task.worktree_path = None;
    app_state.task_repo.create(task.clone()).await.unwrap();

    std::fs::write(repo.path().join("tracked.txt"), "user change\n")
        .expect("dirty primary checkout");
    let head_before = git_output(repo.path(), &["rev-parse", "HEAD"]);
    let status_before = git_output(repo.path(), &["status", "--porcelain"]);

    let ctx = make_exit_ctx(&app_state, task.id.as_str(), project.id.as_str());
    auto_commit_on_execution_done(&ctx).await;

    assert_eq!(git_output(repo.path(), &["rev-parse", "HEAD"]), head_before);
    assert_eq!(
        git_output(repo.path(), &["status", "--porcelain"]),
        status_before,
        "PR mode must preserve primary-checkout changes exactly"
    );
    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    let metadata = ExecutionRecoveryMetadata::from_task_metadata(updated.metadata.as_deref())
        .unwrap()
        .unwrap();
    assert_eq!(metadata.last_state, ExecutionRecoveryState::Succeeded);
}

#[tokio::test]
async fn pr_mode_execution_exit_commits_dirty_isolated_worktree() {
    let primary_checkout = TempDir::new().expect("create primary checkout directory");
    let worktree = setup_git_repo();
    let app_state = AppState::new_test();
    let mut project = Project::new(
        "Protected Project".into(),
        primary_checkout.path().to_string_lossy().into_owned(),
    );
    project.github_pr_enabled = true;
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let recovery = make_retrying_recovery_with_auto_retry();
    let mut task = Task::new(project.id.clone(), "Isolated worktree task".into());
    task.metadata = Some(recovery.update_task_metadata(None).unwrap());
    task.worktree_path = Some(worktree.path().to_string_lossy().into_owned());
    app_state.task_repo.create(task.clone()).await.unwrap();

    std::fs::write(worktree.path().join("tracked.txt"), "task change\n")
        .expect("dirty isolated worktree");
    let head_before = git_output(worktree.path(), &["rev-parse", "HEAD"]);

    let ctx = make_exit_ctx(&app_state, task.id.as_str(), project.id.as_str());
    auto_commit_on_execution_done(&ctx).await;

    let head_after = git_output(worktree.path(), &["rev-parse", "HEAD"]);
    assert_ne!(
        head_after, head_before,
        "PR mode should still preserve task work by committing an isolated worktree"
    );
    assert_eq!(
        git_output(worktree.path(), &["status", "--porcelain"]),
        "",
        "the isolated worktree should be clean after the safety commit"
    );
    assert_eq!(
        git_output(worktree.path(), &["log", "-1", "--pretty=%s"]),
        "feat: Isolated worktree task"
    );
}

// ── GAP B4: Auto-commit skipped for transient failures ───────────────────────

/// Transient timeout: auto_commit_on_execution_done returns early, H11 NOT applied.
#[tokio::test]
async fn auto_commit_skipped_for_transient_timeout() {
    let app_state = AppState::new_test();
    let project = Project::new("Test Project".into(), "/tmp".into());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let recovery = make_transient_recovery(ExecutionFailureSource::TransientTimeout);
    let mut task = Task::new(project.id.clone(), "Timing out task".into());
    task.metadata = Some(recovery.update_task_metadata(None).unwrap());
    app_state.task_repo.create(task.clone()).await.unwrap();

    let ctx = make_exit_ctx(&app_state, task.id.as_str(), project.id.as_str());
    auto_commit_on_execution_done(&ctx).await;

    // H11 must NOT fire: last_state stays Retrying (not updated to Succeeded)
    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    let meta = ExecutionRecoveryMetadata::from_task_metadata(updated.metadata.as_deref())
        .unwrap()
        .unwrap();
    assert_eq!(
        meta.last_state,
        ExecutionRecoveryState::Retrying,
        "transient failure: H11 must not update last_state to Succeeded"
    );
}

/// Transient parse stall: auto_commit_on_execution_done returns early.
#[tokio::test]
async fn auto_commit_skipped_for_parse_stall() {
    let app_state = AppState::new_test();
    let project = Project::new("Test Project".into(), "/tmp".into());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let recovery = make_transient_recovery(ExecutionFailureSource::ParseStall);
    let mut task = Task::new(project.id.clone(), "Stalling task".into());
    task.metadata = Some(recovery.update_task_metadata(None).unwrap());
    app_state.task_repo.create(task.clone()).await.unwrap();

    let ctx = make_exit_ctx(&app_state, task.id.as_str(), project.id.as_str());
    auto_commit_on_execution_done(&ctx).await;

    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    let meta = ExecutionRecoveryMetadata::from_task_metadata(updated.metadata.as_deref())
        .unwrap()
        .unwrap();
    assert_eq!(
        meta.last_state,
        ExecutionRecoveryState::Retrying,
        "parse stall: H11 must not update last_state"
    );
}

/// Transient agent crash: auto_commit_on_execution_done returns early.
#[tokio::test]
async fn auto_commit_skipped_for_agent_crash() {
    let app_state = AppState::new_test();
    let project = Project::new("Test Project".into(), "/tmp".into());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let recovery = make_transient_recovery(ExecutionFailureSource::AgentCrash);
    let mut task = Task::new(project.id.clone(), "Crashing task".into());
    task.metadata = Some(recovery.update_task_metadata(None).unwrap());
    app_state.task_repo.create(task.clone()).await.unwrap();

    let ctx = make_exit_ctx(&app_state, task.id.as_str(), project.id.as_str());
    auto_commit_on_execution_done(&ctx).await;

    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    let meta = ExecutionRecoveryMetadata::from_task_metadata(updated.metadata.as_deref())
        .unwrap()
        .unwrap();
    assert_eq!(
        meta.last_state,
        ExecutionRecoveryState::Retrying,
        "agent crash: H11 must not update last_state"
    );
}

/// Non-transient failure (Unknown source): guard does NOT skip, H11 fires.
/// The last event is a Failed event → H11 guard prevents update to Succeeded.
#[tokio::test]
async fn auto_commit_not_skipped_for_non_transient_failure() {
    let app_state = AppState::new_test();
    let project = Project::new("Test Project".into(), "/tmp".into());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Unknown failure source → not transient → guard does not fire
    // But last event IS a Failed event → H11 guard prevents update to Succeeded
    let recovery = make_transient_recovery(ExecutionFailureSource::Unknown);
    let mut task = Task::new(project.id.clone(), "Unknown failure task".into());
    task.metadata = Some(recovery.update_task_metadata(None).unwrap());
    app_state.task_repo.create(task.clone()).await.unwrap();

    let ctx = make_exit_ctx(&app_state, task.id.as_str(), project.id.as_str());
    // Should not panic and should not update to Succeeded (last event is Failed kind)
    auto_commit_on_execution_done(&ctx).await;

    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    let meta = ExecutionRecoveryMetadata::from_task_metadata(updated.metadata.as_deref())
        .unwrap()
        .unwrap();
    // Last event is Failed → H11 guard prevents update
    assert_ne!(
        meta.last_state,
        ExecutionRecoveryState::Succeeded,
        "failed-kind last event: H11 must not update to Succeeded"
    );
}

// ── GAP H11: Success state updated to Succeeded ──────────────────────────────

/// When task has recovery metadata in Retrying state and last event is NOT Failed,
/// H11 updates last_state to Succeeded after successful execution.
#[tokio::test]
async fn success_state_updated_to_succeeded_after_successful_retry() {
    let app_state = AppState::new_test();
    let project = Project::new("Test Project".into(), "/tmp".into());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Recovery in Retrying state, last event = AutoRetryTriggered (not Failed)
    let recovery = make_retrying_recovery_with_auto_retry();
    let mut task = Task::new(project.id.clone(), "Successful retry task".into());
    task.metadata = Some(recovery.update_task_metadata(None).unwrap());
    app_state.task_repo.create(task.clone()).await.unwrap();

    let ctx = make_exit_ctx(&app_state, task.id.as_str(), project.id.as_str());
    auto_commit_on_execution_done(&ctx).await;

    // H11: last_state must now be Succeeded
    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    let meta = ExecutionRecoveryMetadata::from_task_metadata(updated.metadata.as_deref())
        .unwrap()
        .unwrap();
    assert_eq!(
        meta.last_state,
        ExecutionRecoveryState::Succeeded,
        "H11: last_state must be Succeeded after successful execution"
    );
}

/// H11 backward compat: no recovery metadata → function completes without error,
/// no metadata is added.
#[tokio::test]
async fn success_state_no_op_when_no_metadata() {
    let app_state = AppState::new_test();
    let project = Project::new("Test Project".into(), "/tmp".into());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Task has no metadata at all
    let task = Task::new(project.id.clone(), "Task without recovery metadata".into());
    app_state.task_repo.create(task.clone()).await.unwrap();

    let ctx = make_exit_ctx(&app_state, task.id.as_str(), project.id.as_str());
    // Must not panic
    auto_commit_on_execution_done(&ctx).await;

    // No execution_recovery metadata should have been added
    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    let meta = ExecutionRecoveryMetadata::from_task_metadata(updated.metadata.as_deref()).unwrap();
    assert!(
        meta.is_none(),
        "H11 backward compat: no recovery metadata should be added when none existed"
    );
}

/// H11: already Succeeded → no redundant update.
#[tokio::test]
async fn success_state_no_op_when_already_succeeded() {
    let app_state = AppState::new_test();
    let project = Project::new("Test Project".into(), "/tmp".into());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut recovery = make_retrying_recovery_with_auto_retry();
    recovery.last_state = ExecutionRecoveryState::Succeeded;
    let mut task = Task::new(project.id.clone(), "Already succeeded task".into());
    task.metadata = Some(recovery.update_task_metadata(None).unwrap());
    app_state.task_repo.create(task.clone()).await.unwrap();

    let ctx = make_exit_ctx(&app_state, task.id.as_str(), project.id.as_str());
    auto_commit_on_execution_done(&ctx).await;

    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    let meta = ExecutionRecoveryMetadata::from_task_metadata(updated.metadata.as_deref())
        .unwrap()
        .unwrap();
    assert_eq!(
        meta.last_state,
        ExecutionRecoveryState::Succeeded,
        "already Succeeded: state should remain unchanged"
    );
}
