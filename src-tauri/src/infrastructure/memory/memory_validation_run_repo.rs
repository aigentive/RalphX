use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::domain::entities::{
    TaskId, ValidationCommandResult, ValidationPurpose, ValidationRun, ValidationRunStatus,
    ValidationRunWithResults,
};
use crate::domain::repositories::ValidationRunRepository;
use crate::error::AppResult;

#[derive(Default)]
pub struct MemoryValidationRunRepository {
    runs: Arc<RwLock<HashMap<String, ValidationRun>>>,
    commands: Arc<RwLock<Vec<ValidationCommandResult>>>,
}

impl MemoryValidationRunRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ValidationRunRepository for MemoryValidationRunRepository {
    async fn create_run(&self, run: &ValidationRun) -> AppResult<()> {
        self.runs.write().await.insert(run.id.clone(), run.clone());
        Ok(())
    }

    async fn update_run_status(
        &self,
        run_id: &str,
        status: ValidationRunStatus,
        completed_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> AppResult<()> {
        if let Some(run) = self.runs.write().await.get_mut(run_id) {
            run.status = status;
            run.completed_at = completed_at;
        }
        Ok(())
    }

    async fn record_validated_content_fingerprint(
        &self,
        run_id: &str,
        fingerprint: Option<String>,
    ) -> AppResult<()> {
        if let Some(run) = self.runs.write().await.get_mut(run_id) {
            run.validated_content_fingerprint = fingerprint;
        }
        Ok(())
    }

    async fn promote_run_to_commit(&self, run_id: &str, commit_sha: &str) -> AppResult<()> {
        if let Some(run) = self.runs.write().await.get_mut(run_id) {
            run.promoted_commit_sha = Some(commit_sha.to_string());
        }
        Ok(())
    }

    async fn mark_running_runs_error(
        &self,
        completed_at: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<u64> {
        let mut count = 0;
        for run in self.runs.write().await.values_mut() {
            if run.status == ValidationRunStatus::Running {
                run.status = ValidationRunStatus::Error;
                run.completed_at = Some(completed_at);
                count += 1;
            }
        }
        Ok(count)
    }

    async fn add_command_result(&self, result: &ValidationCommandResult) -> AppResult<()> {
        self.commands.write().await.push(result.clone());
        Ok(())
    }

    async fn list_command_results_for_task(
        &self,
        task_id: &TaskId,
    ) -> AppResult<Vec<ValidationCommandResult>> {
        let mut results = self
            .commands
            .read()
            .await
            .iter()
            .filter(|result| &result.task_id == task_id)
            .cloned()
            .collect::<Vec<_>>();
        results.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(results)
    }

    async fn latest_run_with_results_for_task(
        &self,
        task_id: &TaskId,
    ) -> AppResult<Option<ValidationRunWithResults>> {
        self.latest_run_with_results_for_task_matching(task_id, |_| true)
            .await
    }

    async fn latest_non_baseline_run_with_results_for_task(
        &self,
        task_id: &TaskId,
    ) -> AppResult<Option<ValidationRunWithResults>> {
        self.latest_run_with_results_for_task_matching(task_id, |run| {
            run.purpose != ValidationPurpose::Baseline
        })
        .await
    }
}

impl MemoryValidationRunRepository {
    async fn latest_run_with_results_for_task_matching(
        &self,
        task_id: &TaskId,
        matches_run: impl Fn(&ValidationRun) -> bool,
    ) -> AppResult<Option<ValidationRunWithResults>> {
        let run = self
            .runs
            .read()
            .await
            .values()
            .filter(|run| &run.task_id == task_id)
            .filter(|run| matches_run(run))
            .max_by_key(|run| run.started_at)
            .cloned();
        let Some(run) = run else {
            return Ok(None);
        };
        let mut commands = self
            .commands
            .read()
            .await
            .iter()
            .filter(|result| result.validation_run_id == run.id)
            .cloned()
            .collect::<Vec<_>>();
        commands.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        Ok(Some(ValidationRunWithResults { run, commands }))
    }
}
