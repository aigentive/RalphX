use async_trait::async_trait;

use crate::domain::entities::types::ProjectId;
use crate::domain::entities::{
    TaskOutcome, TaskOutcomeClass, TaskOutcomeId, TaskOutcomeSource, TaskOutcomeStatus,
};
use crate::error::AppResult;

pub const AGENT_WORKSPACE_PR_OUTCOME_SOURCE: TaskOutcomeSource =
    TaskOutcomeSource::AgentWorkspacePr;
pub const TERMINAL_PR_SOURCE_REF_KIND: &str = "pull_request";
pub const WORKSPACE_PR_TERMINAL_CLASS: &str = "workspace_pr_terminal";
pub const WORKSPACE_PR_CLOSED_CLASS: &str = "workspace_pr_closed";
pub const WORKSPACE_PR_FAILED_CLASS: &str = "workspace_pr_failed";
pub const WORKSPACE_PR_MERGED_CLASS: &str = "workspace_pr_merged";
pub const WORKSPACE_PR_MERGED_CLEAN_CLASS: &str = "workspace_pr_merged_clean";
pub const WORKSPACE_PR_MERGED_WITH_FOLLOWUPS_CLASS: &str = "workspace_pr_merged_with_followups";
pub const WORKSPACE_SESSION_ABANDONED_CLASS: &str = "workspace_session_abandoned";
pub const WORKSPACE_PUBLISH_FAILED_CLASS: &str = "workspace_publish_failed";

#[derive(Debug, Clone)]
pub struct ResolvedTaskOutcomeUpsert {
    pub outcome: TaskOutcome,
    pub should_write: bool,
}

pub fn canonical_terminal_pr_source_ref_id(pull_request_id: &str) -> String {
    format!("{pull_request_id}:terminal")
}

pub fn terminal_pr_status_for_class(outcome_class: Option<&TaskOutcomeClass>) -> TaskOutcomeStatus {
    match outcome_class {
        Some(TaskOutcomeClass::WorkspacePrClosed | TaskOutcomeClass::WorkspacePrFailed) => {
            TaskOutcomeStatus::Failed
        }
        Some(
            TaskOutcomeClass::WorkspacePrMerged
            | TaskOutcomeClass::WorkspacePrMergedClean
            | TaskOutcomeClass::WorkspacePrMergedWithFollowups,
        ) => TaskOutcomeStatus::Succeeded,
        _ => TaskOutcomeStatus::Unknown,
    }
}

fn terminal_pr_class_rank(outcome_class: Option<&TaskOutcomeClass>) -> u8 {
    match outcome_class {
        Some(TaskOutcomeClass::WorkspacePrClosed | TaskOutcomeClass::WorkspacePrFailed) => 1,
        Some(TaskOutcomeClass::WorkspacePrMerged) => 2,
        Some(
            TaskOutcomeClass::WorkspacePrMergedClean
            | TaskOutcomeClass::WorkspacePrMergedWithFollowups,
        ) => 3,
        _ => 0,
    }
}

fn is_canonical_terminal_pr_outcome(outcome: &TaskOutcome) -> bool {
    let Some(pull_request_id) = outcome.pull_request_id.as_deref() else {
        return false;
    };
    outcome.source == AGENT_WORKSPACE_PR_OUTCOME_SOURCE
        && outcome.source_ref_kind == TERMINAL_PR_SOURCE_REF_KIND
        && outcome.source_ref_id == canonical_terminal_pr_source_ref_id(pull_request_id)
}

fn preserve_row_identity(existing: &TaskOutcome, incoming: &mut TaskOutcome) {
    incoming.id = existing.id.clone();
    incoming.created_at = existing.created_at;
}

fn merge_missing_terminal_context(existing: &TaskOutcome, incoming: &mut TaskOutcome) {
    incoming.task_id = incoming.task_id.take().or_else(|| existing.task_id.clone());
    incoming.conversation_id = incoming
        .conversation_id
        .take()
        .or_else(|| existing.conversation_id.clone());
    incoming.agent_run_id = incoming
        .agent_run_id
        .take()
        .or_else(|| existing.agent_run_id.clone());
    incoming.pull_request_id = incoming
        .pull_request_id
        .take()
        .or_else(|| existing.pull_request_id.clone());
    incoming.proposal_id = incoming
        .proposal_id
        .take()
        .or_else(|| existing.proposal_id.clone());
    incoming.verification_id = incoming
        .verification_id
        .take()
        .or_else(|| existing.verification_id.clone());
    incoming.review_id = incoming
        .review_id
        .take()
        .or_else(|| existing.review_id.clone());
    incoming.provider_harness = incoming
        .provider_harness
        .take()
        .or_else(|| existing.provider_harness.clone());
    incoming.provider_session_id = incoming
        .provider_session_id
        .take()
        .or_else(|| existing.provider_session_id.clone());
}

fn preserve_existing_terminal_reason(existing: &TaskOutcome, incoming: &mut TaskOutcome) {
    if incoming.outcome_class != existing.outcome_class
        || incoming.evidence_json.get("reason").is_some()
    {
        return;
    }
    let Some(reason) = existing.evidence_json.get("reason").cloned() else {
        return;
    };
    let Some(evidence) = incoming.evidence_json.as_object_mut() else {
        return;
    };
    evidence.insert("reason".to_string(), reason);
}

pub fn resolve_task_outcome_upsert(
    existing: Option<&TaskOutcome>,
    mut incoming: TaskOutcome,
) -> ResolvedTaskOutcomeUpsert {
    let incoming_is_terminal = is_canonical_terminal_pr_outcome(&incoming);
    if incoming_is_terminal {
        incoming.status = terminal_pr_status_for_class(incoming.outcome_class.as_ref());
    }

    let Some(existing) = existing else {
        return ResolvedTaskOutcomeUpsert {
            outcome: incoming,
            should_write: true,
        };
    };

    if incoming_is_terminal && is_canonical_terminal_pr_outcome(existing) {
        if terminal_pr_class_rank(incoming.outcome_class.as_ref())
            < terminal_pr_class_rank(existing.outcome_class.as_ref())
        {
            return ResolvedTaskOutcomeUpsert {
                outcome: existing.clone(),
                should_write: false,
            };
        }
        merge_missing_terminal_context(existing, &mut incoming);
        preserve_existing_terminal_reason(existing, &mut incoming);
    }

    preserve_row_identity(existing, &mut incoming);
    ResolvedTaskOutcomeUpsert {
        outcome: incoming,
        should_write: true,
    }
}

#[derive(Debug, Clone)]
pub struct UpsertTaskOutcomeInput {
    pub outcome: TaskOutcome,
}

#[derive(Debug, Clone, Default)]
pub struct TaskOutcomeListOptions {
    pub source: Option<TaskOutcomeSource>,
    pub status: Option<TaskOutcomeStatus>,
}

#[async_trait]
pub trait TaskOutcomeRepository: Send + Sync {
    async fn upsert(&self, input: UpsertTaskOutcomeInput) -> AppResult<TaskOutcome>;

    async fn get_by_dedupe(
        &self,
        project_id: &ProjectId,
        source: TaskOutcomeSource,
        source_ref_kind: &str,
        source_ref_id: &str,
    ) -> AppResult<Option<TaskOutcome>>;

    async fn get_by_id(&self, id: &TaskOutcomeId) -> AppResult<Option<TaskOutcome>>;

    async fn list_by_project(
        &self,
        project_id: &ProjectId,
        options: TaskOutcomeListOptions,
    ) -> AppResult<Vec<TaskOutcome>>;
}
