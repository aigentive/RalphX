use std::sync::Arc;

use async_trait::async_trait;
use chrono::{TimeZone, Utc};

use super::mark_orphaned_validation_runs_on_startup;
use crate::domain::entities::{
    ProjectId, TaskId, ValidationCommandResult, ValidationContextType, ValidationPurpose,
    ValidationRun, ValidationRunMode, ValidationRunStatus, ValidationRunWithResults,
};
use crate::domain::repositories::ValidationRunRepository;
use crate::error::{AppError, AppResult};
use crate::infrastructure::memory::MemoryValidationRunRepository;

fn validation_run(id: &str, task_id: &str, status: ValidationRunStatus) -> ValidationRun {
    ValidationRun {
        id: id.to_string(),
        task_id: TaskId::from_string(task_id.to_string()),
        project_id: ProjectId::from_string("project-startup-cleanup".to_string()),
        purpose: ValidationPurpose::ReExecution,
        context_type: ValidationContextType::ReExecution,
        requested_by_agent: Some("ralphx-execution-worker".to_string()),
        status,
        mode: ValidationRunMode::ReuseOrRun,
        policy_enabled: true,
        head_sha: Some("abcdef1234567890".to_string()),
        start_content_fingerprint: None,
        validated_content_fingerprint: None,
        promoted_commit_sha: None,
        base_ref: Some("main".to_string()),
        analysis_fingerprint: Some("analysis-a".to_string()),
        status_episode_entered_at: None,
        started_at: Utc.with_ymd_and_hms(2026, 7, 12, 10, 0, 0).unwrap(),
        completed_at: None,
    }
}

#[tokio::test]
async fn startup_cleanup_marks_running_validation_runs_error() {
    let repo = Arc::new(MemoryValidationRunRepository::new());
    repo.create_run(&validation_run(
        "running",
        "task-startup-cleanup-running",
        ValidationRunStatus::Running,
    ))
    .await
    .expect("running run should be created");
    repo.create_run(&validation_run(
        "passed",
        "task-startup-cleanup-passed",
        ValidationRunStatus::Passed,
    ))
    .await
    .expect("passed run should be created");

    mark_orphaned_validation_runs_on_startup(repo.clone()).await;

    let running_task_id = TaskId::from_string("task-startup-cleanup-running".to_string());
    let running = repo
        .latest_run_with_results_for_task(&running_task_id)
        .await
        .expect("running run query should succeed")
        .expect("running run should exist");
    assert_eq!(running.run.id, "running");
    assert_eq!(running.run.status, ValidationRunStatus::Error);
    assert!(running.run.completed_at.is_some());

    let passed_task_id = TaskId::from_string("task-startup-cleanup-passed".to_string());
    let passed = repo
        .latest_run_with_results_for_task(&passed_task_id)
        .await
        .expect("passed run query should succeed")
        .expect("passed run should exist");
    assert_eq!(passed.run.id, "passed");
    assert_eq!(passed.run.status, ValidationRunStatus::Passed);
    assert!(passed.run.completed_at.is_none());
}

#[tokio::test]
async fn startup_cleanup_accepts_no_orphaned_validation_runs() {
    let repo = Arc::new(MemoryValidationRunRepository::new());
    repo.create_run(&validation_run(
        "passed",
        "task-startup-cleanup-no-running",
        ValidationRunStatus::Passed,
    ))
    .await
    .expect("passed run should be created");

    mark_orphaned_validation_runs_on_startup(repo.clone()).await;

    let task_id = TaskId::from_string("task-startup-cleanup-no-running".to_string());
    let passed = repo
        .latest_run_with_results_for_task(&task_id)
        .await
        .expect("passed run query should succeed")
        .expect("passed run should exist");
    assert_eq!(passed.run.status, ValidationRunStatus::Passed);
    assert!(passed.run.completed_at.is_none());
}

#[derive(Default)]
struct FailingValidationRunRepository;

#[async_trait]
impl ValidationRunRepository for FailingValidationRunRepository {
    async fn create_run(&self, _run: &ValidationRun) -> AppResult<()> {
        unimplemented!("startup cleanup error test only calls mark_running_runs_error")
    }

    async fn update_run_status(
        &self,
        _run_id: &str,
        _status: ValidationRunStatus,
        _completed_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> AppResult<()> {
        unimplemented!("startup cleanup error test only calls mark_running_runs_error")
    }

    async fn record_validated_content_fingerprint(
        &self,
        _run_id: &str,
        _fingerprint: Option<String>,
    ) -> AppResult<()> {
        unimplemented!("startup cleanup error test only calls mark_running_runs_error")
    }

    async fn promote_run_to_commit(&self, _run_id: &str, _commit_sha: &str) -> AppResult<()> {
        unimplemented!("startup cleanup error test only calls mark_running_runs_error")
    }

    async fn mark_running_runs_error(
        &self,
        _completed_at: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<u64> {
        Err(AppError::Infrastructure(
            "validation repo unavailable".to_string(),
        ))
    }

    async fn add_command_result(&self, _result: &ValidationCommandResult) -> AppResult<()> {
        unimplemented!("startup cleanup error test only calls mark_running_runs_error")
    }

    async fn list_command_results_for_task(
        &self,
        _task_id: &TaskId,
    ) -> AppResult<Vec<ValidationCommandResult>> {
        unimplemented!("startup cleanup error test only calls mark_running_runs_error")
    }

    async fn latest_run_with_results_for_task(
        &self,
        _task_id: &TaskId,
    ) -> AppResult<Option<ValidationRunWithResults>> {
        unimplemented!("startup cleanup error test only calls mark_running_runs_error")
    }

    async fn latest_non_baseline_run_with_results_for_task(
        &self,
        _task_id: &TaskId,
    ) -> AppResult<Option<ValidationRunWithResults>> {
        unimplemented!("startup cleanup error test only calls mark_running_runs_error")
    }
}

#[tokio::test]
async fn startup_cleanup_logs_and_continues_when_validation_repo_fails() {
    let repo = Arc::new(FailingValidationRunRepository);

    mark_orphaned_validation_runs_on_startup(repo).await;
}
