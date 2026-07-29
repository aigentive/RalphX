use chrono::{Duration, Utc};
use std::sync::Arc;

use super::MemoryAgentConversationWorkspaceRepository;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentRunId, AgentWorkspaceRepairAttempt,
    AgentWorkspaceRepairContinuation, AgentWorkspaceRepairEffect, AgentWorkspaceRepairEffectKind,
    AgentWorkspaceRepairEffectStatus, AgentWorkspaceRepairOutcome, AgentWorkspaceRepairPhase,
    AgentWorkspaceRepairSource, ChatConversationId, IdeationAnalysisBaseRefKind, ProjectId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentWorkspaceRepairAttemptTransition,
    AgentWorkspaceRepairAttemptTransitionOutcome, AgentWorkspaceRepairCompatibilityProjection,
    AgentWorkspaceRepairRepository, BindAgentWorkspaceRepairAttemptRun,
    CompleteAgentWorkspaceRepairEffect, CompleteAgentWorkspaceRepairEffectOutcome,
    CreateAgentWorkspaceRepairEffect, CreateAgentWorkspaceRepairEffectOutcome,
    ImportLegacyAgentWorkspaceRepairAttempt, ImportLegacyAgentWorkspaceRepairAttemptOutcome,
    SettleAndStartAgentWorkspaceRepairSuccessor,
    SettleAndStartAgentWorkspaceRepairSuccessorOutcome, StartOrJoinAgentWorkspaceRepairAttempt,
    StartOrJoinAgentWorkspaceRepairAttemptOutcome,
};

fn workspace(conversation_id: ChatConversationId) -> AgentConversationWorkspace {
    AgentConversationWorkspace::new(
        conversation_id,
        ProjectId::from_string("project-repair".to_string()),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("base-1".to_string()),
        "ralphx/project-repair/agent".to_string(),
        "/tmp/ralphx/project-repair/agent".to_string(),
    )
}

#[tokio::test]
async fn bind_repair_run_rejects_a_stale_same_phase_snapshot() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("memory-repair-bind-fence");
    repo.create_or_update(workspace(conversation_id.clone()))
        .await
        .expect("persist workspace");
    let started = repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: repair_attempt(conversation_id.clone()),
            reason: "first repair".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start repair attempt");
    let StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(stale) = started else {
        panic!("first repair generation must start");
    };
    let mut join = repair_attempt(conversation_id);
    join.updated_at = stale.updated_at + Duration::microseconds(1);
    repo.start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
        attempt: join,
        reason: "same-phase join".to_string(),
        verified_newer_base: false,
        compatibility_projection: None,
        events: Vec::new(),
    })
    .await
    .expect("join current repair generation");

    let bound = repo
        .bind_repair_attempt_run(BindAgentWorkspaceRepairAttemptRun {
            attempt_id: stale.id.clone(),
            generation: stale.generation,
            expected_phase: AgentWorkspaceRepairPhase::Requested,
            expected_updated_at: stale.updated_at,
            run_id: AgentRunId::from_string("stale-memory-repair-run"),
            updated_at: stale.updated_at + Duration::seconds(1),
        })
        .await
        .expect("stale binding is a normal CAS outcome");
    assert!(matches!(
        bound,
        AgentWorkspaceRepairAttemptTransitionOutcome::Stale(ref attempt)
            if attempt.reserved_agent_run_id.is_none() && attempt.updated_at > stale.updated_at
    ));
}

