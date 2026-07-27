use chrono::Duration;

use super::repair_attempt_fencing_tests::{
    attempt, event, join_same_phase, setup_repo, start_dispatching,
};
use crate::domain::entities::{
    AgentWorkspaceRepairEffect, AgentWorkspaceRepairEffectKind, AgentWorkspaceRepairEffectStatus,
    AgentWorkspaceRepairOutcome, AgentWorkspaceRepairPhase,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentWorkspaceRepairRepository,
    CompleteAgentWorkspaceRepairEffect, CompleteAgentWorkspaceRepairEffectOutcome,
    CreateAgentWorkspaceRepairEffect, CreateAgentWorkspaceRepairEffectOutcome,
    SettleAndStartAgentWorkspaceRepairSuccessor, StartOrJoinAgentWorkspaceRepairAttempt,
};

#[tokio::test]
async fn effect_fencing_rejects_stale_create_and_completion_but_replays_exact_completed_receipts() {
    let (_db, repo, conversation_id) = setup_repo();
    let dispatching = start_dispatching(&repo, conversation_id.clone()).await;
    let current = join_same_phase(&repo, conversation_id.clone(), &dispatching).await;
    let before_workspace = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace exists");
    let stale_create = repo
        .create_repair_effect(CreateAgentWorkspaceRepairEffect {
            attempt_id: current.id.clone(),
            generation: current.generation,
            expected_phase: AgentWorkspaceRepairPhase::Dispatching,
            expected_attempt_updated_at: dispatching.updated_at,
            effect: AgentWorkspaceRepairEffect::new(
                current.id.clone(),
                AgentWorkspaceRepairEffectKind::PushBranch,
                "push:stale-sqlite-effect",
                current.updated_at,
            ),
            compatibility_projection: Some(
                crate::domain::repositories::AgentWorkspaceRepairCompatibilityProjection {
                    publication_push_status: Some("must-not-project".to_string()),
                    pr_supervision_status: None,
                    pr_supervision_summary: None,
                    pr_supervision_updated_at: None,
                    pr_auto_merge_current: None,
                    base_commit: None,
                },
            ),
            events: vec![event(conversation_id.clone(), "stale-effect-create")],
        })
        .await
        .expect("reject stale effect create");
    assert!(matches!(
        stale_create,
        CreateAgentWorkspaceRepairEffectOutcome::Stale(_)
    ));
    assert!(repo
        .get_repair_effect_by_idempotency_key("push:stale-sqlite-effect")
        .await
        .expect("load stale effect")
        .is_none());

    let mut observed = AgentWorkspaceRepairEffect::new(
        current.id.clone(),
        AgentWorkspaceRepairEffectKind::PushBranch,
        "push:observed-sqlite-effect",
        current.updated_at,
    );
    observed.status = AgentWorkspaceRepairEffectStatus::InFlight;
    let observed = match repo
        .create_repair_effect(CreateAgentWorkspaceRepairEffect {
            attempt_id: current.id.clone(),
            generation: current.generation,
            expected_phase: AgentWorkspaceRepairPhase::Dispatching,
            expected_attempt_updated_at: current.updated_at,
            effect: observed,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("create observed effect")
    {
        CreateAgentWorkspaceRepairEffectOutcome::Created(effect) => effect,
        outcome => panic!("expected effect create, got {outcome:?}"),
    };
    let mut completed = observed.clone();
    completed.status = AgentWorkspaceRepairEffectStatus::Observed;
    completed.receipt_json = Some("{\"remote_oid\":\"abc\"}".to_string());
    completed.updated_at += Duration::seconds(1);
    completed.completed_at = Some(completed.updated_at);
    let completed = match repo
        .complete_repair_effect(CompleteAgentWorkspaceRepairEffect {
            attempt_id: current.id.clone(),
            generation: current.generation,
            expected_phase: AgentWorkspaceRepairPhase::Dispatching,
            expected_attempt_updated_at: current.updated_at,
            expected_effect_updated_at: observed.updated_at,
            expected_effect_status: AgentWorkspaceRepairEffectStatus::InFlight,
            effect: completed,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("complete observed effect")
    {
        CompleteAgentWorkspaceRepairEffectOutcome::Applied(effect) => effect,
        outcome => panic!("expected observed completion, got {outcome:?}"),
    };

    let mut racing = AgentWorkspaceRepairEffect::new(
        current.id.clone(),
        AgentWorkspaceRepairEffectKind::PushBranch,
        "push:racing-sqlite-effect",
        completed.updated_at + Duration::seconds(1),
    );
    racing.status = AgentWorkspaceRepairEffectStatus::InFlight;
    let racing = match repo
        .create_repair_effect(CreateAgentWorkspaceRepairEffect {
            attempt_id: current.id.clone(),
            generation: current.generation,
            expected_phase: AgentWorkspaceRepairPhase::Dispatching,
            expected_attempt_updated_at: current.updated_at,
            effect: racing,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("create racing effect")
    {
        CreateAgentWorkspaceRepairEffectOutcome::Created(effect) => effect,
        outcome => panic!("expected racing effect, got {outcome:?}"),
    };
    let mut failed = racing.clone();
    failed.status = AgentWorkspaceRepairEffectStatus::Failed;
    failed.last_error = Some("ambiguous remote OID".to_string());
    failed.updated_at += Duration::seconds(1);
    repo.complete_repair_effect(CompleteAgentWorkspaceRepairEffect {
        attempt_id: current.id.clone(),
        generation: current.generation,
        expected_phase: AgentWorkspaceRepairPhase::Dispatching,
        expected_attempt_updated_at: current.updated_at,
        expected_effect_updated_at: racing.updated_at,
        expected_effect_status: AgentWorkspaceRepairEffectStatus::InFlight,
        effect: failed.clone(),
        compatibility_projection: None,
        events: Vec::new(),
    })
    .await
    .expect("record failed effect");
    let mut stale_observed = racing.clone();
    stale_observed.status = AgentWorkspaceRepairEffectStatus::Observed;
    stale_observed.receipt_json = Some("{\"remote_oid\":\"new\"}".to_string());
    stale_observed.updated_at += Duration::seconds(2);
    stale_observed.completed_at = Some(stale_observed.updated_at);
    let stale_complete = repo
        .complete_repair_effect(CompleteAgentWorkspaceRepairEffect {
            attempt_id: current.id.clone(),
            generation: current.generation,
            expected_phase: AgentWorkspaceRepairPhase::Dispatching,
            expected_attempt_updated_at: current.updated_at,
            expected_effect_updated_at: racing.updated_at,
            expected_effect_status: AgentWorkspaceRepairEffectStatus::InFlight,
            effect: stale_observed,
            compatibility_projection: Some(
                crate::domain::repositories::AgentWorkspaceRepairCompatibilityProjection {
                    publication_push_status: Some("must-not-project".to_string()),
                    pr_supervision_status: None,
                    pr_supervision_summary: None,
                    pr_supervision_updated_at: None,
                    pr_auto_merge_current: None,
                    base_commit: None,
                },
            ),
            events: vec![event(conversation_id.clone(), "stale-effect-complete")],
        })
        .await
        .expect("reject stale effect completion");
    assert!(matches!(
        stale_complete,
        CompleteAgentWorkspaceRepairEffectOutcome::Stale(_)
    ));
    assert_eq!(
        repo.get_repair_effect_by_idempotency_key("push:racing-sqlite-effect")
            .await
            .expect("reload failed effect")
            .expect("failed effect exists")
            .status,
        AgentWorkspaceRepairEffectStatus::Failed
    );
    assert_eq!(
        repo.get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace after stale completion"),
        Some(before_workspace.clone())
    );
    assert!(repo
        .list_publication_events(&conversation_id)
        .await
        .expect("events after stale completion")
        .is_empty());

    repo.settle_and_start_repair_successor(SettleAndStartAgentWorkspaceRepairSuccessor {
        attempt_id: current.id.clone(),
        generation: current.generation,
        expected_phase: AgentWorkspaceRepairPhase::Dispatching,
        expected_updated_at: current.updated_at,
        outcome: AgentWorkspaceRepairOutcome::Succeeded,
        settled_at: current.updated_at + Duration::seconds(3),
        successor: StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: attempt(conversation_id.clone()),
            reason: "settle after effects".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        },
        compatibility_projection: None,
        events: Vec::new(),
    })
    .await
    .expect("settle attempt");
    let replay = repo
        .complete_repair_effect(CompleteAgentWorkspaceRepairEffect {
            attempt_id: current.id.clone(),
            generation: current.generation,
            expected_phase: AgentWorkspaceRepairPhase::Dispatching,
            expected_attempt_updated_at: current.updated_at,
            expected_effect_updated_at: completed.updated_at,
            expected_effect_status: AgentWorkspaceRepairEffectStatus::Observed,
            effect: completed.clone(),
            compatibility_projection: Some(
                crate::domain::repositories::AgentWorkspaceRepairCompatibilityProjection {
                    publication_push_status: Some("must-not-project".to_string()),
                    pr_supervision_status: None,
                    pr_supervision_summary: None,
                    pr_supervision_updated_at: None,
                    pr_auto_merge_current: None,
                    base_commit: None,
                },
            ),
            events: vec![event(conversation_id.clone(), "idempotent-complete")],
        })
        .await
        .expect("replay completed effect");
    assert!(matches!(
        replay,
        CompleteAgentWorkspaceRepairEffectOutcome::Applied(ref effect)
            if effect == &completed
    ));
    assert_eq!(
        repo.get_by_conversation_id(&conversation_id)
            .await
            .expect("workspace after replay"),
        Some(before_workspace)
    );
    assert!(repo
        .list_publication_events(&conversation_id)
        .await
        .expect("events after replay")
        .is_empty());
}
