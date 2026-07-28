use std::sync::Arc;

use chrono::Utc;
use serde_json::json;

use crate::domain::entities::{
    ProjectId, TaskOutcome, TaskOutcomeClass, TaskOutcomeId, TaskOutcomeSource, TaskOutcomeStatus,
};
use crate::domain::repositories::{PlanApprovalActor, TaskOutcomeRepository};
use crate::domain::services::OutcomeLedgerService;
use crate::error::AppResult;

const PLAN_VERDICT_SOURCE_REF_KIND: &str = "plan_verdict";
const MAX_VERDICT_REASON_CHARS: usize = 500;

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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PlanVerdictLinkage {
    pub conversation_id: Option<String>,
    pub automation_id: Option<String>,
    pub imported_from_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlanVerdictLedgerInput {
    pub project_id: ProjectId,
    pub session_id: String,
    pub artifact_id: String,
    pub artifact_version: u32,
    pub actor: PlanApprovalActor,
    pub verdict: PlanVerdict,
    pub source_surface: String,
    pub linkage: PlanVerdictLinkage,
    pub reason: Option<String>,
}

impl PlanVerdictLedgerInput {
    pub(crate) fn source_ref_id(&self) -> String {
        let actor = self.actor.as_str();
        let verdict = self.verdict.as_str();
        format!(
            "s{}:{}a{}:{}v{}u{}:{}d{}:{}",
            self.session_id.len(),
            self.session_id,
            self.artifact_id.len(),
            self.artifact_id,
            self.artifact_version,
            actor.len(),
            actor,
            verdict.len(),
            verdict,
        )
    }
}

fn compact_reason(reason: Option<&str>) -> Option<String> {
    reason.map(|reason| reason.chars().take(MAX_VERDICT_REASON_CHARS).collect())
}

pub(crate) async fn record_plan_verdict(
    outcome_repo: Arc<dyn TaskOutcomeRepository>,
    input: PlanVerdictLedgerInput,
) -> AppResult<TaskOutcome> {
    let now = Utc::now();
    let source_ref_id = input.source_ref_id();
    let conversation_id = input.linkage.conversation_id.clone();
    let evidence_json = json!({
        "session_id": input.session_id,
        "artifact_id": input.artifact_id,
        "artifact_version": input.artifact_version,
        "actor": input.actor.as_str(),
        "verdict": input.verdict.as_str(),
        "source_surface": input.source_surface,
        "conversation_id": input.linkage.conversation_id,
        "automation_id": input.linkage.automation_id,
        "imported_from_session_id": input.linkage.imported_from_session_id,
        "reason": compact_reason(input.reason.as_deref()),
    });
    OutcomeLedgerService::new(outcome_repo)
        .record_outcome(TaskOutcome {
            id: TaskOutcomeId::new(),
            project_id: input.project_id,
            source: TaskOutcomeSource::PlanMode,
            source_ref_kind: PLAN_VERDICT_SOURCE_REF_KIND.to_string(),
            source_ref_id,
            task_id: None,
            conversation_id,
            agent_run_id: None,
            pull_request_id: None,
            proposal_id: None,
            verification_id: None,
            review_id: None,
            outcome_class: Some(input.verdict.outcome_class()),
            status: TaskOutcomeStatus::Eligible,
            evidence_json,
            failure_fingerprint: None,
            provider_harness: None,
            provider_session_id: None,
            created_at: now,
            updated_at: now,
        })
        .await
}
