use chrono::Duration;

use super::repair_attempt_fencing_tests::{
    event, join_same_phase, repair_attempt, start_dispatching,
};
use super::MemoryAgentConversationWorkspaceRepository;
use crate::domain::entities::{
    AgentWorkspaceRepairEffect, AgentWorkspaceRepairEffectKind, AgentWorkspaceRepairEffectStatus,
    AgentWorkspaceRepairOutcome, AgentWorkspaceRepairPhase, ChatConversationId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentWorkspaceRepairRepository,
    CompleteAgentWorkspaceRepairEffect, CompleteAgentWorkspaceRepairEffectOutcome,
    CreateAgentWorkspaceRepairEffect, CreateAgentWorkspaceRepairEffectOutcome,
    SettleAndStartAgentWorkspaceRepairSuccessor, StartOrJoinAgentWorkspaceRepairAttempt,
};

#[tokio::test]
async fn effect_writes_reject_stale_attempt_or_effect_versions_and_keep_exact_completion_idempotent(
) {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("memory-effect-fencing");
    let dispatching = start_dispatching(&repo, conversation_id.clone()).await;
    let current = join_same_phase(&repo, conversation_id.clone(), &dispatching).await;
    let before_workspace = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace exists");
    let stale_effect = AgentWorkspaceRepairEffect::new(
        current.id.clone(),
        AgentWorkspaceRepairEffectKind::PushBranch,
        "push:stale-memory-effect",
        current.updated_at,
    );
    let stale_create = repo
        .create_repair_effect(CreateAgentWorkspaceRepairEffect {
            attempt_id: current.id.clone(),
            generation: current.generation,
            expected_phase: AgentWorkspaceRepairPhase::Dispatching,
            expected_attempt_updated_at: dispatching.updated_at,
            effect: stale_effect,
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
        .expect("reject stale effect creation");
    assert!(matches!(
        stale_create,
        CreateAgentWorkspaceRepairEffectOutcome::Stale(ref attempt)
            if attempt.updated_at == current.updated_at
    ));
    assert!(repo
        .get_repair_effect_by_idempotency_key("push:stale-memory-effect")
        .await
        .expect("lookup stale effect")
        .is_none());
    assert_eq!(
        repo.get_by_conversation_id(&conversation_id)
            .await
            .expect("reload workspace"),
        Some(before_workspace.clone())
    );
    assert!(repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list stale events")
        .is_empty());

    let mut observed = AgentWorkspaceRepairEffect::new(
        current.id.clone(),
        AgentWorkspaceRepairEffectKind::PushBranch,
        "push:observed-memory-effect",
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
        .expect("create effect")
    {
        CreateAgentWorkspaceRepairEffectOutcome::Created(effect) => effect,
        outcome => panic!("expected create, got {outcome:?}"),
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
        .expect("complete effect")
    {
        CompleteAgentWorkspaceRepairEffectOutcome::Applied(effect) => effect,
        outcome => panic!("expected completion, got {outcome:?}"),
    };

    let mut racing = AgentWorkspaceRepairEffect::new(
        current.id.clone(),
        AgentWorkspaceRepairEffectKind::PushBranch,
        "push:racing-memory-effect",
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
        repo.get_repair_effect_by_idempotency_key("push:racing-memory-effect")
            .await
            .expect("reload failed effect")
            .expect("failed effect exists")
            .status,
        AgentWorkspaceRepairEffectStatus::Failed
    );
    assert_eq!(
        repo.get_by_conversation_id(&conversation_id)
            .await
            .expect("reload workspace after race"),
        Some(before_workspace.clone())
    );
    assert!(repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list race events")
        .is_empty());

    let successor = repair_attempt(conversation_id.clone());
    repo.settle_and_start_repair_successor(SettleAndStartAgentWorkspaceRepairSuccessor {
        attempt_id: current.id.clone(),
        generation: current.generation,
        expected_phase: AgentWorkspaceRepairPhase::Dispatching,
        expected_updated_at: current.updated_at,
        outcome: AgentWorkspaceRepairOutcome::Succeeded,
        settled_at: current.updated_at + Duration::seconds(3),
        successor: StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: successor,
            reason: "settle after effects".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        },
        compatibility_projection: None,
        events: Vec::new(),
    })
    .await
    .expect("settle attempt after effect completion");
    let idempotent = repo
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
        .expect("idempotent completion");
    assert!(matches!(
        idempotent,
        CompleteAgentWorkspaceRepairEffectOutcome::Applied(ref effect)
            if effect == &completed
    ));
    assert_eq!(
        repo.get_by_conversation_id(&conversation_id)
            .await
            .expect("reload workspace after replay"),
        Some(before_workspace)
    );
    assert!(repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list replay events")
        .is_empty());
}
