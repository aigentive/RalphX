use chrono::{Duration, Utc};

use super::SqliteAgentConversationWorkspaceRepository;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentWorkspaceRepairAttempt,
    AgentWorkspaceRepairContinuation, AgentWorkspaceRepairEffect, AgentWorkspaceRepairEffectKind,
    AgentWorkspaceRepairOutcome, AgentWorkspaceRepairPhase, AgentWorkspaceRepairSource,
    ChatConversationId, IdeationAnalysisBaseRefKind, ProjectId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentWorkspaceRepairAttemptTransition,
    AgentWorkspaceRepairAttemptTransitionOutcome, AgentWorkspaceRepairRepository,
    BindAgentWorkspaceRepairAttemptRun, CreateAgentWorkspaceRepairEffect,
    CreateAgentWorkspaceRepairEffectOutcome, SettleAgentWorkspaceRepairAttempt,
    SettleAgentWorkspaceRepairAttemptOutcome, SettleAndStartAgentWorkspaceRepairSuccessor,
    SettleAndStartAgentWorkspaceRepairSuccessorOutcome, StartOrJoinAgentWorkspaceRepairAttempt,
    StartOrJoinAgentWorkspaceRepairAttemptOutcome,
};
use crate::testing::SqliteTestDb;

pub(super) fn setup_repo() -> (
    SqliteTestDb,
    SqliteAgentConversationWorkspaceRepository,
    ChatConversationId,
) {
    let db = SqliteTestDb::new("sqlite_repair_attempt_fencing_tests");
    let conversation_id = ChatConversationId::from_string("sqlite-repair-fencing");
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO chat_conversations (id, context_type, context_id, created_at, updated_at)
             VALUES (?1, 'project', 'project-repair-fencing', ?2, ?2)",
            rusqlite::params![conversation_id.as_str(), Utc::now().to_rfc3339()],
        )
        .expect("seed conversation");
    });
    let repo = SqliteAgentConversationWorkspaceRepository::from_shared(db.shared_conn());
    (db, repo, conversation_id)
}

#[tokio::test]
async fn settled_and_cross_conversation_attempts_cannot_transition_or_bind_runs() {
    let (_db, repo, conversation_id) = setup_repo();
    let dispatching = start_dispatching(&repo, conversation_id.clone()).await;
    let settled = match repo
        .settle_repair_attempt(SettleAgentWorkspaceRepairAttempt {
            attempt_id: dispatching.id.clone(),
            generation: dispatching.generation,
            expected_phase: dispatching.phase,
            expected_updated_at: dispatching.updated_at,
            outcome: AgentWorkspaceRepairOutcome::Succeeded,
            settled_at: dispatching.updated_at + Duration::seconds(1),
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("settle attempt")
    {
        SettleAgentWorkspaceRepairAttemptOutcome::Applied(attempt) => attempt,
        outcome => panic!("expected settled attempt, got {outcome:?}"),
    };

    let mut revive = settled.clone();
    revive.phase = AgentWorkspaceRepairPhase::Repairing;
    revive.settled_at = None;
    revive.outcome = None;
    revive.updated_at += Duration::seconds(1);
    let transition = repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: revive,
            expected_phase: settled.phase,
            expected_updated_at: settled.updated_at,
            next_phase: AgentWorkspaceRepairPhase::Repairing,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("reject settled transition");
    assert!(matches!(
        transition,
        AgentWorkspaceRepairAttemptTransitionOutcome::Stale(ref attempt)
            if attempt.settled_at == settled.settled_at && attempt.outcome == settled.outcome
    ));

    let bound = repo
        .bind_repair_attempt_run(BindAgentWorkspaceRepairAttemptRun {
            attempt_id: settled.id.clone(),
            generation: settled.generation,
            expected_phase: settled.phase,
            expected_updated_at: settled.updated_at,
            run_id: crate::domain::entities::AgentRunId::from_string("settled-sqlite-run"),
            updated_at: settled.updated_at + Duration::seconds(2),
        })
        .await
        .expect("reject settled run binding");
    assert!(matches!(
        bound,
        AgentWorkspaceRepairAttemptTransitionOutcome::Stale(ref attempt)
            if attempt.reserved_agent_run_id == settled.reserved_agent_run_id
    ));

    let (cross_db, cross_repo, active_conversation) = setup_repo();
    let active = start_dispatching(&cross_repo, active_conversation.clone()).await;
    let other_conversation =
        ChatConversationId::from_string("5b9171f8-3e63-4e7e-a462-5f2eef0b32ca");
    cross_db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO chat_conversations (id, context_type, context_id, created_at, updated_at)
             VALUES (?1, 'project', 'project-repair-fencing', ?2, ?2)",
            rusqlite::params![other_conversation.as_str(), Utc::now().to_rfc3339()],
        )
        .expect("seed other conversation");
    });
    cross_repo
        .create_or_update(workspace(other_conversation.clone()))
        .await
        .expect("persist other workspace");
    let before_other_workspace = cross_repo
        .get_by_conversation_id(&other_conversation)
        .await
        .expect("load other workspace")
        .expect("other workspace exists");
    let mut cross_conversation = active.clone();
    cross_conversation.conversation_id = other_conversation.clone();
    cross_conversation.phase = AgentWorkspaceRepairPhase::Repairing;
    cross_conversation.updated_at += Duration::seconds(1);
    let outcome = cross_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: cross_conversation,
            expected_phase: active.phase,
            expected_updated_at: active.updated_at,
            next_phase: AgentWorkspaceRepairPhase::Repairing,
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
            events: vec![event(other_conversation.clone(), "cross-conversation")],
        })
        .await
        .expect("reject cross-conversation transition");
    let AgentWorkspaceRepairAttemptTransitionOutcome::Stale(stale) = outcome else {
        panic!("cross-conversation transition must be stale")
    };
    assert_eq!(stale.conversation_id, active_conversation);
    assert_eq!(
        cross_repo
            .get_by_conversation_id(&other_conversation)
            .await
            .expect("reload other workspace"),
        Some(before_other_workspace)
    );
    assert!(cross_repo
        .list_publication_events(&other_conversation)
        .await
        .expect("list other events")
        .is_empty());
}

