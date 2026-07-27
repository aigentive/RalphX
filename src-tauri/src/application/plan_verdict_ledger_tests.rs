use std::sync::Arc;

use super::plan_verdict_ledger::{
    record_plan_verdict, PlanVerdict, PlanVerdictLedgerInput, PlanVerdictLinkage,
};
use crate::domain::entities::{ProjectId, TaskOutcomeSource};
use crate::domain::repositories::{
    PlanApprovalActor, TaskOutcomeListOptions, TaskOutcomeRepository,
};
use crate::infrastructure::memory::MemoryTaskOutcomeRepository;

fn input(actor: PlanApprovalActor, verdict: PlanVerdict) -> PlanVerdictLedgerInput {
    PlanVerdictLedgerInput {
        project_id: ProjectId::from_string("project-verdict".to_string()),
        session_id: "session:with:delimiters".to_string(),
        artifact_id: "artifact|with|delimiters".to_string(),
        artifact_version: 7,
        actor,
        verdict,
        source_surface: "plan_mode_http".to_string(),
        linkage: PlanVerdictLinkage {
            conversation_id: Some("conversation-verdict".to_string()),
            automation_id: Some("automation-verdict".to_string()),
            imported_from_session_id: None,
        },
        reason: Some("Needs one more failure-path test".to_string()),
    }
}

#[tokio::test]
async fn verdict_history_is_actor_version_verdict_scoped_and_exact_retry_is_idempotent() {
    let repo = Arc::new(MemoryTaskOutcomeRepository::new());

    let accepted = record_plan_verdict(
        repo.clone() as Arc<dyn TaskOutcomeRepository>,
        input(PlanApprovalActor::User, PlanVerdict::Accepted),
    )
    .await
    .unwrap();
    let retried = record_plan_verdict(
        repo.clone() as Arc<dyn TaskOutcomeRepository>,
        input(PlanApprovalActor::User, PlanVerdict::Accepted),
    )
    .await
    .unwrap();
    assert_eq!(accepted.id, retried.id);

    record_plan_verdict(
        repo.clone() as Arc<dyn TaskOutcomeRepository>,
        input(PlanApprovalActor::Judge, PlanVerdict::RevisionRequested),
    )
    .await
    .unwrap();
    record_plan_verdict(
        repo.clone() as Arc<dyn TaskOutcomeRepository>,
        input(PlanApprovalActor::Judge, PlanVerdict::Accepted),
    )
    .await
    .unwrap();
    record_plan_verdict(
        repo.clone() as Arc<dyn TaskOutcomeRepository>,
        input(PlanApprovalActor::PlanImport, PlanVerdict::Accepted),
    )
    .await
    .unwrap();
    let mut next_version = input(PlanApprovalActor::User, PlanVerdict::Accepted);
    next_version.artifact_version = 8;
    record_plan_verdict(repo.clone() as Arc<dyn TaskOutcomeRepository>, next_version)
        .await
        .unwrap();

    let outcomes = repo
        .list_by_project(
            &ProjectId::from_string("project-verdict".to_string()),
            TaskOutcomeListOptions {
                source: Some(TaskOutcomeSource::PlanMode),
                ..TaskOutcomeListOptions::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(outcomes.len(), 5);
    assert!(outcomes
        .iter()
        .any(|outcome| outcome.outcome_class.as_ref().unwrap().as_str() == "accepted"));
    assert!(outcomes.iter().any(|outcome| {
        outcome.outcome_class.as_ref().unwrap().as_str() == "revision_requested"
    }));
    assert!(outcomes.iter().all(|outcome| {
        outcome.source_ref_kind == "plan_verdict"
            && outcome
                .evidence_json
                .get("artifact_version")
                .and_then(|value| value.as_u64())
                .is_some()
    }));
    for actor in ["user", "judge", "plan_import"] {
        assert!(outcomes.iter().any(|outcome| {
            outcome
                .evidence_json
                .get("actor")
                .and_then(|value| value.as_str())
                == Some(actor)
        }));
    }
    assert!(outcomes.iter().any(|outcome| {
        outcome
            .evidence_json
            .get("artifact_version")
            .and_then(|value| value.as_u64())
            == Some(8)
    }));
}

#[test]
fn deterministic_key_is_collision_safe_for_delimiter_bearing_ids() {
    let left = input(PlanApprovalActor::User, PlanVerdict::Accepted).source_ref_id();
    let mut right_input = input(PlanApprovalActor::User, PlanVerdict::Accepted);
    right_input.session_id = "session".to_string();
    right_input.artifact_id = "with:delimiters|artifact|with|delimiters".to_string();

    assert_ne!(left, right_input.source_ref_id());
    assert_eq!(
        left,
        input(PlanApprovalActor::User, PlanVerdict::Accepted).source_ref_id()
    );
}