#[tokio::test]
async fn concurrent_legacy_import_loses_to_durable_generation_without_projection_or_event_replay() {
    let repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let conversation_id = ChatConversationId::from_string("legacy-import-existing-memory");
    repo.create_or_update(workspace(conversation_id.clone()))
        .await
        .expect("persist workspace");
    let before_workspace = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace exists");
    let legacy_event = publication_event(conversation_id.clone(), "legacy_repair_imported");
    let mut legacy_attempt = repair_attempt(conversation_id.clone());
    legacy_attempt.source = AgentWorkspaceRepairSource::Legacy;
    legacy_attempt.generation = 1;
    legacy_attempt.phase = AgentWorkspaceRepairPhase::Repairing;
    let start_durable = async {
        repo.start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: repair_attempt(conversation_id.clone()),
            reason: "concurrent durable start".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
    };
    let import_legacy = async {
        // Give durable creation the first transaction/lock acquisition, as a concurrent
        // startup/import race must join the new durable owner rather than replay legacy state.
        tokio::task::yield_now().await;
        repo.import_legacy_repair_attempt(ImportLegacyAgentWorkspaceRepairAttempt {
            attempt: legacy_attempt,
            compatibility_projection: Some(AgentWorkspaceRepairCompatibilityProjection {
                publication_push_status: Some("legacy-mutated".to_string()),
                pr_supervision_status: Some("legacy-mutated".to_string()),
                pr_supervision_summary: Some("must not replay".to_string()),
                pr_supervision_updated_at: Some(Utc::now()),
                pr_auto_merge_current: Some(true),
                base_commit: Some("legacy-base".to_string()),
            }),
            events: vec![legacy_event],
        })
        .await
    };
    let (start_outcome, outcome) = tokio::join!(start_durable, import_legacy);
    let durable = match start_outcome.expect("start durable generation") {
        StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(attempt) => attempt,
        outcome => panic!("expected durable start, got {outcome:?}"),
    };
    let outcome = outcome.expect("legacy import loses to durable generation");
    assert!(matches!(
        outcome,
        ImportLegacyAgentWorkspaceRepairAttemptOutcome::ExistingDurable(ref attempt)
            if attempt.id == durable.id
    ));
    assert_eq!(
        repo.get_by_conversation_id(&conversation_id)
            .await
            .expect("reload workspace")
            .expect("workspace exists"),
        before_workspace
    );
    assert!(repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list events")
        .is_empty());
    assert!(repo
        .get_open_repair_effect(&durable.id)
        .await
        .expect("load effects")
        .is_none());
}

fn repair_attempt(conversation_id: ChatConversationId) -> AgentWorkspaceRepairAttempt {
    AgentWorkspaceRepairAttempt::new(
        conversation_id,
        AgentWorkspaceRepairSource::BaseUpdate,
        AgentWorkspaceRepairContinuation::UpdateOnly,
        "origin/main",
        false,
        false,
        false,
        None,
        Utc::now(),
    )
}

fn publication_event(
    conversation_id: ChatConversationId,
    step: &str,
) -> AgentConversationWorkspacePublicationEvent {
    AgentConversationWorkspacePublicationEvent::new(
        conversation_id,
        step,
        "succeeded",
        format!("repair {step}"),
        Some("repair".to_string()),
    )
}