#[tokio::test]
async fn corrupt_repair_attempt_phase_and_effect_kind_fail_closed_on_read() {
    let (db, repo, conversation_id) = setup_repo();
    let dispatching = start_dispatching(&repo, conversation_id).await;
    db.with_connection(|conn| {
        conn.execute_batch("PRAGMA ignore_check_constraints = ON;")
            .expect("permit corrupt test row");
        conn.execute(
            "UPDATE agent_workspace_repair_attempts SET phase = 'unknown_phase' WHERE id = ?1",
            rusqlite::params![dispatching.id.as_str()],
        )
        .expect("corrupt repair attempt phase");
    });
    assert!(repo.get_repair_attempt(&dispatching.id).await.is_err());

    let (db, repo, conversation_id) = setup_repo();
    let dispatching = start_dispatching(&repo, conversation_id).await;
    let effect = AgentWorkspaceRepairEffect::new(
        dispatching.id.clone(),
        AgentWorkspaceRepairEffectKind::PushBranch,
        "corrupt-effect-kind",
        dispatching.updated_at,
    );
    assert!(matches!(
        repo.create_repair_effect(CreateAgentWorkspaceRepairEffect {
            attempt_id: dispatching.id.clone(),
            generation: dispatching.generation,
            expected_phase: dispatching.phase,
            expected_attempt_updated_at: dispatching.updated_at,
            effect: effect.clone(),
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("create effect"),
        CreateAgentWorkspaceRepairEffectOutcome::Created(_)
    ));
    db.with_connection(|conn| {
        conn.execute_batch("PRAGMA ignore_check_constraints = ON;")
            .expect("permit corrupt test row");
        conn.execute(
            "UPDATE agent_workspace_repair_effects SET kind = 'unknown_kind' WHERE id = ?1",
            rusqlite::params![effect.id.as_str()],
        )
        .expect("corrupt repair effect kind");
    });
    assert!(repo
        .get_repair_effect_by_idempotency_key(&effect.idempotency_key)
        .await
        .is_err());
}

fn workspace(conversation_id: ChatConversationId) -> AgentConversationWorkspace {
    AgentConversationWorkspace::new(
        conversation_id,
        ProjectId::from_string("project-repair-fencing".to_string()),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("base-1".to_string()),
        "ralphx/project-repair-fencing/agent".to_string(),
        "/tmp/ralphx/project-repair-fencing/agent".to_string(),
    )
}

pub(super) fn attempt(conversation_id: ChatConversationId) -> AgentWorkspaceRepairAttempt {
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

pub(super) fn event(
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

pub(super) async fn start_dispatching(
    repo: &SqliteAgentConversationWorkspaceRepository,
    conversation_id: ChatConversationId,
) -> AgentWorkspaceRepairAttempt {
    repo.create_or_update(workspace(conversation_id.clone()))
        .await
        .expect("persist workspace");
    let started = match repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: attempt(conversation_id),
            reason: "base moved".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("start attempt")
    {
        StartOrJoinAgentWorkspaceRepairAttemptOutcome::Started(attempt) => attempt,
        outcome => panic!("expected start, got {outcome:?}"),
    };
    let mut dispatching = started.clone();
    dispatching.phase = AgentWorkspaceRepairPhase::Dispatching;
    dispatching.updated_at += Duration::seconds(1);
    repo.transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
        attempt: dispatching.clone(),
        expected_phase: AgentWorkspaceRepairPhase::Requested,
        expected_updated_at: started.updated_at,
        next_phase: AgentWorkspaceRepairPhase::Dispatching,
        compatibility_projection: None,
        events: Vec::new(),
    })
    .await
    .expect("move to dispatching");
    dispatching
}

pub(super) async fn join_same_phase(
    repo: &SqliteAgentConversationWorkspaceRepository,
    conversation_id: ChatConversationId,
    current: &AgentWorkspaceRepairAttempt,
) -> AgentWorkspaceRepairAttempt {
    let mut join = attempt(conversation_id);
    join.updated_at = current.updated_at + Duration::seconds(1);
    match repo
        .start_or_join_repair_attempt(StartOrJoinAgentWorkspaceRepairAttempt {
            attempt: join,
            reason: "same-phase retry".to_string(),
            verified_newer_base: false,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("join same phase")
    {
        StartOrJoinAgentWorkspaceRepairAttemptOutcome::Joined(attempt) => attempt,
        outcome => panic!("expected join, got {outcome:?}"),
    }
}

#[tokio::test]
async fn successor_fencing_rejects_same_phase_stale_and_duplicate_settlement_without_side_effects()
{
    let (_db, repo, conversation_id) = setup_repo();
    let dispatching = start_dispatching(&repo, conversation_id.clone()).await;
    let current = join_same_phase(&repo, conversation_id.clone(), &dispatching).await;
    let before_workspace = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace exists");
    let stale_successor = attempt(conversation_id.clone());
    let stale_id = stale_successor.id.clone();
    let stale = repo
        .settle_and_start_repair_successor(SettleAndStartAgentWorkspaceRepairSuccessor {
            attempt_id: current.id.clone(),
            generation: current.generation,
            expected_phase: AgentWorkspaceRepairPhase::Dispatching,
            expected_updated_at: dispatching.updated_at,
            outcome: AgentWorkspaceRepairOutcome::Superseded,
            settled_at: current.updated_at + Duration::seconds(1),
            successor: StartOrJoinAgentWorkspaceRepairAttempt {
                attempt: stale_successor,
                reason: "stale successor".to_string(),
                verified_newer_base: false,
                compatibility_projection: None,
                events: vec![event(conversation_id.clone(), "stale-successor")],
            },
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
            events: vec![event(conversation_id.clone(), "stale-settlement")],
        })
        .await
        .expect("reject stale settlement");
    assert!(matches!(
        stale,
        SettleAndStartAgentWorkspaceRepairSuccessorOutcome::Stale(ref attempt)
            if attempt.updated_at == current.updated_at && attempt.settled_at.is_none()
    ));
    assert!(repo
        .get_repair_attempt(&stale_id)
        .await
        .expect("load stale successor")
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
        .expect("list events")
        .is_empty());

    let accepted = match repo
        .settle_and_start_repair_successor(SettleAndStartAgentWorkspaceRepairSuccessor {
            attempt_id: current.id.clone(),
            generation: current.generation,
            expected_phase: AgentWorkspaceRepairPhase::Dispatching,
            expected_updated_at: current.updated_at,
            outcome: AgentWorkspaceRepairOutcome::Succeeded,
            settled_at: current.updated_at + Duration::seconds(2),
            successor: StartOrJoinAgentWorkspaceRepairAttempt {
                attempt: attempt(conversation_id.clone()),
                reason: "accepted successor".to_string(),
                verified_newer_base: false,
                compatibility_projection: None,
                events: Vec::new(),
            },
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await
        .expect("settle once")
    {
        SettleAndStartAgentWorkspaceRepairSuccessorOutcome::Started(successor) => successor,
        outcome => panic!("expected successor, got {outcome:?}"),
    };
    let settled_parent = repo
        .get_repair_attempt(&current.id)
        .await
        .expect("load parent")
        .expect("parent exists");
    let duplicate = attempt(conversation_id.clone());
    let duplicate_id = duplicate.id.clone();
    let duplicate = repo
        .settle_and_start_repair_successor(SettleAndStartAgentWorkspaceRepairSuccessor {
            attempt_id: current.id.clone(),
            generation: current.generation,
            expected_phase: AgentWorkspaceRepairPhase::Dispatching,
            expected_updated_at: settled_parent.updated_at,
            outcome: AgentWorkspaceRepairOutcome::Succeeded,
            settled_at: settled_parent.updated_at + Duration::seconds(1),
            successor: StartOrJoinAgentWorkspaceRepairAttempt {
                attempt: duplicate,
                reason: "duplicate successor".to_string(),
                verified_newer_base: false,
                compatibility_projection: None,
                events: vec![event(conversation_id.clone(), "duplicate-successor")],
            },
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
            events: vec![event(conversation_id.clone(), "duplicate-settlement")],
        })
        .await
        .expect("reject duplicate settlement");
    assert!(matches!(
        duplicate,
        SettleAndStartAgentWorkspaceRepairSuccessorOutcome::Stale(_)
    ));
    assert!(repo
        .get_repair_attempt(&duplicate_id)
        .await
        .expect("load duplicate successor")
        .is_none());
    assert_eq!(
        repo.get_current_repair_attempt(&conversation_id)
            .await
            .expect("load active successor")
            .expect("one active successor")
            .id,
        accepted.id
    );
    assert_eq!(
        repo.get_by_conversation_id(&conversation_id)
            .await
            .expect("reload workspace after duplicate"),
        Some(before_workspace)
    );
    assert!(repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list duplicate events")
        .is_empty());
}
