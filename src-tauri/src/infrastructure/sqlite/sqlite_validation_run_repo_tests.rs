use super::*;
use crate::domain::entities::{
    Project, Task, ValidationCacheDecision, ValidationCommandCategory, ValidationCommandResult,
    ValidationCommandSource, ValidationCommandStatus, ValidationContextType, ValidationPurpose,
    ValidationRun, ValidationRunMode, ValidationRunStatus,
};
use crate::domain::repositories::{ProjectRepository, TaskRepository, ValidationRunRepository};
use crate::testing::SqliteTestDb;
use chrono::{TimeZone, Utc};

async fn setup_repo() -> (SqliteTestDb, SqliteValidationRunRepository, Task) {
    let db = SqliteTestDb::new("sqlite-validation-run-repo");
    let project_repo = SqliteProjectRepository::new(db.new_connection());
    let task_repo = SqliteTaskRepository::new(db.new_connection());
    let repo = SqliteValidationRunRepository::from_shared(db.shared_conn());
    let project = project_repo
        .create(Project::new(
            "Validation Repo".to_string(),
            "/tmp/validation-repo".to_string(),
        ))
        .await
        .expect("project should be created");
    let task = task_repo
        .create(Task::new(project.id, "Persist validation".to_string()))
        .await
        .expect("task should be created");
    (db, repo, task)
}

fn validation_run(task: &Task) -> ValidationRun {
    ValidationRun {
        id: "validation-run-1".to_string(),
        task_id: task.id.clone(),
        project_id: task.project_id.clone(),
        purpose: ValidationPurpose::ReExecution,
        context_type: ValidationContextType::ReExecution,
        requested_by_agent: Some("ralphx-execution-worker".to_string()),
        status: ValidationRunStatus::Running,
        mode: ValidationRunMode::ReuseOrRun,
        policy_enabled: true,
        head_sha: Some("abcdef1234567890".to_string()),
        start_content_fingerprint: Some("tree-start".to_string()),
        validated_content_fingerprint: Some("tree-validated".to_string()),
        promoted_commit_sha: None,
        base_ref: Some("main".to_string()),
        analysis_fingerprint: Some("analysis-a".to_string()),
        status_episode_entered_at: Some(Utc.with_ymd_and_hms(2026, 7, 3, 12, 0, 0).unwrap()),
        started_at: Utc.with_ymd_and_hms(2026, 7, 3, 12, 1, 0).unwrap(),
        completed_at: None,
    }
}

fn command_result(
    task: &Task,
    id: &str,
    created_at: chrono::DateTime<Utc>,
    status: ValidationCommandStatus,
) -> ValidationCommandResult {
    ValidationCommandResult {
        id: id.to_string(),
        validation_run_id: "validation-run-1".to_string(),
        task_id: task.id.clone(),
        project_id: task.project_id.clone(),
        command_source: ValidationCommandSource::ProjectAnalysisRef,
        command_ref: Some("unit-tests".to_string()),
        command: "cargo test validation".to_string(),
        cwd: "/tmp/validation-repo".to_string(),
        label: Some("Validation tests".to_string()),
        category: ValidationCommandCategory::Test,
        reason: Some("PR validation".to_string()),
        related_files: vec!["src/lib.rs".to_string(), "src/main.rs".to_string()],
        cache_key: format!("cache-{id}"),
        cache_decision: ValidationCacheDecision::Ran,
        status,
        exit_code: Some(0),
        duration_ms: Some(125),
        stdout_snippet: Some("ok".to_string()),
        stderr_snippet: Some("warning".to_string()),
        stdout_log_path: Some("/tmp/stdout.log".to_string()),
        stderr_log_path: Some("/tmp/stderr.log".to_string()),
        launcher_kind: Some("production_shell_resolver".to_string()),
        resolved_shell_path: Some("/bin/zsh".to_string()),
        head_sha: Some("abcdef1234567890".to_string()),
        analysis_fingerprint: Some("analysis-a".to_string()),
        status_episode_entered_at: Some(Utc.with_ymd_and_hms(2026, 7, 3, 12, 0, 0).unwrap()),
        created_at,
    }
}

