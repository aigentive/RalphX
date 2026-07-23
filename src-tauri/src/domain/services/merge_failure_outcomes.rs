use std::sync::Arc;

use serde_json::Value;

use crate::domain::entities::{
    Task, TaskOutcome, TaskOutcomeClass, TaskOutcomeSource, TaskOutcomeStatus,
};
use crate::domain::repositories::TaskOutcomeRepository;
use crate::error::{AppError, AppResult};

use super::failure_fingerprint::{attach_recurrence_evidence, failure_fingerprint};
use super::{new_empty_task_outcome, OutcomeLedgerService};

pub(crate) async fn record_merge_failure_outcome(
    repo: Arc<dyn TaskOutcomeRepository>,
    task: &Task,
    class: TaskOutcomeClass,
    attempt: u32,
    mut evidence: Value,
    fingerprint_evidence: &str,
) -> AppResult<TaskOutcome> {
    if !matches!(
        class,
        TaskOutcomeClass::MergeConflict
            | TaskOutcomeClass::MergeQaFailed
            | TaskOutcomeClass::MergeTimeout
    ) {
        return Err(AppError::Validation(format!(
            "{} is not a merge failure summary class",
            class.as_str()
        )));
    }
    if attempt == 0 {
        return Err(AppError::Validation(
            "merge failure attempt must be positive".to_string(),
        ));
    }

    let evidence_object = evidence.as_object_mut().ok_or_else(|| {
        AppError::Validation("merge failure evidence must be an object".to_string())
    })?;
    evidence_object.insert("task_id".to_string(), Value::String(task.id.to_string()));
    evidence_object.insert("attempt".to_string(), Value::from(attempt));
    evidence_object.insert(
        "failure_class".to_string(),
        Value::String(class.as_str().to_string()),
    );
    let trusted_session = task.ideation_session_id.as_ref().map(|id| id.as_str());
    attach_recurrence_evidence(&mut evidence, fingerprint_evidence, trusted_session);

    let mut outcome = new_empty_task_outcome(
        task.project_id.clone(),
        TaskOutcomeSource::Merge,
        "merge_attempt",
        format!("{}:attempt:{attempt}", task.id.as_str()),
    );
    outcome.task_id = Some(task.id.as_str().to_string());
    outcome.outcome_class = Some(class.clone());
    outcome.status = TaskOutcomeStatus::Failed;
    outcome.failure_fingerprint = Some(failure_fingerprint(&class, fingerprint_evidence));
    outcome.evidence_json = evidence;

    OutcomeLedgerService::new(repo)
        .record_outcome(outcome)
        .await
}
