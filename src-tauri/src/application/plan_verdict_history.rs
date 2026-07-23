use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::domain::entities::{
    ProjectId, TaskOutcome, TaskOutcomeClass, TaskOutcomeSource, TaskOutcomeStatus,
};
use crate::domain::repositories::{PlanApprovalActor, TaskOutcomeRepository};
use crate::domain::services::{new_empty_task_outcome, OutcomeLedgerService};
use crate::error::AppResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlanVerdict {
    Accepted,
    Declined,
    RevisionRequested,
}

impl PlanVerdict {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Declined => "declined",
            Self::RevisionRequested => "revision_requested",
        }
    }

    fn outcome_class(self) -> TaskOutcomeClass {
        match self {
            Self::Accepted => TaskOutcomeClass::PlanModeAccepted,
            Self::Declined => TaskOutcomeClass::PlanModeDeclined,
            Self::RevisionRequested => TaskOutcomeClass::PlanModeRevisionRequested,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PlanVerdictCapture {
    pub project_id: ProjectId,
    pub conversation_id: Option<String>,
    pub session_id: String,
    pub artifact_id: String,
    pub artifact_version: u32,
    pub actor: PlanApprovalActor,
    pub verdict: PlanVerdict,
    pub origin: &'static str,
    pub summary: Option<String>,
}

pub(crate) async fn record_plan_verdict(
    repo: Arc<dyn TaskOutcomeRepository>,
    capture: PlanVerdictCapture,
) -> AppResult<TaskOutcome> {
    let identity = serde_json::json!({
        "session_id": capture.session_id,
        "artifact_id": capture.artifact_id,
        "artifact_version": capture.artifact_version,
        "actor": capture.actor.as_str(),
        "verdict": capture.verdict.as_str(),
    });
    let identity_json = serde_json::to_vec(&identity).map_err(|error| {
        crate::error::AppError::Infrastructure(format!(
            "failed to encode plan verdict identity: {error}"
        ))
    })?;
    let source_ref_id = format!(
        "plan-verdict-v1:{:x}",
        Sha256::digest(identity_json.as_slice())
    );
    let mut outcome = new_empty_task_outcome(
        capture.project_id,
        TaskOutcomeSource::PlanMode,
        "plan_verdict",
        source_ref_id,
    );
    outcome.conversation_id = capture.conversation_id;
    outcome.outcome_class = Some(capture.verdict.outcome_class());
    outcome.status = TaskOutcomeStatus::Eligible;
    outcome.evidence_json = serde_json::json!({
        "identity": identity,
        "origin": capture.origin,
        "summary": capture.summary,
    });
    OutcomeLedgerService::new(repo)
        .record_outcome(outcome)
        .await
}
