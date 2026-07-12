use std::sync::Arc;

use chrono::{TimeZone, Utc};

use super::mark_orphaned_validation_runs_on_startup;
use crate::domain::entities::{
    ProjectId, TaskId, ValidationContextType, ValidationPurpose, ValidationRun, ValidationRunMode,
    ValidationRunStatus,
};
use crate::domain::repositories::ValidationRunRepository;
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