#[tokio::test]
async fn validation_run_repo_roundtrips_runs_and_orders_command_results() {
    let (_db, repo, task) = setup_repo().await;
    let run = validation_run(&task);
    repo.create_run(&run).await.expect("run should persist");
    let completed_at = Utc.with_ymd_and_hms(2026, 7, 3, 12, 2, 0).unwrap();
    repo.update_run_status(&run.id, ValidationRunStatus::Passed, Some(completed_at))
        .await
        .expect("run status should update");

    let later = command_result(
        &task,
        "command-later",
        Utc.with_ymd_and_hms(2026, 7, 3, 12, 1, 20).unwrap(),
        ValidationCommandStatus::Passed,
    );
    let earlier = command_result(
        &task,
        "command-earlier",
        Utc.with_ymd_and_hms(2026, 7, 3, 12, 1, 10).unwrap(),
        ValidationCommandStatus::Cached,
    );
    repo.add_command_result(&later)
        .await
        .expect("later command should persist");
    repo.add_command_result(&earlier)
        .await
        .expect("earlier command should persist");

    let all_results = repo
        .list_command_results_for_task(&task.id)
        .await
        .expect("command results should list");
    assert_eq!(all_results.len(), 2);
    assert_eq!(all_results[0].id, "command-later");
    assert_eq!(all_results[1].id, "command-earlier");

    let latest = repo
        .latest_run_with_results_for_task(&task.id)
        .await
        .expect("latest run lookup should succeed")
        .expect("latest run should exist");
    assert_eq!(latest.run.id, run.id);
    assert_eq!(latest.run.status, ValidationRunStatus::Passed);
    assert_eq!(latest.run.completed_at, Some(completed_at));
    assert_eq!(
        latest.run.requested_by_agent.as_deref(),
        Some("ralphx-execution-worker")
    );
    assert_eq!(latest.run.mode, ValidationRunMode::ReuseOrRun);
    assert_eq!(
        latest.run.analysis_fingerprint.as_deref(),
        Some("analysis-a")
    );
    assert_eq!(
        latest.run.start_content_fingerprint.as_deref(),
        Some("tree-start")
    );
    assert_eq!(
        latest.run.validated_content_fingerprint.as_deref(),
        Some("tree-validated")
    );
    assert_eq!(latest.commands.len(), 2);
    assert_eq!(latest.commands[0].id, "command-earlier");
    assert_eq!(latest.commands[0].status, ValidationCommandStatus::Cached);
    assert_eq!(latest.commands[0].related_files, earlier.related_files);
    assert_eq!(
        latest.commands[0].launcher_kind.as_deref(),
        Some("production_shell_resolver")
    );
    assert_eq!(latest.commands[1].id, "command-later");
    assert_eq!(latest.commands[1].status, ValidationCommandStatus::Passed);
}

#[tokio::test]
async fn validation_run_repo_promotes_a_matching_validated_run_to_commit() {
    let (_db, repo, task) = setup_repo().await;
    let run = validation_run(&task);
    repo.create_run(&run).await.expect("run should persist");

    repo.record_validated_content_fingerprint(&run.id, Some("tree-final".to_string()))
        .await
        .expect("validated fingerprint should persist");
    repo.promote_run_to_commit(&run.id, "commit-validated-tree")
        .await
        .expect("promotion should persist");

    let latest = repo
        .latest_run_with_results_for_task(&task.id)
        .await
        .expect("latest run lookup should succeed")
        .expect("run should exist");
    assert_eq!(
        latest.run.promoted_commit_sha.as_deref(),
        Some("commit-validated-tree")
    );
    assert_eq!(
        latest.run.validated_content_fingerprint.as_deref(),
        Some("tree-final")
    );
}

#[tokio::test]
async fn validation_run_repo_latest_non_baseline_skips_newer_baseline_run() {
    let (_db, repo, task) = setup_repo().await;
    let mut final_run = validation_run(&task);
    final_run.id = "final-run".to_string();
    final_run.purpose = ValidationPurpose::Final;
    final_run.started_at = Utc.with_ymd_and_hms(2026, 7, 3, 12, 1, 0).unwrap();
    repo.create_run(&final_run)
        .await
        .expect("final run should persist");

    let mut baseline_run = validation_run(&task);
    baseline_run.id = "baseline-run".to_string();
    baseline_run.purpose = ValidationPurpose::Baseline;
    baseline_run.started_at = Utc.with_ymd_and_hms(2026, 7, 3, 12, 2, 0).unwrap();
    repo.create_run(&baseline_run)
        .await
        .expect("baseline run should persist");

    let latest = repo
        .latest_run_with_results_for_task(&task.id)
        .await
        .expect("latest lookup should succeed")
        .expect("latest run should exist");
    assert_eq!(latest.run.id, "baseline-run");

    let latest_non_baseline = repo
        .latest_non_baseline_run_with_results_for_task(&task.id)
        .await
        .expect("latest non-baseline lookup should succeed")
        .expect("latest non-baseline run should exist");
    assert_eq!(latest_non_baseline.run.id, "final-run");
}

#[tokio::test]
async fn validation_run_repo_returns_none_without_runs() {
    let (_db, repo, task) = setup_repo().await;

    let latest = repo
        .latest_run_with_results_for_task(&task.id)
        .await
        .expect("lookup should succeed");

    assert!(latest.is_none());
}

#[tokio::test]
async fn validation_run_repo_marks_only_running_runs_error() {
    let (_db, repo, task) = setup_repo().await;
    let completed_at = Utc.with_ymd_and_hms(2026, 7, 3, 12, 2, 0).unwrap();

    let mut running = validation_run(&task);
    running.id = "running-run".to_string();
    running.started_at = Utc.with_ymd_and_hms(2026, 7, 3, 12, 1, 0).unwrap();
    repo.create_run(&running)
        .await
        .expect("running run should persist");

    let mut passed = validation_run(&task);
    passed.id = "passed-run".to_string();
    passed.status = ValidationRunStatus::Passed;
    passed.started_at = Utc.with_ymd_and_hms(2026, 7, 3, 12, 0, 0).unwrap();
    passed.completed_at = Some(completed_at);
    repo.create_run(&passed)
        .await
        .expect("passed run should persist");

    let marked = repo
        .mark_running_runs_error(completed_at)
        .await
        .expect("running runs should be marked error");
    assert_eq!(marked, 1);

    let latest = repo
        .latest_run_with_results_for_task(&task.id)
        .await
        .expect("latest lookup should succeed")
        .expect("latest run should exist");
    assert_eq!(latest.run.id, "running-run");
    assert_eq!(latest.run.status, ValidationRunStatus::Error);
    assert_eq!(latest.run.completed_at, Some(completed_at));
}