#[tokio::test]
async fn repair_attempt_cas_effect_and_successor_match_sqlite_behavior() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("repair-attempt-memory");
    repo.create_or_update(workspace(conversation_id.clone()))
        .await
        .expect("persist workspace");

    let started = match repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: repair_attempt(conversation_id.clone()),
            reason: "base moved".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start repair attempt")
    {
        StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(attempt) => attempt,
        outcome => panic!("expected started repair attempt, got {outcome:?}"),
    };
    assert_eq!(started.generation, 1);

    let mut stale_attempt = started.clone();
    stale_attempt.phase = AgentWorkspaceRepairPhase::Repairing;
    stale_attempt.updated_at += Duration::seconds(1);
    let stale_event = publication_event(conversation_id.clone(), "stale-transition");
    let stale = repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: stale_attempt,
            expected_phase: AgentWorkspaceRepairPhase::Dispatching,
            expected_updated_at: started.updated_at,
            next_phase: AgentWorkspaceRepairPhase::Repairing,
            compatibility_projection: Some(AgentWorkspaceRepairCompatibilityProjection {
                publication_push_status: Some("should-not-write".to_string()),
                pr_supervision_status: None,
                pr_supervision_summary: None,
                pr_supervision_updated_at: None,
                pr_auto_merge_current: None,
                base_commit: None,
            }),
            events: vec![stale_event],
        })
        .await
        .expect("reject stale repair transition");
    assert!(matches!(
        stale,
        AgentWorkspaceRepairAttemptTransitionOutcome::Stale(_)
    ));
    assert!(repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list events after stale cas")
        .is_empty());
    assert_eq!(
        repo.get_repair_attempt(&started.id)
            .await
            .expect("reload repair attempt")
            .expect("attempt exists")
            .phase,
        AgentWorkspaceRepairPhase::Requested
    );

    let mut dispatching = started.clone();
    dispatching.phase = AgentWorkspaceRepairPhase::Dispatching;
    dispatching.updated_at += Duration::seconds(2);
    let applied_event = publication_event(conversation_id.clone(), "dispatching");
    let applied = repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: dispatching.clone(),
            expected_phase: AgentWorkspaceRepairPhase::Requested,
            expected_updated_at: started.updated_at,
            next_phase: AgentWorkspaceRepairPhase::Dispatching,
            compatibility_projection: Some(AgentWorkspaceRepairCompatibilityProjection {
                publication_push_status: Some("repairing".to_string()),
                pr_supervision_status: Some("repairing".to_string()),
                pr_supervision_summary: Some("Updating base".to_string()),
                pr_supervision_updated_at: Some(dispatching.updated_at),
                pr_auto_merge_current: Some(false),
                base_commit: Some("base-2".to_string()),
            }),
            events: vec![applied_event.clone()],
        })
        .await
        .expect("apply repair transition");
    assert!(matches!(
        applied,
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(_)
    ));
    assert_eq!(
        repo.get_by_conversation_id(&conversation_id)
            .await
            .expect("load workspace")
            .expect("workspace exists")
            .publication_push_status
            .as_deref(),
        Some("repairing")
    );
    assert_eq!(
        repo.list_publication_events(&conversation_id)
            .await
            .expect("list applied event"),
        vec![applied_event]
    );

    let effect = AgentWorkspaceRepairEffect::new(
        dispatching.id.clone(),
        AgentWorkspaceRepairEffectKind::PushBranch,
        "push:repair-attempt-memory",
        dispatching.updated_at,
    );
    let created = repo
        .create_repair_effect(CreateAgentWorkspaceRepairEffect {
            attempt_id: dispatching.id.clone(),
            generation: dispatching.generation,
            expected_phase: AgentWorkspaceRepairPhase::Dispatching,
            expected_attempt_updated_at: dispatching.updated_at,
            effect: effect.clone(),
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("create repair effect");
    assert!(matches!(
        created,
        CreateAgentWorkspaceRepairEffectOutcome::Created(_)
    ));

    let mut observed = effect.clone();
    observed.status = AgentWorkspaceRepairEffectStatus::Observed;
    observed.receipt_json = Some("{\"remote_oid\":\"abc\"}".to_string());
    observed.completed_at = Some(effect.created_at + Duration::seconds(1));
    observed.updated_at = observed.completed_at.expect("completion timestamp");
    let settled_at = observed.updated_at + Duration::seconds(1);
    let completed = repo
        .complete_repair_effect(CompleteAgentWorkspaceRepairEffect {
            attempt_id: dispatching.id.clone(),
            generation: dispatching.generation,
            expected_phase: AgentWorkspaceRepairPhase::Dispatching,
            expected_attempt_updated_at: dispatching.updated_at,
            expected_effect_updated_at: effect.updated_at,
            expected_effect_status: AgentWorkspaceRepairEffectStatus::Pending,
            effect: observed,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("complete repair effect");
    assert!(matches!(
        completed,
        CompleteAgentWorkspaceRepairEffectOutcome::Applied(_)
    ));
    assert!(repo
        .get_open_repair_effect(&dispatching.id)
        .await
        .expect("load open effect")
        .is_none());
    let reloaded = repo
        .get_repair_effect_by_idempotency_key(&effect.idempotency_key)
        .await
        .expect("reload observed effect by idempotency key")
        .expect("observed effect exists");
    assert_eq!(reloaded.id, effect.id);
    assert_eq!(reloaded.status, AgentWorkspaceRepairEffectStatus::Observed);
    assert_eq!(
        reloaded.receipt_json.as_deref(),
        Some("{\"remote_oid\":\"abc\"}")
    );
    assert_eq!(
        reloaded.completed_at,
        Some(effect.created_at + Duration::seconds(1))
    );

    let mut failed = AgentWorkspaceRepairEffect::new(
        dispatching.id.clone(),
        AgentWorkspaceRepairEffectKind::PushBranch,
        "push:failed-memory",
        settled_at,
    );
    failed.status = AgentWorkspaceRepairEffectStatus::InFlight;
    let failed = match repo
        .create_repair_effect(CreateAgentWorkspaceRepairEffect {
            attempt_id: dispatching.id.clone(),
            generation: dispatching.generation,
            expected_phase: AgentWorkspaceRepairPhase::Dispatching,
            expected_attempt_updated_at: dispatching.updated_at,
            effect: failed,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("create failed repair effect")
    {
        CreateAgentWorkspaceRepairEffectOutcome::Created(effect) => effect,
        outcome => panic!("expected failed effect creation, got {outcome:?}"),
    };
    let mut failed = failed;
    let expected_effect_updated_at = failed.updated_at;
    failed.status = AgentWorkspaceRepairEffectStatus::Failed;
    failed.last_error = Some("ambiguous remote OID".to_string());
    failed.updated_at += Duration::seconds(1);
    let failed = match repo
        .complete_repair_effect(CompleteAgentWorkspaceRepairEffect {
            attempt_id: dispatching.id.clone(),
            generation: dispatching.generation,
            expected_phase: AgentWorkspaceRepairPhase::Dispatching,
            expected_attempt_updated_at: dispatching.updated_at,
            expected_effect_updated_at,
            expected_effect_status: AgentWorkspaceRepairEffectStatus::InFlight,
            effect: failed,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("record failed repair effect")
    {
        CompleteAgentWorkspaceRepairEffectOutcome::Applied(effect) => effect,
        outcome => panic!("expected failed effect completion, got {outcome:?}"),
    };
    assert_eq!(failed.status, AgentWorkspaceRepairEffectStatus::Failed);
    assert!(failed.completed_at.is_none());
    assert!(repo
        .get_open_repair_effect(&dispatching.id)
        .await
        .expect("failed effect cannot hold the repair lease")
        .is_none());
    assert!(repo
        .get_repair_effect_by_idempotency_key("push:missing-memory")
        .await
        .expect("look up missing idempotency key")
        .is_none());

    let stale_successor = repair_attempt(conversation_id.clone());
    let stale_successor_id = stale_successor.id.clone();
    let stale = repo
        .settle_and_start_repair_successor(SettleAndStartAgentWorkspaceRepairSuccessor {
            attempt_id: dispatching.id.clone(),
            generation: dispatching.generation + 1,
            expected_phase: AgentWorkspaceRepairPhase::Dispatching,
            expected_updated_at: dispatching.updated_at,
            outcome: AgentWorkspaceRepairOutcome::Superseded,
            settled_at,
            successor: StartOrJoinAgentWorkspaceRepairAttempt {
                attempt: stale_successor,
                reason: "stale retry".to_string(),
                verified_newer_base: false,
                compatibility_projection: Some(AgentWorkspaceRepairCompatibilityProjection {
                    publication_push_status: Some("must-not-project".to_string()),
                    pr_supervision_status: Some("must-not-project".to_string()),
                    pr_supervision_summary: None,
                    pr_supervision_updated_at: None,
                    pr_auto_merge_current: None,
                    base_commit: None,
                }),
                events: Vec::new(),
            },
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("reject stale successor generation");
    assert!(matches!(
        stale,
        SettleAndStartAgentWorkspaceRepairSuccessorOutcome::Stale(_)
    ));
    assert!(repo
        .get_repair_attempt(&stale_successor_id)
        .await
        .expect("load stale successor")
        .is_none());
    assert_eq!(
        repo.get_by_conversation_id(&conversation_id)
            .await
            .expect("load workspace after stale successor")
            .expect("workspace exists")
            .publication_push_status
            .as_deref(),
        Some("repairing")
    );

    let invalid_successor = repair_attempt(ChatConversationId::new());
    let invalid_successor_id = invalid_successor.id.clone();
    let failure = repo
        .settle_and_start_repair_successor(SettleAndStartAgentWorkspaceRepairSuccessor {
            attempt_id: dispatching.id.clone(),
            generation: dispatching.generation,
            expected_phase: AgentWorkspaceRepairPhase::Dispatching,
            expected_updated_at: dispatching.updated_at,
            outcome: AgentWorkspaceRepairOutcome::Superseded,
            settled_at,
            successor: StartOrJoinAgentWorkspaceRepairAttempt {
                attempt: invalid_successor,
                reason: "invalid retry".to_string(),
                verified_newer_base: false,
                compatibility_projection: None,
                events: Vec::new(),
            },
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await;
    assert!(
        failure.is_err(),
        "mismatched successor must fail without committing state: {failure:?}"
    );
    assert!(repo
        .get_repair_attempt(&invalid_successor_id)
        .await
        .expect("load invalid successor")
        .is_none());
    assert_eq!(
        repo.get_repair_attempt(&dispatching.id)
            .await
            .expect("reload attempt after failed successor")
            .expect("attempt exists")
            .phase,
        AgentWorkspaceRepairPhase::Dispatching
    );
    let workspace = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace after failed successor")
        .expect("workspace exists");
    assert_eq!(
        workspace.publication_push_status.as_deref(),
        Some("repairing")
    );
    assert_eq!(
        workspace.pr_supervision_status.as_deref(),
        Some("repairing")
    );

    let successor = repair_attempt(conversation_id.clone());
    let successor_projection = AgentWorkspaceRepairCompatibilityProjection {
        publication_push_status: Some("needs_agent".to_string()),
        pr_supervision_status: Some("fixing".to_string()),
        pr_supervision_summary: Some("Retry requested".to_string()),
        pr_supervision_updated_at: Some(successor.updated_at),
        pr_auto_merge_current: None,
        base_commit: None,
    };
    let started_successor = repo
        .settle_and_start_repair_successor(SettleAndStartAgentWorkspaceRepairSuccessor {
            attempt_id: dispatching.id.clone(),
            generation: dispatching.generation,
            expected_phase: AgentWorkspaceRepairPhase::Dispatching,
            expected_updated_at: dispatching.updated_at,
            outcome: AgentWorkspaceRepairOutcome::Succeeded,
            settled_at,
            successor: StartOrJoinAgentWorkspaceRepairAttempt {
                attempt: successor,
                reason: "publish continuation".to_string(),
                verified_newer_base: false,
                compatibility_projection: Some(successor_projection),
                events: Vec::new(),
            },
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("settle and start successor");
    let successor = match started_successor {
        SettleAndStartAgentWorkspaceRepairSuccessorOutcome::Started(attempt) => attempt,
        outcome => panic!("expected successor, got {outcome:?}"),
    };
    assert_eq!(successor.generation, 2);
    let workspace = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace after successor")
        .expect("workspace exists");
    assert_eq!(
        workspace.publication_push_status.as_deref(),
        Some("needs_agent")
    );
    assert_eq!(workspace.pr_supervision_status.as_deref(), Some("fixing"));
    assert_eq!(
        repo.get_current_repair_attempt(&conversation_id)
            .await
            .expect("load current repair attempt")
            .expect("successor is current")
            .id,
        successor.id
    );
    assert_eq!(
        repo.get_repair_effect_by_idempotency_key(&effect.idempotency_key)
            .await
            .expect("reload observed effect after successor")
            .expect("observed effect survives successor")
            .receipt_json
            .as_deref(),
        Some("{\"remote_oid\":\"abc\"}")
    );
}
