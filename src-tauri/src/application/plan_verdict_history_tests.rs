use std::sync::Arc;

use crate::application::plan_verdict_history::{
    record_plan_verdict, PlanVerdict, PlanVerdictCapture,
};
use crate::domain::entities::{ProjectId, TaskOutcomeSource};
use crate::domain::repositories::{
    PlanApprovalActor, TaskOutcomeListOptions, TaskOutcomeRepository,
};
use crate::infrastructure::memory::MemoryTaskOutcomeRepository;

fn capture(
    actor: PlanApprovalActor,
    verdict: PlanVerdict,
    artifact_version: u32,
) -> PlanVerdictCapture {
    PlanVerdictCapture {
        project_id: ProjectId::from_string("project-1".to_string()),
        conversation_id: Some("conversation-1".to_string()),
        session_id: "session-1".to_string(),
        artifact_id: "artifact-1".to_string(),
        artifact_version,
        actor,
        verdict,
        origin: "test_flow",
        summary: Some("reviewed".to_string()),
    }
}

#[tokio::test]
async fn exact_plan_verdict_delivery_is_idempotent() {
    let repo = Arc::new(MemoryTaskOutcomeRepository::new());

    let first = record_plan_verdict(
        repo.clone(),
        capture(PlanApprovalActor::User, PlanVerdict::Accepted, 2),
    )
    .await
    .expect("record first verdict");
    let second = record_plan_verdict(
        repo.clone(),
        capture(PlanApprovalActor::User, PlanVerdict::Accepted, 2),
    )
    .await
    .expect("record duplicate verdict");

    let rows = repo
        .list_by_project(
            &ProjectId::from_string("project-1".to_string()),
            TaskOutcomeListOptions {
                source: Some(TaskOutcomeSource::PlanMode),
                ..TaskOutcomeListOptions::default()
            },
        )
        .await
        .expect("list verdicts");
    assert_eq!(rows.len(), 1);
    assert_eq!(first.id, second.id);
    assert_eq!(rows[0].source_ref_kind, "plan_verdict");
}

#[tokio::test]
async fn actor_version_and_verdict_each_create_distinct_history() {
    let repo = Arc::new(MemoryTaskOutcomeRepository::new());
    for input in [
        capture(PlanApprovalActor::User, PlanVerdict::Accepted, 2),
        capture(PlanApprovalActor::User, PlanVerdict::Declined, 2),
        capture(PlanApprovalActor::Judge, PlanVerdict::Accepted, 2),
        capture(PlanApprovalActor::User, PlanVerdict::Accepted, 3),
        capture(PlanApprovalActor::Judge, PlanVerdict::RevisionRequested, 3),
    ] {
        record_plan_verdict(repo.clone(), input)
            .await
            .expect("record distinct verdict");
    }

    let rows = repo
        .list_by_project(
            &ProjectId::from_string("project-1".to_string()),
            TaskOutcomeListOptions {
                source: Some(TaskOutcomeSource::PlanMode),
                ..TaskOutcomeListOptions::default()
            },
        )
        .await
        .expect("list verdicts");
    assert_eq!(rows.len(), 5);
    assert!(rows.iter().any(|row| {
        row.outcome_class
            .as_ref()
            .is_some_and(|class| class.as_str() == "plan_mode_revision_requested")
    }));
}
